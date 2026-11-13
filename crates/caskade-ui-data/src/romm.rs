//! RomM servers in Settings → Emulation (#1, #105): the servers and their
//! states, adding, changing, testing, syncing and removing one, the
//! download cache and running downloads, through the app layer's commands
//! as the webview's `api/romm.ts` does. Passwords and API tokens go
//! straight to the OS keychain and never come back.

use caskade_app::commands::romm as commands;
use caskade_core::error::ErrorReport;
use caskade_core::romm::RommService;

/// The types the panel shows.
pub use caskade_core::romm::{
    CacheStatus, ConnectionReport, ConnectionState, DownloadPhase, DownloadProgress, RommAuth,
    RommOverview, RommServerConfig, RommSettings, RommSyncReport, ServerStatus,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What the RomM panel reads and changes, over one app's RomM service.
#[derive(Clone)]
pub struct Romm {
    romm: RommService,
}

impl Romm {
    pub fn new(romm: RommService) -> Self {
        Self { romm }
    }

    /// The running app's; `None` where RomM didn't start.
    pub fn of(services: &caskade_app::start::Services) -> Option<Self> {
        services.romm.clone().map(Self::new)
    }

    /// Settings, server states, the cache and running downloads.
    pub async fn overview(&self) -> Result<RommOverview> {
        commands::romm_overview(&self.romm).await
    }

    /// Replaces `core.romm`; servers left out go with their games.
    pub async fn set_settings(&self, settings: RommSettings) -> Result<RommOverview> {
        commands::romm_settings_set(&self.romm, settings).await
    }

    /// Adds or changes a server; `secret` goes to the keychain, `None`
    /// keeps the saved one.
    pub async fn save_server(
        &self,
        server: RommServerConfig,
        secret: Option<String>,
    ) -> Result<RommOverview> {
        commands::romm_server_save(&self.romm, server, secret).await
    }

    pub async fn remove_server(&self, id: String) -> Result<RommOverview> {
        commands::romm_server_remove(&self.romm, id).await
    }

    /// Test connection, with `secret` before it is saved.
    pub async fn test(
        &self,
        server: RommServerConfig,
        secret: Option<String>,
    ) -> Result<ConnectionReport> {
        commands::romm_server_test(&self.romm, server, secret).await
    }

    /// Syncs server `id` now, or every server.
    pub async fn sync(&self, id: Option<String>) -> Result<Vec<RommSyncReport>> {
        commands::romm_sync(&self.romm, id).await
    }

    /// Removes every download except kept games.
    pub async fn clear_cache(&self) -> Result<CacheStatus> {
        commands::romm_cache_clear(&self.romm).await
    }

    pub fn cancel_download(&self, entry_id: String) -> bool {
        commands::romm_cancel_download(&self.romm, entry_id)
    }
}

impl PartialEq for Romm {
    /// One app's: a panel's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
