//! RomM servers IPC (#1) over `caskade_core::romm::RommService`: servers
//! (credentials go to the OS keychain, never back to the webview), sync,
//! the download cache and download progress. Launching a RomM game needs
//! no command of its own: the launch pipeline downloads it first.
use super::text;
use caskade_core::romm::{
    CacheStatus, ConnectionReport, DownloadProgress, RommGameStatus, RommOverview,
    RommServerConfig, RommService, RommSettings, RommSyncReport,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::Manager;
use tauri_specta::Event as _;

/// Every change of a RomM download (progress, done, failed, cancelled).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct RommDownloadChanged(pub DownloadProgress);

type State<'a> = tauri::State<'a, RommService>;

/// Registers `core.romm`, builds the service, forwards download progress
/// and starts the background sync.
pub(crate) fn setup<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
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
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match updates.recv().await {
                Ok(progress) => {
                    if let Err(error) = RommDownloadChanged(progress).emit(&handle) {
                        tracing::warn!(%error, "RomM download event not delivered");
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
    tauri::async_runtime::spawn({
        let romm = romm.clone();
        async move {
            romm.watch();
        }
    });
    app.manage(romm.clone());
    Ok(romm)
}

/// Settings, server states, the cache and running downloads.
#[tauri::command]
#[specta::specta]
pub async fn romm_overview(state: State<'_>) -> Result<RommOverview, String> {
    state.overview().await.map_err(text)
}

/// Replaces `core.romm`; removed servers leave with their games and cache.
#[tauri::command]
#[specta::specta]
pub async fn romm_settings_set(
    state: State<'_>,
    settings: RommSettings,
) -> Result<RommOverview, String> {
    state.save_settings(settings).await.map_err(text)
}

/// Adds or updates a server. `secret` (password or API token) goes to the
/// keychain; `null` keeps the saved one.
#[tauri::command]
#[specta::specta]
pub async fn romm_server_save(
    state: State<'_>,
    server: RommServerConfig,
    secret: Option<String>,
) -> Result<RommOverview, String> {
    if secret.as_ref().is_some_and(|secret| secret.len() > 4096) {
        return Err(caskade_core::tr!("the password or token is too long"));
    }
    state.save_server(server, secret).await.map_err(text)
}

#[tauri::command]
#[specta::specta]
pub async fn romm_server_remove(state: State<'_>, id: String) -> Result<RommOverview, String> {
    state.remove_server(&id).await.map_err(text)
}

/// "Test connection": with `secret`, before it is saved.
#[tauri::command]
#[specta::specta]
pub async fn romm_server_test(
    state: State<'_>,
    server: RommServerConfig,
    secret: Option<String>,
) -> Result<ConnectionReport, String> {
    state.test_connection(server, secret).await.map_err(text)
}

/// Syncs one server now, or every server. Syncing every server fails when
/// none of them synced; otherwise it reports the ones that did, and each
/// failed server's status carries its error.
#[tauri::command]
#[specta::specta]
pub async fn romm_sync(
    state: State<'_>,
    id: Option<String>,
) -> Result<Vec<RommSyncReport>, String> {
    match id {
        Some(id) => Ok(vec![state.sync(&id).await.map_err(text)?]),
        None => all_synced(state.sync_all().await),
    }
}

fn all_synced(
    outcomes: Vec<caskade_core::romm::Result<RommSyncReport>>,
) -> Result<Vec<RommSyncReport>, String> {
    let (reports, errors): (Vec<_>, Vec<_>) = outcomes.into_iter().partition(Result::is_ok);
    if reports.is_empty() && !errors.is_empty() {
        return Err(errors
            .into_iter()
            .filter_map(Result::err)
            .map(text)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    Ok(reports.into_iter().filter_map(Result::ok).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn romm_cache_status(state: State<'_>) -> Result<CacheStatus, String> {
    state.cache_status().await.map_err(text)
}

/// Removes every download except pinned games.
#[tauri::command]
#[specta::specta]
pub async fn romm_cache_clear(state: State<'_>) -> Result<CacheStatus, String> {
    state.clear_cache().await.map_err(text)
}

/// A RomM game's cache state; `null` for other entries.
#[tauri::command]
#[specta::specta]
pub async fn romm_game_status(
    state: State<'_>,
    entry_id: String,
) -> Result<Option<RommGameStatus>, String> {
    state.game_status(&entry_id).await.map_err(text)
}

/// "Keep downloaded".
#[tauri::command]
#[specta::specta]
pub async fn romm_set_pinned(
    state: State<'_>,
    entry_id: String,
    pinned: bool,
) -> Result<RommGameStatus, String> {
    state.set_pinned(&entry_id, pinned).await.map_err(text)
}

/// Removes one game's download.
#[tauri::command]
#[specta::specta]
pub async fn romm_evict(state: State<'_>, entry_id: String) -> Result<bool, String> {
    state.evict(&entry_id).await.map_err(text)
}

/// Cancels a running download; the launch that started it fails as
/// cancelled and the partial file is kept for a resume.
#[tauri::command]
#[specta::specta]
pub fn romm_cancel_download(state: State<'_>, entry_id: String) -> bool {
    state.cancel_download(&entry_id)
}

#[cfg(test)]
mod tests {
    use super::all_synced;
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
        assert_eq!(error.lines().count(), 2);
        assert!(error.contains("can't reach the server"), "{error}");
        let reports = all_synced(vec![offline(), synced()]).unwrap();
        assert_eq!(reports.len(), 1);
        assert!(all_synced(Vec::new()).unwrap().is_empty());
    }
}
