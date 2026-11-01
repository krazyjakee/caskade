//! Making a backup: plan, snapshot, write, verify, then publish under the
//! final name. Until that last rename the destination holds only hidden
//! `.caskade-backup-*.partial*` files, which the next backup there clears.

use super::{
    BackupError, Backups,
    archive::{self, Source},
    manifest::{Component, ComponentSummary, Exclusion, Manifest},
    snapshot,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

/// Partial files' names start with this.
const PARTIAL: &str = ".caskade-backup-";
/// Finished backups' names start with this.
pub const PREFIX: &str = "caskade-backup-";
pub const EXTENSION: &str = "zip";
/// Room kept free beyond the estimate.
const MARGIN: u64 = 16 * 1024 * 1024;

/// What to back up, and where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupExportOptions")]
pub struct ExportOptions {
    /// An existing folder.
    pub destination: PathBuf,
    /// The optional components to add; the library is always in.
    pub components: BTreeSet<Component>,
    /// The automatic backup set (#193), recorded in the manifest.
    pub set_id: Option<String>,
}

/// A dry run's answer: what a backup would hold and leave out, and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupPlan {
    pub destination: PathBuf,
    pub components: Vec<PlannedComponent>,
    pub excluded: Vec<Exclusion>,
    /// The selected components' size, uncompressed.
    pub estimated_bytes: u64,
    /// Free space at the destination; `None` when it can't be read (or the
    /// folder is missing).
    pub available_bytes: Option<u64>,
    pub destination_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupPlannedComponent")]
pub struct PlannedComponent {
    pub id: Component,
    pub selected: bool,
    pub files: u32,
    pub bytes: u64,
}

/// Where a backup is up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupPhase")]
pub enum Phase {
    Snapshot,
    Writing,
    Verifying,
    /// A restore's files coming out of the archive.
    Extracting,
    /// A restore's database brought up to this Caskade's schema.
    Migrating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupProgress")]
pub struct Progress {
    pub phase: Phase,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// A finished, verified backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupExported")]
pub struct Exported {
    pub path: PathBuf,
    pub bytes: u64,
    pub manifest: Manifest,
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

impl Backups {
    /// What a backup with `options` would hold, without writing anything.
    pub async fn plan(&self, options: &ExportOptions) -> Result<BackupPlan, BackupError> {
        let library = self.store.used_bytes().await?;
        let artwork = self.store.artwork_hashes().await?;
        let paths = self.paths.clone();
        let destination = options.destination.clone();
        let (sizes, mut available, exists) =
            caskade_jobs::blocking("Planning a backup", move || {
                let sizes: Vec<(Component, u32, u64)> = Component::ALL
                    .into_iter()
                    .filter(|c| *c != Component::Library)
                    .map(|component| {
                        let files = files(&paths, component, &artwork, Path::new(""));
                        let bytes = files
                            .iter()
                            .filter_map(|f| std::fs::symlink_metadata(&f.file).ok())
                            .map(|m| m.len())
                            .sum();
                        (
                            component,
                            u32::try_from(files.len()).unwrap_or(u32::MAX),
                            bytes,
                        )
                    })
                    .collect();
                let exists = destination.is_dir();
                let available = exists
                    .then(|| fs2::available_space(&destination).ok())
                    .flatten();
                (sizes, available, exists)
            })
            .await
            .map_err(|error| BackupError::Io(std::io::Error::other(error)))?;
        if let Some(fake) = self.faults().available {
            available = Some(fake);
        }
        let mut components = vec![PlannedComponent {
            id: Component::Library,
            selected: true,
            files: 1,
            bytes: library,
        }];
        components.extend(
            sizes
                .into_iter()
                .map(|(id, files, bytes)| PlannedComponent {
                    id,
                    selected: options.components.contains(&id),
                    files,
                    bytes,
                }),
        );
        Ok(BackupPlan {
            destination: options.destination.clone(),
            estimated_bytes: components
                .iter()
                .filter(|c| c.selected)
                .map(|c| c.bytes)
                .sum(),
            components,
            excluded: Exclusion::ALL.to_vec(),
            available_bytes: available,
            destination_exists: exists,
        })
    }

    /// Makes a backup in `options.destination`: published under its final
    /// name only once it has been read back and checked. On failure or
    /// cancellation nothing is left but the backups that were there.
    pub async fn export(
        &self,
        options: ExportOptions,
        progress: ProgressFn,
        cancel: CancellationToken,
    ) -> Result<Exported, BackupError> {
        let _running = self.running.try_lock().map_err(|_| BackupError::Busy)?;
        let _activity = caskade_jobs::global().activity("Backing up");
        let destination = options.destination.clone();
        if destination == self.default_destination() {
            tokio::fs::create_dir_all(&destination).await?;
        }
        if !tokio::fs::metadata(&destination)
            .await
            .is_ok_and(|m| m.is_dir())
        {
            return Err(BackupError::DestinationMissing(destination));
        }
        sweep(&destination).await;
        let plan = self.plan(&options).await?;
        // The snapshot, then the archive holding a copy of it.
        let needed = plan.estimated_bytes + plan.components[0].bytes + MARGIN;
        if let Some(available) = plan.available_bytes
            && available < needed
        {
            return Err(BackupError::NoSpace { needed, available });
        }
        let id = crate::model::new_id();
        let database = destination.join(format!("{PARTIAL}{id}.partial.sqlite"));
        let partial = destination.join(format!("{PARTIAL}{id}.partial"));
        let result = self
            .export_staged(&options, &plan, &database, &partial, progress, cancel)
            .await;
        if !self.faults().crash {
            let _ = tokio::fs::remove_file(&database).await;
            if result.is_err() {
                let _ = tokio::fs::remove_file(&partial).await;
            }
        }
        result
    }

    async fn export_staged(
        &self,
        options: &ExportOptions,
        plan: &super::export::BackupPlan,
        database: &Path,
        partial: &Path,
        progress: ProgressFn,
        cancel: CancellationToken,
    ) -> Result<Exported, BackupError> {
        progress(Progress {
            phase: Phase::Snapshot,
            done_bytes: 0,
            total_bytes: plan.estimated_bytes,
        });
        let snapshot = snapshot::take(&self.store, database).await?;
        if cancel.is_cancelled() {
            return Err(BackupError::Cancelled);
        }
        let mut sources = Vec::new();
        let selected: Vec<Component> = std::iter::once(Component::Library)
            .chain(options.components.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for component in &selected {
            sources.extend(files(&self.paths, *component, &snapshot.artwork, database));
        }
        let manifest = Manifest {
            format: super::manifest::FORMAT,
            app_version: crate::VERSION.to_owned(),
            schema_version: snapshot.schema_version,
            created_at_ms: crate::now_ms(),
            os: std::env::consts::OS.to_owned(),
            set_id: options.set_id.clone(),
            components: selected
                .iter()
                .map(|id| ComponentSummary {
                    id: *id,
                    files: 0,
                    bytes: 0,
                })
                .collect(),
            tables: snapshot.tables,
            excluded: Exclusion::ALL.to_vec(),
            files: Vec::new(),
        };
        let total: u64 = sources
            .iter()
            .filter_map(|s| std::fs::metadata(&s.file).ok())
            .map(|m| m.len())
            .sum();
        let budget = self.faults().disk_full_after;
        let (manifest, bytes) = caskade_jobs::blocking("Backing up", {
            let (partial, cancel, progress) =
                (partial.to_owned(), cancel.clone(), progress.clone());
            move || -> Result<(Manifest, u64), BackupError> {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&partial)?;
                let out = Budget {
                    inner: std::io::BufWriter::new(file),
                    left: budget,
                };
                let report = |done| {
                    progress(Progress {
                        phase: Phase::Writing,
                        done_bytes: done,
                        total_bytes: total,
                    })
                };
                let (out, manifest) = archive::write(out, &sources, manifest, &cancel, &report)?;
                let file = out.inner.into_inner().map_err(|e| e.into_error())?;
                file.sync_all()?;
                let bytes = file.metadata()?.len();
                Ok((manifest, bytes))
            }
        })
        .await
        .map_err(|error| BackupError::Io(std::io::Error::other(error)))??;
        let verified = caskade_jobs::blocking("Checking the backup", {
            let (partial, cancel, progress) =
                (partial.to_owned(), cancel.clone(), progress.clone());
            let total = manifest.files.iter().map(|f| f.size).sum();
            move || {
                archive::verify(&partial, &cancel, &|done| {
                    progress(Progress {
                        phase: Phase::Verifying,
                        done_bytes: done,
                        total_bytes: total,
                    })
                })
            }
        })
        .await
        .map_err(|error| BackupError::Io(std::io::Error::other(error)))??;
        if verified != manifest {
            return Err(BackupError::Damaged(crate::tr!(
                "the backup read back differently from how it was written"
            )));
        }
        if cancel.is_cancelled() {
            return Err(BackupError::Cancelled);
        }
        if self.faults().crash {
            return Err(BackupError::Io(std::io::Error::other(
                "crash before publishing",
            )));
        }
        let path = publish(partial, &options.destination, manifest.created_at_ms).await?;
        tracing::info!(bytes, files = manifest.files.len(), "backup made");
        Ok(Exported {
            path,
            bytes,
            manifest,
        })
    }
}

/// The files of `component`. The library's is the snapshot at `database`.
fn files(
    paths: &crate::AppPaths,
    component: Component,
    artwork: &[(String, bool)],
    database: &Path,
) -> Vec<Source> {
    let mut out = Vec::new();
    match component {
        Component::Library => out.push(Source {
            path: super::manifest::DATABASE.to_owned(),
            component,
            file: database.to_owned(),
        }),
        Component::Artwork | Component::DownloadedArtwork => {
            let downloaded = component == Component::DownloadedArtwork;
            let root = paths.cache.join("artwork");
            for (hash, igdb) in artwork {
                if *igdb != downloaded || !content_hash(hash) {
                    continue;
                }
                let Ok(entries) = std::fs::read_dir(root.join(hash)) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    // `.lock` and half-written files stay behind.
                    if name.starts_with('.') || !entry.file_type().is_ok_and(|t| t.is_file()) {
                        continue;
                    }
                    out.push(Source {
                        path: format!("artwork/{hash}/{name}"),
                        component,
                        file: entry.path(),
                    });
                }
            }
        }
        Component::Plugins => walk(&paths.data.join("plugins"), component, &mut out),
        Component::PluginData => walk(&paths.data.join("plugin-data"), component, &mut out),
        Component::Themes => walk(&paths.data.join("themes"), component, &mut out),
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// An artwork cache folder's name: a lowercase SHA-256.
fn content_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Every regular file under `root`, symlinks not followed.
fn walk(root: &Path, component: Component, out: &mut Vec<Source>) {
    let mut stack = vec![(root.to_owned(), component.folder().to_owned())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                tracing::warn!(path = %entry.path().display(), "left out of the backup: its name isn't UTF-8");
                continue;
            };
            let path = format!("{prefix}/{name}");
            if archive::safe_path(&path, component).is_err() {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push((entry.path(), path)),
                Ok(kind) if kind.is_file() => out.push(Source {
                    path,
                    component,
                    file: entry.path(),
                }),
                _ => {}
            }
        }
    }
}

/// Removes partial files a crashed or killed backup left in `destination`.
/// One backup runs at a time, so none is in use.
async fn sweep(destination: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(destination).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(PARTIAL) && name.contains(".partial") {
            tracing::info!(name, "removing a backup that didn't finish");
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// Renames `partial` to `caskade-backup-<UTC time>.zip`, never over an
/// existing file.
async fn publish(partial: &Path, destination: &Path, at_ms: i64) -> Result<PathBuf, BackupError> {
    let stamp = stamp(at_ms);
    for attempt in 1..1000 {
        let name = if attempt == 1 {
            format!("{PREFIX}{stamp}.{EXTENSION}")
        } else {
            format!("{PREFIX}{stamp}-{attempt}.{EXTENSION}")
        };
        let path = destination.join(name);
        if tokio::fs::symlink_metadata(&path).await.is_ok() {
            continue;
        }
        tokio::fs::rename(partial, &path).await?;
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(destination) {
            let _ = dir.sync_all();
        }
        return Ok(path);
    }
    Err(BackupError::Io(std::io::Error::from(
        std::io::ErrorKind::AlreadyExists,
    )))
}

/// `YYYYMMDD-HHMMSS` in UTC.
pub(super) fn stamp(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = crate::civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A writer that fails like a full disk after `left` bytes (tests).
struct Budget<W> {
    inner: W,
    left: Option<u64>,
}

impl<W: std::io::Write> std::io::Write for Budget<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(left) = &mut self.left {
            if *left < buf.len() as u64 {
                return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
            }
            *left -= buf.len() as u64;
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<W: std::io::Seek> std::io::Seek for Budget<W> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}
