//! RomM servers IPC (#1) over `caskade_core::romm::RommService`: servers
//! (credentials go to the OS keychain, never back to the webview), sync,
//! the download cache and download progress. Launching a RomM game needs
//! no command of its own: the launch pipeline downloads it first.
use crate::error::{CommandError, refused, report};
use caskade_core::romm::{
    CacheStatus, ConnectionReport, DownloadProgress, RommGameStatus, RommOverview,
    RommServerConfig, RommService, RommSettings, RommSyncReport,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};
/// Every change of a RomM download (progress, done, failed, cancelled).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RommDownloadChanged(pub DownloadProgress);

/// Registers `core.romm`, builds the service, sends download progress to
/// `events` and starts the background sync.
pub fn setup(
    events: Events,
    store: caskade_core::db::LibraryStore,
    bus: caskade_core::bus::Bus,
    settings: &caskade_core::settings::SettingsService,
    emulation: caskade_core::emulation::EmulationService,
    paths: &caskade_core::AppPaths,
    artwork: Option<caskade_core::artwork::ArtworkCache>,
) -> Result<RommService, Box<dyn std::error::Error>> {
    caskade_core::romm::register_settings(settings)?;
    let mut romm = RommService::new(
        store,
        bus,
        settings.clone(),
        emulation,
        paths.data.join("romm-cache"),
    );
    if let Some(artwork) = artwork {
        romm = romm.with_artwork(artwork);
    }
    let mut updates = romm.subscribe();
    crate::runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match updates.recv().await {
                Ok(progress) => {
                    events.send(Event::RommDownloadChanged(RommDownloadChanged(progress)))
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
    crate::runtime::spawn({
        let romm = romm.clone();
        async move {
            romm.watch();
        }
    });
    Ok(romm)
}

/// Settings, server states, the cache and running downloads.
pub async fn romm_overview(state: &RommService) -> Result<RommOverview, CommandError> {
    state.overview().await.map_err(report)
}

/// Replaces `core.romm`; removed servers leave with their games and cache.
pub async fn romm_settings_set(
    state: &RommService,
    settings: RommSettings,
) -> Result<RommOverview, CommandError> {
    crate::permissions::enforce("romm_settings_set", serde_json::Value::default)?;
    state.save_settings(settings).await.map_err(report)
}

/// Adds or updates a server. `secret` (password or API token) goes to the
/// keychain; `null` keeps the saved one.
pub async fn romm_server_save(
    state: &RommService,
    server: RommServerConfig,
    secret: Option<String>,
) -> Result<RommOverview, CommandError> {
    crate::permissions::enforce("romm_server_save", serde_json::Value::default)?;
    if secret.as_ref().is_some_and(|secret| secret.len() > 4096) {
        return Err(refused(caskade_core::tr!(
            "the password or token is too long"
        )));
    }
    state.save_server(server, secret).await.map_err(report)
}

pub async fn romm_server_remove(
    state: &RommService,
    id: String,
) -> Result<RommOverview, CommandError> {
    crate::permissions::enforce("romm_server_remove", serde_json::Value::default)?;
    state.remove_server(&id).await.map_err(report)
}

/// "Test connection": with `secret`, before it is saved.
pub async fn romm_server_test(
    state: &RommService,
    server: RommServerConfig,
    secret: Option<String>,
) -> Result<ConnectionReport, CommandError> {
    crate::permissions::enforce("romm_server_test", serde_json::Value::default)?;
    state.test_connection(server, secret).await.map_err(report)
}

/// Syncs one server now, or every server. Syncing every server fails when
/// none of them synced; otherwise it reports the ones that did, and each
/// failed server's status carries its error.
pub async fn romm_sync(
    state: &RommService,
    id: Option<String>,
) -> Result<Vec<RommSyncReport>, CommandError> {
    crate::permissions::enforce("romm_sync", serde_json::Value::default)?;
    match id {
        Some(id) => Ok(vec![state.sync(&id).await.map_err(report)?]),
        None => all_synced(state.sync_all().await),
    }
}

fn all_synced(
    outcomes: Vec<caskade_core::romm::Result<RommSyncReport>>,
) -> Result<Vec<RommSyncReport>, CommandError> {
    let (reports, errors): (Vec<_>, Vec<_>) = outcomes.into_iter().partition(Result::is_ok);
    // The first failure's code and hints, with every server's summary.
    let failure = errors
        .into_iter()
        .filter_map(Result::err)
        .map(report)
        .reduce(|mut first, next| {
            first.summary.push('\n');
            first.summary.push_str(&next.summary);
            first
        });
    if let (true, Some(failure)) = (reports.is_empty(), failure) {
        return Err(failure);
    }
    Ok(reports.into_iter().filter_map(Result::ok).collect())
}

/// Removes every download except pinned games.
pub async fn romm_cache_clear(state: &RommService) -> Result<CacheStatus, CommandError> {
    crate::permissions::enforce("romm_cache_clear", serde_json::Value::default)?;
    state.clear_cache().await.map_err(report)
}

/// "Keep downloaded".
pub async fn romm_set_pinned(
    state: &RommService,
    entry_id: String,
    pinned: bool,
) -> Result<RommGameStatus, CommandError> {
    crate::permissions::enforce("romm_set_pinned", serde_json::Value::default)?;
    state.set_pinned(&entry_id, pinned).await.map_err(report)
}

/// Removes one game's download.
pub async fn romm_evict(state: &RommService, entry_id: String) -> Result<bool, CommandError> {
    crate::permissions::enforce("romm_evict", serde_json::Value::default)?;
    state.evict(&entry_id).await.map_err(report)
}

/// Cancels a running download; the launch that started it fails as
/// cancelled and the partial file is kept for a resume.
pub fn romm_cancel_download(state: &RommService, entry_id: String) -> bool {
    if crate::permissions::enforce("romm_cancel_download", serde_json::Value::default).is_err() {
        return false;
    }
    state.cancel_download(&entry_id)
}

#[cfg(test)]
mod tests {
    use super::all_synced;
    use crate::error::ErrorCode;
    use caskade_core::romm::{RommError, RommSyncReport};

    #[test]
    fn syncing_every_server_fails_only_when_none_synced() {
        let offline = || Err(RommError::Unreachable("connection refused".into()));
        let synced = || {
            Ok(RommSyncReport {
                server_id: "home".into(),
                ..Default::default()
            })
        };
        let error = all_synced(vec![offline(), offline()]).unwrap_err();
        assert_eq!(error.code, ErrorCode::Offline);
        assert_eq!(error.summary.lines().count(), 2);
        assert!(error.summary.contains("Can't reach the server"), "{error}");
        let reports = all_synced(vec![offline(), synced()]).unwrap();
        assert_eq!(reports.len(), 1);
        assert!(all_synced(Vec::new()).unwrap().is_empty());
    }
}
