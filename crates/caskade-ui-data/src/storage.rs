//! Storage locations for flows (#108): the locations and their games, the
//! "always use the default" preference, and moving a game's files, through
//! the app layer's commands as the webview's `api/storageNative.ts` does.

use std::time::Duration;

use caskade_app::commands::{install, storage};
use caskade_core::{
    error::ErrorReport,
    install::{InstallService, JobSnapshot, JobStatus},
    settings::SettingsService,
    storage::{RootSummary, StorageService},
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// Why a move didn't finish; the UI words the first two.
#[derive(Clone, Debug, PartialEq)]
pub enum MoveError {
    /// This app has no install service.
    Unavailable,
    /// The job stopped for an answer in Activity.
    NeedsUser,
    /// It failed or was cancelled, with the core's message.
    Failed(String),
}

/// What storage flows read and change, over one app's services.
#[derive(Clone)]
pub struct Storage {
    storage: StorageService,
    install: Option<InstallService>,
    settings: SettingsService,
}

impl Storage {
    pub fn new(
        storage: StorageService,
        install: Option<InstallService>,
        settings: SettingsService,
    ) -> Self {
        Self {
            storage,
            install,
            settings,
        }
    }

    /// The running app's storage; `None` where it has none.
    pub fn of(services: &caskade_app::start::Services) -> Option<Self> {
        Some(Self::new(
            services.storage.clone()?,
            services.install.clone(),
            services.settings.clone(),
        ))
    }

    /// Every location, with its state, space and games.
    pub async fn roots(&self) -> Result<Vec<RootSummary>> {
        storage::storage_list(&self.storage).await
    }

    /// Whether installs go to the default location without asking. The
    /// setting's schema gives it a default, so a missing value is `false`.
    pub async fn always_use_default(&self) -> Result<bool> {
        let snapshot = self
            .settings
            .get("core.storage")
            .await
            .map_err(|error| ErrorReport::from_error(&error))?;
        Ok(snapshot
            .value
            .get("alwaysUseDefault")
            .and_then(|value| value.as_bool())
            .unwrap_or(false))
    }

    /// Whether this app can install games.
    pub fn can_install(&self) -> bool {
        self.install.is_some()
    }

    /// Starts installing `input` (a path, a URL or a magnet link) on the
    /// location `root_id` (the default without one), as a job in Activity;
    /// the core's message when it can't start.
    pub async fn install(
        &self,
        input: String,
        root_id: Option<String>,
    ) -> std::result::Result<JobSnapshot, MoveError> {
        let install = self.install.as_ref().ok_or(MoveError::Unavailable)?;
        install::install_start(install, input, root_id)
            .await
            .map_err(MoveError::Failed)
    }

    /// Moves an installation's files to another location, as a job in
    /// Activity, and waits for it: an error when it fails, is cancelled or
    /// needs an answer in Activity.
    pub async fn move_game(
        &self,
        entry_id: String,
        target_root_id: String,
    ) -> std::result::Result<(), MoveError> {
        let install = self.install.as_ref().ok_or(MoveError::Unavailable)?;
        let job = install::move_game(install, &self.storage, entry_id, target_root_id)
            .await
            .map_err(MoveError::Failed)?;
        complete(install, job).await
    }
}

/// Waits for `job` to end, as the webview's `complete`.
async fn complete(
    install: &InstallService,
    mut job: JobSnapshot,
) -> std::result::Result<(), MoveError> {
    loop {
        match job.status {
            JobStatus::Completed => return Ok(()),
            JobStatus::Failed | JobStatus::Cancelled => {
                let status = if job.status == JobStatus::Failed {
                    "failed"
                } else {
                    "cancelled"
                };
                return Err(MoveError::Failed(
                    job.error.unwrap_or_else(|| format!("Job {status}")),
                ));
            }
            JobStatus::NeedsUser => return Err(MoveError::NeedsUser),
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        job = install::job_get(install, job.id.clone()).map_err(MoveError::Failed)?;
    }
}

impl PartialEq for Storage {
    /// One storage per app: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
