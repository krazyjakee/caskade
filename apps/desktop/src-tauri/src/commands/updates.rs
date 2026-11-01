//! Game updates: newer releases of games installed from GitHub, checked
//! without an account (`caskade_core::updates`). Updates are install jobs
//! (`kind: update`), followed like any other.
use crate::error::{CommandError, report};
use caskade_core::{
    install::JobSnapshot,
    launch::LaunchService,
    updates::{GameUpdate, Updates},
};
use tauri::State;

/// The updates of a game's followed installs; empty when none is followed.
#[tauri::command]
#[specta::specta]
pub async fn game_updates(
    updates: State<'_, Updates>,
    game_id: String,
) -> Result<Vec<GameUpdate>, CommandError> {
    updates.game(&game_id).await.map_err(report)
}

/// The installs with a newer release out.
#[tauri::command]
#[specta::specta]
pub async fn updates_available(
    updates: State<'_, Updates>,
) -> Result<Vec<GameUpdate>, CommandError> {
    updates.available().await.map_err(report)
}

/// Asks the install's provider for its newest release now.
#[tauri::command]
#[specta::specta]
pub async fn game_update_check(
    updates: State<'_, Updates>,
    entry_id: String,
) -> Result<GameUpdate, CommandError> {
    updates.check(&entry_id).await.map_err(report)
}

/// The update to offer before the install is played; `null` to play
/// straight away.
#[tauri::command]
#[specta::specta]
pub async fn game_update_before_play(
    updates: State<'_, Updates>,
    entry_id: String,
) -> Result<Option<GameUpdate>, CommandError> {
    updates.before_play(&entry_id).await.map_err(report)
}

/// Stops offering release `version`; `null` offers every release again.
#[tauri::command]
#[specta::specta]
pub async fn game_update_skip(
    updates: State<'_, Updates>,
    entry_id: String,
    version: Option<String>,
) -> Result<GameUpdate, CommandError> {
    updates.skip(&entry_id, version).await.map_err(report)
}

/// Offers prereleases too, or stops, and checks again.
#[tauri::command]
#[specta::specta]
pub async fn game_update_prereleases(
    updates: State<'_, Updates>,
    entry_id: String,
    prereleases: bool,
) -> Result<GameUpdate, CommandError> {
    updates
        .set_prereleases(&entry_id, prereleases)
        .await
        .map_err(report)
}

/// Installs the newest release over the game, keeping the player's files.
#[tauri::command]
#[specta::specta]
pub async fn game_update_install(
    updates: State<'_, Updates>,
    launch: State<'_, LaunchService>,
    entry_id: String,
) -> Result<JobSnapshot, CommandError> {
    updates
        .install(&entry_id, launch.inner().clone())
        .await
        .map_err(report)
}
