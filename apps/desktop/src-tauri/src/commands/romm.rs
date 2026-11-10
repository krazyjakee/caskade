//! RomM servers IPC (#1) over `caskade_core::romm::RommService`: servers
//! (credentials go to the OS keychain, never back to the webview), sync,
//! the download cache and download progress. Launching a RomM game needs
//! no command of its own: the launch pipeline downloads it first.
use crate::error::CommandError;
pub use app::*;
use caskade_app::commands::romm as app;
use caskade_core::romm::{
    CacheStatus, ConnectionReport, RommGameStatus, RommOverview, RommServerConfig, RommService,
    RommSettings, RommSyncReport,
};
type State<'a> = tauri::State<'a, RommService>;

/// Settings, server states, the cache and running downloads.
#[tauri::command]
#[specta::specta]
pub async fn romm_overview(state: State<'_>) -> Result<RommOverview, CommandError> {
    app::romm_overview(&state).await
}

/// Replaces `core.romm`; removed servers leave with their games and cache.
#[tauri::command]
#[specta::specta]
pub async fn romm_settings_set(
    state: State<'_>,
    settings: RommSettings,
) -> Result<RommOverview, CommandError> {
    app::romm_settings_set(&state, settings).await
}

/// Adds or updates a server. `secret` (password or API token) goes to the
/// keychain; `null` keeps the saved one.
#[tauri::command]
#[specta::specta]
pub async fn romm_server_save(
    state: State<'_>,
    server: RommServerConfig,
    secret: Option<String>,
) -> Result<RommOverview, CommandError> {
    app::romm_server_save(&state, server, secret).await
}

#[tauri::command]
#[specta::specta]
pub async fn romm_server_remove(
    state: State<'_>,
    id: String,
) -> Result<RommOverview, CommandError> {
    app::romm_server_remove(&state, id).await
}

/// "Test connection": with `secret`, before it is saved.
#[tauri::command]
#[specta::specta]
pub async fn romm_server_test(
    state: State<'_>,
    server: RommServerConfig,
    secret: Option<String>,
) -> Result<ConnectionReport, CommandError> {
    app::romm_server_test(&state, server, secret).await
}

/// Syncs one server now, or every server. Syncing every server fails when
/// none of them synced; otherwise it reports the ones that did, and each
/// failed server's status carries its error.
#[tauri::command]
#[specta::specta]
pub async fn romm_sync(
    state: State<'_>,
    id: Option<String>,
) -> Result<Vec<RommSyncReport>, CommandError> {
    app::romm_sync(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn romm_cache_status(state: State<'_>) -> Result<CacheStatus, CommandError> {
    app::romm_cache_status(&state).await
}

/// Removes every download except pinned games.
#[tauri::command]
#[specta::specta]
pub async fn romm_cache_clear(state: State<'_>) -> Result<CacheStatus, CommandError> {
    app::romm_cache_clear(&state).await
}

/// A RomM game's cache state; `null` for other entries.
#[tauri::command]
#[specta::specta]
pub async fn romm_game_status(
    state: State<'_>,
    entry_id: String,
) -> Result<Option<RommGameStatus>, CommandError> {
    app::romm_game_status(&state, entry_id).await
}

/// "Keep downloaded".
#[tauri::command]
#[specta::specta]
pub async fn romm_set_pinned(
    state: State<'_>,
    entry_id: String,
    pinned: bool,
) -> Result<RommGameStatus, CommandError> {
    app::romm_set_pinned(&state, entry_id, pinned).await
}

/// Removes one game's download.
#[tauri::command]
#[specta::specta]
pub async fn romm_evict(state: State<'_>, entry_id: String) -> Result<bool, CommandError> {
    app::romm_evict(&state, entry_id).await
}

/// Cancels a running download; the launch that started it fails as
/// cancelled and the partial file is kept for a resume.
#[tauri::command]
#[specta::specta]
pub fn romm_cancel_download(state: State<'_>, entry_id: String) -> bool {
    app::romm_cancel_download(&state, entry_id)
}
