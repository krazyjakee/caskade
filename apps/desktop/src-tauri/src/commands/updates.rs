//! Game updates: newer releases of games installed from GitHub, checked
//! without an account (`caskade_core::updates`). Updates are install jobs
//! (`kind: update`), followed like any other. Forwarded to
//! `caskade_app::commands::updates`.
use crate::error::CommandError;
use caskade_app::commands::updates as app;
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
    app::game_updates(&updates, &game_id).await
}

/// Asks the install's provider for its newest release now.
#[tauri::command]
#[specta::specta]
pub async fn game_update_check(
    updates: State<'_, Updates>,
    entry_id: String,
) -> Result<GameUpdate, CommandError> {
    app::game_update_check(&updates, &entry_id).await
}

/// The update to offer before the install is played; `null` to play
/// straight away.
#[tauri::command]
#[specta::specta]
pub async fn game_update_before_play(
    updates: State<'_, Updates>,
    entry_id: String,
) -> Result<Option<GameUpdate>, CommandError> {
    app::game_update_before_play(&updates, &entry_id).await
}

/// Stops offering release `version`; `null` offers every release again.
#[tauri::command]
#[specta::specta]
pub async fn game_update_skip(
    updates: State<'_, Updates>,
    entry_id: String,
    version: Option<String>,
) -> Result<GameUpdate, CommandError> {
    app::game_update_skip(&updates, &entry_id, version).await
}

/// Offers prereleases too, or stops, and checks again.
#[tauri::command]
#[specta::specta]
pub async fn game_update_prereleases(
    updates: State<'_, Updates>,
    entry_id: String,
    prereleases: bool,
) -> Result<GameUpdate, CommandError> {
    app::game_update_prereleases(&updates, &entry_id, prereleases).await
}

/// Installs the newest release over the game, keeping the player's files.
#[tauri::command]
#[specta::specta]
pub async fn game_update_install(
    updates: State<'_, Updates>,
    launch: State<'_, LaunchService>,
    entry_id: String,
) -> Result<JobSnapshot, CommandError> {
    app::game_update_install(&updates, &launch, &entry_id).await
}
