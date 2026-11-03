//! Restoring a backup (#192): inspect, stage, then apply at the next start.
//!
//! [`inspect`] checks an archive without changing anything. [`stage`]
//! extracts it into `<data>/restore/staged`, migrates the database there
//! and writes the journal; the running profile is untouched. The app then
//! restarts, and [`apply_pending`] swaps the staged files in before
//! anything opens the library, moving the replaced ones to
//! `<data>/restore/rollback`. Once the library opens, [`finish`] deletes the
//! rollback; if it doesn't, [`roll_back`] puts the old profile back. Every
//! step is a rename recorded in the journal, so a start after a crash
//! carries on where it stopped.

use super::{
    BackupError, Phase, Progress, archive,
    manifest::{self, Component, ComponentSummary, Manifest},
    snapshot,
};
use crate::{AppPaths, db::LibraryStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

const JOURNAL: &str = "journal.json";
/// What the last restore did, for the app to show after the restart.
const LAST: &str = "last.json";

/// What restoring an archive would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupInspection")]
pub struct Inspection {
    /// The backup's manifest without its file list.
    pub manifest: Manifest,
    /// The components it holds, with their files and sizes.
    pub components: Vec<ComponentSummary>,
    /// This Caskade's newest schema; the backup's is older when it needs
    /// migrating.
    pub current_schema: i64,
    pub needs_migration: bool,
    /// It was made on another OS: its paths won't be found here.
    pub other_os: bool,
    /// Storage locations it knows whose folders aren't here.
    pub missing_roots: Vec<PathBuf>,
    /// Games found elsewhere (not in a storage location) whose folders
    /// aren't here.
    pub missing_games: u32,
}

/// What to restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOptions {
    pub archive: PathBuf,
    /// The optional components to restore, of those the backup holds; the
    /// library always is. The others stay as they are.
    pub components: BTreeSet<Component>,
}

/// Where a restore is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RestoreState {
    /// Waiting for the restart that applies it.
    Staged,
    /// Swapping files; a start that finds it here carries on.
    Applying,
    /// Swapped; the old profile is kept until the library opens.
    Applied,
    /// Putting the old profile back; a start that finds it here carries on.
    RollingBack,
}

/// The journal, `<data>/restore/journal.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "RestoreJournal")]
pub struct Journal {
    pub state: RestoreState,
    /// The archive's file name.
    pub archive: String,
    /// When the backup was made (Unix ms).
    pub created_at_ms: i64,
    pub app_version: String,
    /// The components being restored, the library included.
    pub components: BTreeSet<Component>,
    pub inspection: Inspection,
}

/// How the last restore ended, kept for the app to say so once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub journal: Journal,
    /// `None` when it succeeded; otherwise why it was undone.
    pub error: Option<String>,
    pub finished_at_ms: i64,
}

/// `<data>/restore`.
pub fn folder(paths: &AppPaths) -> PathBuf {
    paths.data.join("restore")
}

/// Checks `archive` against its manifest and this Caskade, and reports what
/// restoring it would do. Changes nothing but a temporary copy of its
/// database in `<data>/restore`.
pub async fn inspect(
    paths: &AppPaths,
    archive_path: &Path,
    cancel: &CancellationToken,
    progress: &super::ProgressFn,
) -> Result<Inspection, BackupError> {
    Ok(examine(paths, archive_path, cancel, progress).await?.1)
}

/// [`inspect`], and the verified manifest.
async fn examine(
    paths: &AppPaths,
    archive_path: &Path,
    cancel: &CancellationToken,
    progress: &super::ProgressFn,
) -> Result<(Manifest, Inspection), BackupError> {
    let manifest = verified(archive_path, cancel, progress).await?;
    let dir = folder(paths);
    tokio::fs::create_dir_all(&dir).await?;
    let file = dir.join(format!("inspect-{}.sqlite", crate::model::new_id()));
    let result = async {
        let archive_path = archive_path.to_owned();
        let (target, manifest_copy) = (file.clone(), manifest.clone());
        caskade_jobs::blocking("Checking a backup", move || {
            extract_one(&archive_path, &manifest_copy, manifest::DATABASE, &target)
        })
        .await
        .map_err(join)??;
        let mut connection = snapshot::open(&file).await?;
        let result = async {
            snapshot::check(&mut connection).await?;
            check_migrations(&mut connection).await?;
            survey(&mut connection, &manifest).await
        }
        .await;
        let _ = sqlx::Connection::close(connection).await;
        result
    }
    .await;
    let _ = tokio::fs::remove_file(&file).await;
    Ok((manifest, result?))
}

/// Verifies the archive on the resource queue.
async fn verified(
    archive_path: &Path,
    cancel: &CancellationToken,
    progress: &super::ProgressFn,
) -> Result<Manifest, BackupError> {
    let (path, cancel, progress) = (archive_path.to_owned(), cancel.clone(), progress.clone());
    let total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let manifest = caskade_jobs::blocking("Checking a backup", move || {
        archive::verify(&path, &cancel, &|done| {
            progress(Progress {
                phase: Phase::Verifying,
                done_bytes: done,
                total_bytes: total,
            })
        })
    })
    .await
    .map_err(join)??;
    let newest = newest_schema();
    if manifest.schema_version > newest {
        return Err(BackupError::Newer);
    }
    Ok(manifest)
}

fn newest_schema() -> i64 {
    crate::db::MIGRATOR
        .iter()
        .map(|m| m.version)
        .max()
        .unwrap_or(0)
}

/// Every migration the backup's database has applied must be one this
/// Caskade has, with the same checksum: otherwise a newer Caskade (or
/// another build) wrote it.
async fn check_migrations(connection: &mut sqlx::SqliteConnection) -> Result<(), BackupError> {
    let applied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations WHERE success")
            .fetch_all(&mut *connection)
            .await
            .map_err(|_| {
                BackupError::Damaged(crate::tr!("its database has no migration record"))
            })?;
    for (version, checksum) in applied {
        match crate::db::MIGRATOR.iter().find(|m| m.version == version) {
            Some(known) if *known.checksum == *checksum => {}
            Some(_) => {
                return Err(BackupError::Damaged(crate::tr!(
                    "its database schema {version} differs from this Caskade's",
                    version = version
                )));
            }
            None => return Err(BackupError::Newer),
        }
    }
    Ok(())
}

/// What a restore would find missing here.
async fn survey(
    connection: &mut sqlx::SqliteConnection,
    manifest: &Manifest,
) -> Result<Inspection, BackupError> {
    let roots: Vec<String> = sqlx::query_scalar("SELECT path FROM storage_roots ORDER BY path")
        .fetch_all(&mut *connection)
        .await
        .map_err(crate::Error::from)?;
    let external: Vec<String> =
        sqlx::query_scalar("SELECT external_path FROM game_entries WHERE location_kind='external'")
            .fetch_all(&mut *connection)
            .await
            .map_err(crate::Error::from)?;
    let (missing_roots, missing_games) = tokio::task::spawn_blocking(move || {
        let missing_roots: Vec<PathBuf> = roots
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| !path.exists())
            .collect();
        let missing_games = external
            .iter()
            .filter(|path| !Path::new(path).exists())
            .count();
        (
            missing_roots,
            u32::try_from(missing_games).unwrap_or(u32::MAX),
        )
    })
    .await
    .map_err(join)?;
    let newest = newest_schema();
    let mut summary = manifest.clone();
    summary.files.clear();
    Ok(Inspection {
        components: manifest.components.clone(),
        current_schema: newest,
        needs_migration: manifest.schema_version < newest,
        other_os: manifest.os != std::env::consts::OS,
        missing_roots,
        missing_games,
        manifest: summary,
    })
}

/// Extracts the backup into `<data>/restore/staged`, migrates its database
/// and writes the journal, so the next start applies it. The running
/// profile isn't changed, but for artwork, which is added to the cache
/// (content-addressed, so nothing there is replaced). A restore already
/// staged is replaced.
pub async fn stage(
    paths: &AppPaths,
    options: &RestoreOptions,
    cancel: &CancellationToken,
    progress: &super::ProgressFn,
) -> Result<Journal, BackupError> {
    if let Some(journal) = pending(paths)?
        && journal.state != RestoreState::Staged
    {
        return Err(BackupError::Busy);
    }
    let dir = folder(paths);
    discard_staged(paths).await?;
    let (manifest, inspection) = examine(paths, &options.archive, cancel, progress).await?;
    let held: BTreeSet<Component> = manifest.components.iter().map(|c| c.id).collect();
    let components: BTreeSet<Component> = std::iter::once(Component::Library)
        .chain(
            options
                .components
                .iter()
                .copied()
                .filter(|c| held.contains(c)),
        )
        .collect();
    let needed: u64 = manifest
        .files
        .iter()
        .filter(|f| components.contains(&f.component))
        .map(|f| f.size)
        .sum();
    tokio::fs::create_dir_all(&dir).await?;
    if let Ok(available) = fs2::available_space(&dir)
        && available < needed + 16 * 1024 * 1024
    {
        return Err(BackupError::NoSpace { needed, available });
    }
    let staged = dir.join("staged");
    let result = async {
        caskade_jobs::blocking("Restoring a backup", {
            let (archive_path, staged, artwork) = (
                options.archive.clone(),
                staged.clone(),
                paths.cache.join("artwork"),
            );
            let (manifest, components, cancel, progress) = (
                manifest.clone(),
                components.clone(),
                cancel.clone(),
                progress.clone(),
            );
            move || {
                extract(
                    &archive_path,
                    &manifest,
                    &components,
                    &staged,
                    &artwork,
                    &cancel,
                    &|done| {
                        progress(Progress {
                            phase: Phase::Extracting,
                            done_bytes: done,
                            total_bytes: needed,
                        })
                    },
                )
            }
        })
        .await
        .map_err(join)??;
        progress(Progress {
            phase: Phase::Migrating,
            done_bytes: 0,
            total_bytes: 0,
        });
        prepare(&staged.join("library.sqlite"), &components).await?;
        let journal = Journal {
            state: RestoreState::Staged,
            archive: options
                .archive
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            created_at_ms: manifest.created_at_ms,
            app_version: manifest.app_version.clone(),
            components,
            inspection,
        };
        write_journal(&dir, &journal)?;
        Ok(journal)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&staged).await;
    }
    result
}

/// Forgets a staged restore that hasn't been applied.
pub async fn discard_staged(paths: &AppPaths) -> Result<(), BackupError> {
    let dir = folder(paths);
    match pending(paths)? {
        Some(journal) if journal.state != RestoreState::Staged => return Err(BackupError::Busy),
        Some(_) => fs::remove_file(dir.join(JOURNAL))?,
        None => {}
    }
    match tokio::fs::remove_dir_all(dir.join("staged")).await {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// The restore under way, if any.
pub fn pending(paths: &AppPaths) -> Result<Option<Journal>, BackupError> {
    match fs::read(folder(paths).join(JOURNAL)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).map_err(|_| {
            BackupError::Damaged(crate::tr!("the restore journal can't be read"))
        })?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// How the last restore ended, once: reading it forgets it.
pub fn take_outcome(paths: &AppPaths) -> Option<RestoreOutcome> {
    let file = folder(paths).join(LAST);
    let outcome = serde_json::from_slice(&fs::read(&file).ok()?).ok();
    let _ = fs::remove_file(file);
    outcome
}

/// Extracts the chosen components: the database and plugin folders into
/// `staged`, artwork straight into the cache. Each file is hashed again on
/// the way, so an archive changed since it was verified is refused.
fn extract(
    archive_path: &Path,
    manifest: &Manifest,
    components: &BTreeSet<Component>,
    staged: &Path,
    artwork: &Path,
    cancel: &CancellationToken,
    report: archive::Report<'_>,
) -> Result<(), BackupError> {
    fs::create_dir_all(staged)?;
    for component in components {
        if let Some(name) = swapped_folder(*component) {
            fs::create_dir_all(staged.join(name))?;
        }
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(archive_path)?)
        .map_err(|_| BackupError::Damaged(crate::tr!("the backup archive is damaged")))?;
    let mut done = 0u64;
    for file in manifest
        .files
        .iter()
        .filter(|f| components.contains(&f.component))
    {
        archive::safe_path(&file.path, file.component)?;
        let target = match file.component {
            Component::Library if file.path == manifest::DATABASE => staged.join("library.sqlite"),
            Component::Library => continue,
            Component::Artwork | Component::DownloadedArtwork => {
                let mut parts = file.path.split('/').skip(1);
                let (Some(hash), Some(name), None) = (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let dir = artwork.join(hash);
                // Content-addressed: an entry already there is the same.
                if dir.join(name).exists() {
                    continue;
                }
                fs::create_dir_all(&dir)?;
                dir.join(name)
            }
            _ => staged.join(&file.path),
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut entry = zip.by_name(&file.path).map_err(|_| {
            BackupError::Damaged(crate::tr!("“{path}” is missing", path = file.path))
        })?;
        let partial = target.with_extension("caskade-partial");
        let mut out = fs::File::create(&partial)?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        let mut limited = (&mut entry).take(file.size.saturating_add(1));
        loop {
            if cancel.is_cancelled() {
                let _ = fs::remove_file(&partial);
                return Err(BackupError::Cancelled);
            }
            let read = limited.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
            out.write_all(&buffer[..read])?;
            size += read as u64;
            done += read as u64;
            report(done);
        }
        out.sync_all()?;
        drop(out);
        if size != file.size || format!("{:x}", hash.finalize()) != file.sha256 {
            let _ = fs::remove_file(&partial);
            return Err(BackupError::Damaged(crate::tr!(
                "“{path}” doesn't match its checksum",
                path = file.path
            )));
        }
        fs::rename(&partial, &target)?;
    }
    Ok(())
}

/// Copies one file of a verified archive to `target`.
fn extract_one(
    archive_path: &Path,
    manifest: &Manifest,
    path: &str,
    target: &Path,
) -> Result<(), BackupError> {
    let entry = manifest
        .files
        .iter()
        .find(|f| f.path == path)
        .ok_or_else(|| BackupError::Damaged(crate::tr!("“{path}” is missing", path = path)))?;
    let mut zip = zip::ZipArchive::new(fs::File::open(archive_path)?)
        .map_err(|_| BackupError::Damaged(crate::tr!("the backup archive is damaged")))?;
    let mut reader = zip
        .by_name(path)
        .map_err(|_| BackupError::Damaged(crate::tr!("“{path}” is missing", path = path)))?;
    let mut out = fs::File::create(target)?;
    std::io::copy(&mut (&mut reader).take(entry.size), &mut out)?;
    Ok(())
}

/// The staged database: migrated to this Caskade's schema, its plugins'
/// grants dropped unless their files come back too, out of WAL and checked.
async fn prepare(file: &Path, components: &BTreeSet<Component>) -> Result<(), BackupError> {
    // The library it will be: opened as patiently as the library is.
    let store = LibraryStore::open_with(file, crate::db::Opening::LIBRARY).await?;
    store.close().await;
    let mut connection = snapshot::open(file).await?;
    let result = async {
        if !components.contains(&Component::Plugins) {
            // A plugin here may not be the build that was granted: it
            // asks again, as a newly installed one would.
            sqlx::query("UPDATE plugins SET enabled=0, grants_json='null'")
                .execute(&mut connection)
                .await
                .map_err(crate::Error::from)?;
        }
        snapshot::check(&mut connection).await
    }
    .await;
    let _ = sqlx::Connection::close(connection).await;
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(sidecar(file, suffix));
    }
    result
}

/// The folders under `<data>` a component replaces whole.
fn swapped_folder(component: Component) -> Option<&'static str> {
    match component {
        Component::Plugins => Some("plugins"),
        Component::PluginData => Some("plugin-data"),
        Component::Themes => Some("themes"),
        _ => None,
    }
}

/// One path the swap moves: `staged` (relative to the staged folder) to
/// `target`, the target's old content to `rollback` (relative to the
/// rollback folder). Without `staged`, the target only moves aside.
struct Item {
    staged: Option<&'static str>,
    target: PathBuf,
    rollback: &'static str,
}

fn items(paths: &AppPaths, components: &BTreeSet<Component>) -> Vec<Item> {
    let database = paths.database();
    let mut items = vec![
        Item {
            staged: Some("library.sqlite"),
            target: database.clone(),
            rollback: "library.sqlite",
        },
        Item {
            staged: None,
            target: sidecar(&database, "-wal"),
            rollback: "library.sqlite-wal",
        },
        Item {
            staged: None,
            target: sidecar(&database, "-shm"),
            rollback: "library.sqlite-shm",
        },
        // The storage locations' mirror would add this profile's
        // locations back into the restored library at start.
        Item {
            staged: None,
            target: paths.config.join("storage-roots.json"),
            rollback: "storage-roots.json",
        },
    ];
    for component in components {
        if let Some(name) = swapped_folder(*component) {
            items.push(Item {
                staged: Some(name),
                target: paths.data.join(name),
                rollback: name,
            });
        }
    }
    items
}

fn sidecar(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Run at start, before anything opens the library. Applies a staged
/// restore, carries on with an interrupted one, or finishes an interrupted
/// rollback. Returns the journal of an applied restore: the caller opens
/// the library, then calls [`finish`] or, if it can't, [`roll_back`].
pub fn apply_pending(paths: &AppPaths) -> Result<Option<Journal>, BackupError> {
    let Some(mut journal) = pending(paths)? else {
        return Ok(None);
    };
    let dir = folder(paths);
    match journal.state {
        RestoreState::RollingBack => {
            roll_back(paths, &crate::tr!("the restore was interrupted"))?;
            return Ok(None);
        }
        RestoreState::Applied => return Ok(Some(journal)),
        RestoreState::Staged | RestoreState::Applying => {}
    }
    journal.state = RestoreState::Applying;
    write_journal(&dir, &journal)?;
    let (staged, rollback) = (dir.join("staged"), dir.join("rollback"));
    fs::create_dir_all(&rollback)?;
    for item in items(paths, &journal.components) {
        let aside = rollback.join(item.rollback);
        let source = item.staged.map(|name| staged.join(name));
        // Already swapped: the staged copy is gone, or nothing was to come.
        let pending_source = source.as_ref().is_some_and(|s| exists(s));
        if source.is_some() && !pending_source {
            continue;
        }
        if exists(&item.target) {
            if exists(&aside) {
                // Something appeared after it was moved aside: not ours.
                remove(&item.target)?;
            } else {
                move_path(&item.target, &aside)?;
            }
        } else if source.is_some() && !exists(&aside) {
            // A rollback removes what comes, and puts nothing back.
            fs::write(absent(&aside), b"")?;
        }
        if let Some(source) = source {
            if let Some(parent) = item.target.parent() {
                fs::create_dir_all(parent)?;
            }
            move_path(&source, &item.target)?;
        }
    }
    let _ = fs::remove_dir_all(&staged);
    journal.state = RestoreState::Applied;
    write_journal(&dir, &journal)?;
    tracing::info!(archive = %journal.archive, "backup restored; the old profile is kept until the library opens");
    Ok(Some(journal))
}

/// The restored library opened: the old profile goes.
pub fn finish(paths: &AppPaths) -> Result<(), BackupError> {
    let Some(journal) = pending(paths)? else {
        return Ok(());
    };
    let dir = folder(paths);
    write_outcome(&dir, &journal, None);
    fs::remove_file(dir.join(JOURNAL))?;
    let _ = fs::remove_dir_all(dir.join("rollback"));
    Ok(())
}

/// Puts the profile from before the restore back: the restored library
/// didn't open, or the restore was interrupted while rolling back.
pub fn roll_back(paths: &AppPaths, why: &str) -> Result<(), BackupError> {
    let Some(mut journal) = pending(paths)? else {
        return Ok(());
    };
    let dir = folder(paths);
    if journal.state == RestoreState::Staged {
        // Nothing was applied.
        return Ok(());
    }
    journal.state = RestoreState::RollingBack;
    write_journal(&dir, &journal)?;
    let rollback = dir.join("rollback");
    for item in items(paths, &journal.components) {
        let aside = rollback.join(item.rollback);
        let marker = absent(&aside);
        if exists(&aside) {
            if exists(&item.target) {
                remove(&item.target)?;
            }
            move_path(&aside, &item.target)?;
        } else if exists(&marker) {
            // There was none before: the restored one goes.
            if exists(&item.target) {
                remove(&item.target)?;
            }
            fs::remove_file(&marker)?;
        }
    }
    write_outcome(&dir, &journal, Some(why.to_owned()));
    fs::remove_file(dir.join(JOURNAL))?;
    let _ = fs::remove_dir_all(&rollback);
    let _ = fs::remove_dir_all(dir.join("staged"));
    tracing::warn!(why, "the restore was undone");
    Ok(())
}

fn write_outcome(dir: &Path, journal: &Journal, error: Option<String>) {
    let outcome = RestoreOutcome {
        journal: journal.clone(),
        error,
        finished_at_ms: crate::now_ms(),
    };
    if let Err(error) = serde_json::to_vec(&outcome)
        .map_err(std::io::Error::other)
        .and_then(|bytes| atomic_write(dir, LAST, &bytes))
    {
        tracing::warn!(%error, "the restore's outcome wasn't kept");
    }
}

fn write_journal(dir: &Path, journal: &Journal) -> Result<(), BackupError> {
    atomic_write(dir, JOURNAL, &serde_json::to_vec_pretty(journal)?)?;
    Ok(())
}

fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(".{name}.{}.tmp", crate::model::new_id()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, dir.join(name))?;
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Beside a rollback path: the target wasn't there before the restore.
fn absent(aside: &Path) -> PathBuf {
    sidecar(aside, ".absent")
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn remove(path: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// A rename, or a copy and delete across drives (files only: the folders
/// moved all live under `<data>`).
fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if fs::symlink_metadata(from).is_ok_and(|m| m.is_file()) => {
            tracing::debug!(%error, "moving by copying");
            let partial = sidecar(to, ".caskade-partial");
            fs::copy(from, &partial)?;
            fs::File::open(&partial)?.sync_all()?;
            fs::rename(&partial, to)?;
            fs::remove_file(from)
        }
        Err(error) => Err(error),
    }
}

fn join(error: tokio::task::JoinError) -> BackupError {
    BackupError::Io(std::io::Error::other(error))
}
