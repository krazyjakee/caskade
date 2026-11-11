//! Storage locations for flows (#108) and Settings → Storage (#104): the
//! locations and their games, the "always use the default" preference,
//! moving a game's files, and adding, editing, choosing the default and
//! removing locations, through the app layer's commands as the webview's
//! `api/storageNative.ts` does.

use std::time::Duration;

use caskade_app::commands::{install, storage};
use caskade_core::{
    accounts::AccountService,
    error::ErrorReport,
    install::{InstallService, JobSnapshot, JobStatus},
    settings::SettingsService,
    storage::{RootSummary, StorageService},
};
pub use caskade_sources::storage::StorageSource;

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
    /// What adds a location and adopts its games, and whose permissions a
    /// move checks: Settings → Storage's (`with_locations`).
    locations: Option<(StorageSource, AccountService)>,
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
            locations: None,
        }
    }

    /// Adds what Settings → Storage needs to add locations and move them.
    pub fn with_locations(mut self, source: StorageSource, accounts: AccountService) -> Self {
        self.locations = Some((source, accounts));
        self
    }

    /// The running app's storage; `None` where it has none.
    pub fn of(services: &caskade_app::start::Services) -> Option<Self> {
        let storage = Self::new(
            services.storage.clone()?,
            services.install.clone(),
            services.settings.clone(),
        );
        Some(match services.storage_source.clone() {
            Some(source) => storage.with_locations(source, services.accounts.clone()),
            None => storage,
        })
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

impl Storage {
    /// Whether this app can add and move locations.
    pub fn manages_locations(&self) -> bool {
        self.locations.is_some()
    }

    /// Adds the folder at `path` as a location and adopts the games already
    /// in it; what went wrong comes back as its warnings.
    pub async fn add(&self, path: String, label: Option<String>) -> Result<RootSummary> {
        let (source, _) = self.locations.as_ref().ok_or_else(unavailable)?;
        storage::storage_add(&self.storage, source, path, label).await
    }

    /// Changes a location's label and folder template.
    pub async fn edit(
        &self,
        id: &str,
        label: Option<String>,
        folder_template: String,
    ) -> Result<RootSummary> {
        storage::storage_edit(&self.storage, id, label, folder_template).await
    }

    /// Makes a location the default for new installs.
    pub async fn set_default(&self, id: &str) -> Result<RootSummary> {
        storage::storage_default(&self.storage, id).await
    }

    /// Removes a location from the library, keeping its files.
    pub async fn remove(&self, id: &str) -> Result<()> {
        storage::storage_remove(&self.storage, id).await
    }

    /// Moves every game on `id` to `target_id`, then removes `id`, as one
    /// job in Activity, and waits for it.
    pub async fn move_and_remove(
        &self,
        id: String,
        target_id: String,
    ) -> std::result::Result<(), MoveError> {
        let install = self.install.as_ref().ok_or(MoveError::Unavailable)?;
        let (_, accounts) = self.locations.as_ref().ok_or(MoveError::Unavailable)?;
        let job = storage::storage_move_and_remove(accounts, install, &self.storage, id, target_id)
            .await
            .map_err(|error| MoveError::Failed(error.summary))?;
        complete(install, job).await
    }
}

fn unavailable() -> ErrorReport {
    ErrorReport::new(
        caskade_core::error::ErrorCode::Other,
        caskade_core::tr!(
            "Storage is unavailable. Open the Caskade desktop app to manage locations."
        ),
    )
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
