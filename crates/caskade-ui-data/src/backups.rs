//! Settings → Backup (#193, #243, #105): Back up now, what a backup would
//! hold, checking and staging a restore, the backups paired devices keep,
//! and whether a restart may apply a restore, through the app layer's
//! commands as the webview's `api/backup.ts` does. The panel's settings are
//! `core.backups`.

use caskade_app::commands::{backup, peer_backups};
use caskade_backup::Backups as Service;
use caskade_backup::peers::PeerBackups;
use caskade_core::devices::Devices;
use caskade_core::error::ErrorReport;
use caskade_core::install::InstallService;
use caskade_core::launch::LaunchService;
use caskade_core::sync::SyncService;

/// The types the panel shows.
pub use caskade_app::commands::peer_backups::DeviceBackups;
pub use caskade_backup::peers::HeldBackup;
pub use caskade_backup::{
    BackupJobKind, BackupPlan, BackupStatus, Component, ComponentSummary, Exclusion, Inspection,
    Journal, LastBackup, Manifest, Phase, Progress, RestoreOutcome, RunningJob,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What Settings → Backup reads and does, over one app's backups.
#[derive(Clone)]
pub struct Backups {
    backups: Service,
    peers: Option<PeerBackups>,
    devices: Option<Devices>,
    sync: Option<SyncService>,
    launch: Option<LaunchService>,
    install: Option<InstallService>,
}

impl Backups {
    pub fn new(backups: Service) -> Self {
        Self {
            backups,
            peers: None,
            devices: None,
            sync: None,
            launch: None,
            install: None,
        }
    }

    /// The running app's.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self {
            backups: services.backups.clone(),
            peers: Some(services.peer_backups.clone()),
            devices: Some(services.devices.clone()),
            sync: Some(services.sync.clone()),
            launch: Some(services.launch.clone()),
            install: services.install.clone(),
        }
    }

    pub async fn status(&self) -> Result<BackupStatus> {
        backup::backup_status(&self.backups).await
    }

    /// What a backup to `destination` (the settings' folder without one)
    /// with `components` would hold.
    pub async fn plan(
        &self,
        destination: Option<String>,
        components: Vec<Component>,
    ) -> Result<BackupPlan> {
        backup::backup_plan(&self.backups, destination, components).await
    }

    /// Back up now; the status follows it to its outcome.
    pub async fn start(&self) -> Result<()> {
        backup::backup_start(&self.backups).await
    }

    pub fn cancel(&self) {
        backup::backup_cancel(&self.backups);
    }

    /// Checks a backup and says what restoring it would do.
    pub async fn inspect(&self, path: String) -> Result<Inspection> {
        backup::backup_inspect(&self.backups, path).await
    }

    /// Stages a restore; a restart applies it.
    pub async fn restore(&self, path: String, components: Vec<Component>) -> Result<Journal> {
        backup::backup_restore(&self.backups, path, components).await
    }

    pub async fn discard_restore(&self) -> Result<()> {
        backup::backup_restore_discard(&self.backups).await
    }

    /// Shows a backup in the file manager.
    pub async fn reveal(&self, path: String) -> Result<()> {
        backup::backup_reveal(&self.backups, path).await
    }

    /// Whether a restart may apply the staged restore now.
    pub async fn restart_ready(&self) -> Result<()> {
        backup::backup_restart_ready(&self.backups, self.launch.as_ref(), self.install.as_ref())
            .await
    }

    /// The backups paired devices keep.
    pub async fn on_devices(&self) -> Result<Vec<DeviceBackups>> {
        peer_backups::backups_on_devices(self.peers.as_ref(), self.devices.as_ref()).await
    }

    /// Fetches backup `hash` from the device that keeps it: where it is
    /// here.
    pub async fn fetch_from_device(&self, hash: &str) -> Result<String> {
        peer_backups::backup_fetch_from_device(self.peers.as_ref(), self.sync.as_ref(), hash).await
    }

    /// The paired devices, for Also keep backups on.
    pub async fn paired(&self) -> Vec<caskade_core::devices::PairedDevice> {
        match &self.devices {
            Some(devices) => devices.status().await.devices,
            None => Vec::new(),
        }
    }

    /// Hears every change to the status.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<Option<BackupStatus>> {
        self.backups.subscribe()
    }
}

impl PartialEq for Backups {
    /// One app's: a panel's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
