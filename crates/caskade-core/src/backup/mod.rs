//! App-data backups (#191): the library database, settings and accounts in
//! one versioned, verifiable archive, with artwork, plugins, their data and
//! themes as optional components. See `README.md` beside this file.

mod archive;
mod export;
mod manifest;
#[cfg(feature = "net")]
pub mod peers;
pub mod policy;
pub mod restore;
#[cfg(test)]
mod restore_tests;
mod schedule;
#[cfg(test)]
mod schedule_tests;
mod snapshot;
#[cfg(test)]
mod tests;

pub use archive::verify;
pub use export::{
    BackupPlan, EXTENSION, ExportOptions, Exported, PREFIX, Phase, PlannedComponent, Progress,
    ProgressFn,
};
pub use manifest::{
    Component, ComponentSummary, DATABASE, Exclusion, FORMAT, FileEntry, MANIFEST, Manifest,
    TableRows,
};
pub use restore::{Inspection, Journal, RestoreOptions, RestoreOutcome, RestoreState};
pub use schedule::{
    BackupJobKind, BackupSettings, BackupStatus, LastBackup, NAMESPACE, RunningJob, Schedule,
    register_settings,
};

use crate::{AppPaths, db::LibraryStore};
use std::{path::PathBuf, sync::Arc};
/// What backups and restores take to stop them.
pub use tokio_util::sync::CancellationToken;

/// Makes and checks backups. Clones share one lock, so one backup runs at
/// a time.
#[derive(Clone)]
pub struct Backups {
    store: LibraryStore,
    paths: AppPaths,
    running: Arc<tokio::sync::Mutex<()>>,
    faults: Arc<std::sync::Mutex<Faults>>,
    settings: Arc<std::sync::OnceLock<crate::settings::SettingsService>>,
    /// The job Settings shows, and what cancels it.
    job: Arc<std::sync::Mutex<Option<(CancellationToken, RunningJob)>>>,
    changes: Arc<tokio::sync::watch::Sender<Option<BackupStatus>>>,
    last_restore: Arc<std::sync::Mutex<Option<RestoreOutcome>>>,
}

/// Failures tests inject.
#[derive(Debug, Clone, Copy, Default)]
struct Faults {
    /// The archive's disk fills after this many bytes.
    disk_full_after: Option<u64>,
    /// The process "dies" just before publishing: nothing is cleaned up.
    crash: bool,
    /// The destination's free space.
    available: Option<u64>,
}

impl Backups {
    pub fn new(store: LibraryStore, paths: AppPaths) -> Self {
        Self {
            store,
            paths,
            running: Arc::default(),
            faults: Arc::default(),
            settings: Arc::default(),
            job: Arc::default(),
            changes: Arc::new(tokio::sync::watch::channel(None).0),
            last_restore: Arc::default(),
        }
    }

    /// `<data>/backups`, created on first use.
    pub fn default_destination(&self) -> PathBuf {
        self.paths.data.join("backups")
    }

    /// Checks a backup and says what restoring it would do; changes
    /// nothing.
    pub async fn inspect(
        &self,
        archive: &std::path::Path,
        progress: ProgressFn,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Inspection, BackupError> {
        let _running = self.running.try_lock().map_err(|_| BackupError::Busy)?;
        let _activity = caskade_jobs::global().activity("Checking a backup");
        restore::inspect(&self.paths, archive, &cancel, &progress).await
    }

    /// Stages a restore for the next start ([`restore::stage`]); the app
    /// restarts to apply it.
    pub async fn stage_restore(
        &self,
        options: RestoreOptions,
        progress: ProgressFn,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Journal, BackupError> {
        let _running = self.running.try_lock().map_err(|_| BackupError::Busy)?;
        let _activity = caskade_jobs::global().activity("Restoring a backup");
        restore::stage(&self.paths, &options, &cancel, &progress).await
    }

    /// The restore waiting for a restart, if any.
    pub fn staged_restore(&self) -> Result<Option<Journal>, BackupError> {
        restore::pending(&self.paths)
    }

    /// Forgets the restore waiting for a restart.
    pub async fn discard_restore(&self) -> Result<(), BackupError> {
        let _running = self.running.try_lock().map_err(|_| BackupError::Busy)?;
        restore::discard_staged(&self.paths).await
    }

    fn faults(&self) -> Faults {
        *crate::lock(&self.faults)
    }
}

/// Why a backup failed. The text is for people.
#[derive(Debug)]
#[non_exhaustive]
pub enum BackupError {
    /// Another backup or restore is running.
    Busy,
    Cancelled,
    /// The destination folder isn't there (a drive not connected).
    DestinationMissing(PathBuf),
    NoSpace {
        needed: u64,
        available: u64,
    },
    /// The archive or the snapshot isn't what it should be.
    Damaged(String),
    /// A newer Caskade wrote it.
    Newer,
    Io(std::io::Error),
    Store(crate::Error),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Busy => crate::tr!("Another backup or restore is under way."),
            Self::Cancelled => crate::tr!("The backup was cancelled."),
            Self::DestinationMissing(path) => crate::tr!(
                "The backup folder “{path}” isn't there. Connect its drive or choose another folder.",
                path = path.display()
            ),
            Self::NoSpace { needed, available } => crate::tr!(
                "There isn't enough free space for the backup: it needs about {needed} MB and {available} MB are free.",
                needed = needed.div_ceil(1024 * 1024),
                available = available / (1024 * 1024)
            ),
            Self::Damaged(why) => crate::tr!("The backup is damaged: {why}.", why = why),
            Self::Newer => {
                crate::tr!("This backup was made by a newer version of Caskade.")
            }
            Self::Io(error) => error.to_string(),
            Self::Store(error) => error.to_string(),
        })
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::Error> for BackupError {
    fn from(error: crate::Error) -> Self {
        Self::Store(error)
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Io(std::io::Error::other(error))
    }
}
