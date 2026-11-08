//! Relocate a game to another root, the move job:
//! move the files, retarget the alias and absolute prefix links, and rewrite
//! every real-path reference the scanner recorded, as one transaction. Every
//! change is undone, newest first, on any failure or cancellation.
pub mod backends;
mod effects;
mod files;
mod journal;
mod spelling;
pub use effects::wow64_32bit_key;
use files::*;
use journal::UndoLog;
pub use spelling::{plain_windows, replace_all, spelling_pairs};

use crate::{
    disk::DiskSpace,
    events::{EventSink, JobEvent, Phase},
    manifest::InstallManifest,
    runtime::alias::AliasStore,
    scan::{Location, Reference, Via},
};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

/// One registry value write. `hive` is `HKCU`/`HKLM`; data is raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegistryWrite {
    pub hive: String,
    pub key: String,
    pub name: String,
    pub value_type: String,
    pub data: Vec<u8>,
}

/// Host (or prefix) registry access. `apply` receives all writes of a move
/// at once so a Windows implementation can batch HKLM into one elevated
/// `reg import` (one OS prompt); it is called again with the originals on
/// rollback.
pub trait RegistryStore: Send + Sync {
    fn read(&self, hive: &str, key: &str, name: &str) -> io::Result<Option<RegistryWrite>>;
    fn apply(&self, writes: &[RegistryWrite]) -> io::Result<()>;
}

/// `.lnk` shortcuts are binary; a platform editor rewrites their target,
/// working directory and arguments (`IShellLinkW` on Windows). Undo restores
/// the original file bytes.
pub trait ShortcutEditor: Send + Sync {
    fn rewrite(&self, path: &Path, old: &str, new: &str) -> io::Result<()>;
}

/// Is the game running? Injected; moving a running game is refused.
pub trait RunningCheck: Send + Sync {
    fn running(&self, game_dir: &Path) -> bool;
}

pub struct RelocateRequest {
    pub job_id: String,
    /// Current game directory (real path).
    pub game_dir: PathBuf,
    /// Destination storage root; the game keeps its directory name.
    pub target_root: PathBuf,
    /// References from the manifest's relocation record (#32).
    pub references: Vec<Reference>,
    /// Extra (old, new) spellings, e.g. `C:\Games\X` → `D:\Games\X` for
    /// paths installers saw through a Windows drive.
    pub spellings: Vec<(String, String)>,
    pub alias: Option<(AliasStore, String)>,
    /// Force copy + verify + delete even on one volume (tests, policy).
    pub force_copy: bool,
}

/// Rewrite references after the game already moved (reconcile, #61): no
/// files are moved; every rewrite is journalled and rolled back on failure.
pub struct RepairRequest {
    pub job_id: String,
    /// Where references point now, e.g. the manifest's `relocation.last_path`.
    pub old_dir: String,
    /// The game's current directory.
    pub game_dir: PathBuf,
    pub references: Vec<Reference>,
    pub spellings: Vec<(String, String)>,
    pub alias: Option<(AliasStore, String)>,
    /// Optional manifest changes applied in the same transaction.
    pub root_id: Option<String>,
    pub relative_dir: Option<PathBuf>,
    /// Verdict to record with the repair (reconcile's conservative rescan).
    pub verdict: Option<crate::manifest::RelocationVerdict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelocateReport {
    pub game_dir: PathBuf,
    pub copied: bool,
    pub rewritten_files: usize,
    pub rewritten_values: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum RelocateError {
    #[error("the game is running")]
    Running,
    #[error("target is not usable: {0}")]
    Target(String),
    #[error("not enough free space: need {needed} bytes, {available} available")]
    Space { needed: u64, available: u64 },
    #[error("copied file differs from its source: {0}")]
    Verify(PathBuf),
    #[error("move cancelled; every change was rolled back")]
    Cancelled,
    #[error("move failed and was rolled back: {0}")]
    RolledBack(String),
    #[error("move failed ({error}) and rollback was incomplete: {rollback}")]
    RollbackFailed { error: String, rollback: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct Relocator {
    pub disk: Arc<dyn DiskSpace>,
    pub sink: Arc<dyn EventSink>,
    pub registry: Arc<dyn RegistryStore>,
    pub shortcuts: Arc<dyn ShortcutEditor>,
    pub running: Arc<dyn RunningCheck>,
}

/// Undo log entry, replayed newest first. Also the move journal's format.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum Undo {
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    Copied {
        to: PathBuf,
    },
    File {
        path: PathBuf,
        original: Vec<u8>,
    },
    Registry(Vec<RegistryWrite>),
    Alias {
        #[serde(rename = "base")]
        store: AliasStore,
        id: String,
        target: PathBuf,
    },
    AliasCreated {
        #[serde(rename = "base")]
        store: AliasStore,
        id: String,
    },
    Link {
        path: PathBuf,
        target: PathBuf,
    },
}

impl Relocator {
    pub fn relocate(
        &self,
        request: &RelocateRequest,
        cancel: &CancellationToken,
    ) -> Result<RelocateReport, RelocateError> {
        self.relocate_with_verdict(request, cancel, None)
    }

    /// [`Self::relocate`], also recording `verdict` (e.g. a conservative
    /// reclassification made while preparing the move) in the same manifest
    /// transaction; `None` keeps the recorded verdict.
    pub fn relocate_with_verdict(
        &self,
        request: &RelocateRequest,
        cancel: &CancellationToken,
        verdict: Option<crate::manifest::RelocationVerdict>,
    ) -> Result<RelocateReport, RelocateError> {
        let name = request
            .game_dir
            .file_name()
            .ok_or_else(|| RelocateError::Target("game directory has no name".into()))?;
        let source = dunce::canonicalize(&request.game_dir)?;
        if self.running.running(&source) {
            return Err(RelocateError::Running);
        }
        let target_root = dunce::canonicalize(&request.target_root).map_err(|error| {
            RelocateError::Target(format!("{}: {error}", request.target_root.display()))
        })?;
        let destination = target_root.join(name);
        if destination.starts_with(&source) || source.starts_with(&destination) {
            return Err(RelocateError::Target("source and target overlap".into()));
        }
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(RelocateError::Target(format!(
                "{} already exists",
                destination.display()
            )));
        }
        let probe = target_root.join(format!(".caskade-move-{}", request.job_id));
        fs::create_dir(&probe)
            .and_then(|()| fs::remove_dir(&probe))
            .map_err(|error| RelocateError::Target(format!("not writable: {error}")))?;
        let copy = request.force_copy || !same_volume(&source, &target_root)?;
        let total = tree_size(&source)?;
        if copy {
            let available = self.disk.available_bytes(&target_root)?;
            if available < total {
                return Err(RelocateError::Space {
                    needed: total,
                    available,
                });
            }
        }
        let mut undo = UndoLog::create(request, &source, &destination, copy)?;
        let result = self.apply(
            request,
            &source,
            &destination,
            copy,
            total,
            verdict,
            cancel,
            &mut undo,
        );
        match result {
            Ok(report) => {
                // Commit is durable before deleting the source copy. Startup
                // recovery can finish cleanup after a kill at either point.
                undo.commit()?;
                self.emit(&request.job_id, "committed", total, total);
                if copy {
                    remove_tree(&source)?;
                    if let Some(parent) = source.parent() {
                        sync_directory(parent)?;
                    }
                }
                undo.finish()?;
                self.emit(&request.job_id, "completed", total, total);
                Ok(report)
            }
            Err(error) => Err(self.roll_back(error, cancel, &mut undo)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply(
        &self,
        request: &RelocateRequest,
        source: &Path,
        destination: &Path,
        copy: bool,
        total: u64,
        verdict: Option<crate::manifest::RelocationVerdict>,
        cancel: &CancellationToken,
        undo: &mut UndoLog,
    ) -> Result<RelocateReport, RelocateError> {
        let check = || check_cancelled(cancel);
        check()?;
        self.emit(&request.job_id, "moving", 0, total);
        if copy {
            let staging = undo
                .staging()
                .expect("durable move has staging")
                .to_path_buf();
            undo.record(Undo::Copied {
                to: staging.clone(),
            })?;
            copy_tree(source, &staging, cancel, &|copied| {
                self.emit(&request.job_id, "copying", copied, total)
            })?;
            verify_tree(source, &staging)?;
            undo.record(Undo::Copied {
                to: destination.into(),
            })?;
            rename_durable(&staging, destination)?;
            // A sink may cancel (or a test corrupt bytes) before verification.
            self.emit(&request.job_id, "copied", total, total);
            check()?;
            verify_tree(source, destination)?;
        } else {
            undo.record(Undo::Moved {
                from: destination.into(),
                to: source.into(),
            })?;
            rename_durable(source, destination)?;
        }
        self.emit(&request.job_id, "moved", total, total);
        check()?;
        let mut report = self.repair_steps(
            &RepairRequest {
                job_id: request.job_id.clone(),
                old_dir: source.to_string_lossy().into_owned(),
                game_dir: destination.into(),
                references: request.references.clone(),
                spellings: request.spellings.clone(),
                alias: request.alias.clone(),
                // Adopt the target root's identity when it has one.
                root_id: destination
                    .parent()
                    .and_then(|root| crate::manifest::RootManifest::read(root).ok())
                    .map(|root| root.root_id().to_owned()),
                relative_dir: None,
                verdict,
            },
            total,
            cancel,
            undo,
        )?;
        report.copied = copy;
        Ok(report)
    }

    /// Reconcile entry point (#61): rewrite references from `old_dir` to the
    /// game's current directory, retarget its alias and prefix links and
    /// update the manifest, all-or-nothing.
    pub fn repair(
        &self,
        request: &RepairRequest,
        cancel: &CancellationToken,
    ) -> Result<RelocateReport, RelocateError> {
        let mut undo = UndoLog::memory();
        self.repair_steps(request, 0, cancel, &mut undo)
            .map_err(|error| self.roll_back(error, cancel, &mut undo))
    }

    /// Undo every recorded change after `error` and report the outcome.
    fn roll_back(
        &self,
        error: RelocateError,
        cancel: &CancellationToken,
        undo: &mut UndoLog,
    ) -> RelocateError {
        let rollback = self.rollback_log(undo);
        let error = if cancel.is_cancelled() {
            RelocateError::Cancelled
        } else {
            RelocateError::RolledBack(error.to_string())
        };
        match rollback {
            Ok(()) => error,
            Err(rollback) => RelocateError::RollbackFailed {
                error: error.to_string(),
                rollback,
            },
        }
    }

    fn repair_steps(
        &self,
        request: &RepairRequest,
        total: u64,
        cancel: &CancellationToken,
        undo: &mut UndoLog,
    ) -> Result<RelocateReport, RelocateError> {
        let check = || check_cancelled(cancel);
        let destination = request.game_dir.as_path();
        let source = Path::new(&request.old_dir);
        if let Some((store, id)) = &request.alias {
            let previous = store.target(id).map_err(io::Error::other)?;
            undo.record(match previous {
                Some(target) => Undo::Alias {
                    store: store.clone(),
                    id: id.clone(),
                    target,
                },
                None => Undo::AliasCreated {
                    store: store.clone(),
                    id: id.clone(),
                },
            })?;
            store.link(id, destination).map_err(io::Error::other)?;
        }
        // Absolute prefix links (prefixes kept outside the game) follow it.
        retarget_prefix_links(destination, source, undo)?;
        let mut spellings =
            spelling_pairs(&source.to_string_lossy(), &destination.to_string_lossy());
        spellings.extend(request.spellings.iter().cloned());
        let mut report = RelocateReport {
            game_dir: destination.into(),
            copied: false,
            rewritten_files: 0,
            rewritten_values: 0,
        };
        let mut registry = vec![];
        let mut rewritten = std::collections::BTreeSet::new();
        // Only rewritable places are touched; binary/Fragile evidence is kept
        // in the manifest but never written (move what we can, §7.1).
        for reference in request
            .references
            .iter()
            .filter(|r| r.via == Via::Real && r.location.rewritable())
        {
            check()?;
            match &reference.location {
                Location::GameFile { path, .. } => {
                    if self.rewrite_file(&destination.join(path), &spellings, undo)? {
                        report.rewritten_files += 1;
                        rewritten.insert(path.replace('\\', "/"));
                    }
                }
                Location::ExternalFile { path, .. } => {
                    if self.rewrite_file(Path::new(path), &spellings, undo)? {
                        report.rewritten_files += 1;
                    }
                }
                Location::Registry {
                    hive,
                    key,
                    name,
                    value_type,
                    ..
                } => {
                    if !matches!(
                        value_type.as_str(),
                        "REG_SZ" | "REG_EXPAND_SZ" | "REG_MULTI_SZ"
                    ) {
                        continue;
                    }
                    let Some(original) = self.registry.read(hive, key, name)? else {
                        continue;
                    };
                    if let Some(data) = replace_all(&original.data, &spellings) {
                        registry.push((original.clone(), RegistryWrite { data, ..original }));
                    }
                }
            }
            self.emit(&request.job_id, "rewriting", total, total);
        }
        if !registry.is_empty() {
            check()?;
            let (originals, writes): (Vec<_>, Vec<_>) = registry.iter().cloned().unzip();
            report.rewritten_values = writes.len();
            // Recorded before applying: a partially applied batch is undone.
            undo.record(Undo::Registry(originals))?;
            self.registry.apply(&writes)?;
        }
        check()?;
        self.update_manifest(
            destination,
            &spellings,
            request,
            &rewritten,
            &registry,
            undo,
        )?;
        Ok(report)
    }

    /// Replace every spelling in a file (any encoding, BOM and line endings
    /// preserved since only the path bytes change). `.lnk` files go through
    /// the shortcut editor. Returns whether the file changed.
    fn rewrite_file(
        &self,
        path: &Path,
        spellings: &[(String, String)],
        undo: &mut UndoLog,
    ) -> Result<bool, RelocateError> {
        let Ok(original) = fs::read(path) else {
            return Ok(false);
        };
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
        {
            undo.record(Undo::File {
                path: path.into(),
                original,
            })?;
            for (old, new) in spellings {
                self.shortcuts.rewrite(path, old, new)?;
            }
            return Ok(true);
        }
        let Some(updated) = replace_all(&original, spellings) else {
            return Ok(false);
        };
        undo.record(Undo::File {
            path: path.into(),
            original,
        })?;
        write_atomic(path, &updated)?;
        Ok(true)
    }

    fn update_manifest(
        &self,
        game: &Path,
        spellings: &[(String, String)],
        request: &RepairRequest,
        rewritten: &std::collections::BTreeSet<String>,
        registry: &[(RegistryWrite, RegistryWrite)],
        undo: &mut UndoLog,
    ) -> Result<(), RelocateError> {
        let path = game.join(crate::manifest::INSTALL_MANIFEST_PATH);
        let Ok(original) = fs::read(&path) else {
            return Ok(());
        };
        let manifest = InstallManifest::read(game).map_err(io::Error::other)?;
        let mut record = manifest.into_record();
        // The caller's (possibly rescanned and merged) references become the
        // recorded evidence; external paths are then moved with the game.
        record.relocation.references = request.references.clone();
        for reference in &mut record.relocation.references {
            if let Location::ExternalFile { path, .. } = &mut reference.location
                && let Some(updated) = replace_all(path.as_bytes(), spellings)
            {
                *path = String::from_utf8_lossy(&updated).into_owned();
            }
        }
        record.relocation.last_path = Some(game.to_string_lossy().into_owned());
        // Uninstall compares the host registry with the recorded effects:
        // they follow the values rewritten here. A prefix keeps its own
        // registry, which no recorded effect describes.
        if record.runtime == crate::RuntimeSpec::Native {
            effects::follow_rewrites(
                &mut record.external_effects,
                registry,
                spellings,
                &*self.registry,
            )?;
        }
        // Only files rewritten by this transaction get a new size and hash;
        // anything else changed on disk must stay detectable (e.g. a tampered
        // uninstaller is never re-blessed).
        for file in record
            .files
            .iter_mut()
            .filter(|file| rewritten.contains(file.path.as_str()))
        {
            let path = game.join(file.path.as_str());
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            file.size = metadata.len();
            if file.sha256.is_some() {
                let mut hasher = Sha256::new();
                io::copy(&mut fs::File::open(&path)?, &mut hasher)?;
                file.sha256 = Some(
                    crate::manifest::Sha256Digest::new(format!("{:x}", hasher.finalize()))
                        .map_err(io::Error::other)?,
                );
            }
        }
        if let Some(root_id) = &request.root_id {
            record.plan.root_id = root_id.clone();
        }
        if let Some(relative_dir) = &request.relative_dir {
            record.plan.relative_dir = relative_dir.clone();
        }
        if let Some(verdict) = request.verdict {
            record.relocation.verdict = verdict;
        }
        undo.record(Undo::File {
            path: path.clone(),
            original,
        })?;
        InstallManifest::new(record)
            .and_then(|manifest| manifest.write(game))
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn rollback(&self, undo: Vec<Undo>) -> Result<(), String> {
        let mut failures = vec![];
        // Newest first, except aliases: they need their old target to exist,
        // so they are restored after the game has moved back.
        let (aliases, rest): (Vec<_>, Vec<_>) = undo
            .into_iter()
            .partition(|step| matches!(step, Undo::Alias { .. } | Undo::AliasCreated { .. }));
        for step in rest.into_iter().rev().chain(aliases.into_iter().rev()) {
            let result = match step {
                Undo::File { path, original } => write_atomic(&path, &original),
                Undo::Registry(originals) => self.registry.apply(&originals),
                Undo::Alias { store, id, target } => store
                    .restore(&id, &target)
                    .map(|_| ())
                    .map_err(io::Error::other),
                Undo::Link { path, target } => relink(&path, &target),
                Undo::AliasCreated { store, id } => store.remove(&id).map_err(io::Error::other),
                Undo::Moved { from, to } => match (from.try_exists(), to.try_exists()) {
                    (Ok(true), Ok(false)) => rename_durable(&from, &to),
                    (Ok(false), Ok(true)) => Ok(()),
                    _ => Err(io::Error::other("ambiguous move rollback paths")),
                },
                Undo::Copied { to } => match fs::symlink_metadata(&to) {
                    Ok(_) => remove_tree(&to),
                    Err(_) => Ok(()),
                },
            };
            if let Err(error) = result {
                failures.push(error.to_string());
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn emit(&self, job_id: &str, message: &str, bytes: u64, total: u64) {
        self.sink.emit(JobEvent {
            job_id: job_id.into(),
            phase: Phase::Move,
            step: None,
            bytes,
            total_bytes: Some(total),
            files: 0,
            eta_secs: None,
            message: message.into(),
        });
    }
}

fn check_cancelled(cancel: &CancellationToken) -> Result<(), RelocateError> {
    if cancel.is_cancelled() {
        Err(RelocateError::Cancelled)
    } else {
        Ok(())
    }
}

fn retarget_prefix_links(game: &Path, old: &Path, undo: &mut UndoLog) -> io::Result<()> {
    let link = game.join(".caskade/prefix/drive_g/Game");
    if let Ok(target) = fs::read_link(&link)
        && target.is_absolute()
        && target.starts_with(old)
    {
        undo.record(Undo::Link {
            path: link.clone(),
            target,
        })?;
        relink(&link, game)?;
    }
    Ok(())
}
