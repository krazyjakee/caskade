//! Launch actions (#177/#178): a game's releases with their play and tool
//! actions, editing them, and running a tool. Targets are checked in the
//! core; the webview never sends a command line. Forwarded to
//! `caskade_app::commands::actions`.
use crate::error::CommandError;
use caskade_app::commands::actions as app;
#[cfg(all(test, target_os = "linux"))]
use caskade_core::bus::Event;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    install::ProtonBuilds,
    launch::{LaunchService, ReleaseActions},
    model::{EntryActionSettings, GameAction},
    settings::SettingsService,
};
use tauri::State;

pub use app::ToolOutcome;

/// Each release of the game with its actions: its own launch first, then
/// the ones the user added.
#[tauri::command]
#[specta::specta]
pub async fn game_actions(
    store: State<'_, LibraryStore>,
    game_id: String,
) -> Result<Vec<ReleaseActions>, CommandError> {
    app::game_actions(&store, &game_id).await
}

/// Adds or changes an action. An empty `id` adds one; an empty `entryId`
/// adds it to the game's manual release, made for it when needed.
#[tauri::command]
#[specta::specta]
pub async fn game_action_save(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    action: GameAction,
) -> Result<GameAction, CommandError> {
    app::game_action_save(&store, &bus, &game_id, action).await
}

#[tauri::command]
#[specta::specta]
pub async fn game_action_delete(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    id: String,
) -> Result<bool, CommandError> {
    app::game_action_delete(&store, &bus, &game_id, &id).await
}

/// Puts a release's added actions in the order of `ids`.
#[tauri::command]
#[specta::specta]
pub async fn game_actions_order(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    entry_id: String,
    ids: Vec<String>,
) -> Result<(), CommandError> {
    app::game_actions_order(&store, &bus, &game_id, &entry_id, &ids).await
}

/// A release's default action and ask-at-launch choice.
#[tauri::command]
#[specta::specta]
pub async fn game_action_settings_save(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    entry_id: String,
    settings: EntryActionSettings,
) -> Result<EntryActionSettings, CommandError> {
    app::game_action_settings_save(&store, &bus, &game_id, &entry_id, settings).await
}

/// The Proton builds installed on this device that an action can choose,
/// and the one an action without its own runs now.
#[tauri::command]
#[specta::specta]
pub async fn proton_builds(
    settings: State<'_, SettingsService>,
) -> Result<ProtonBuilds, CommandError> {
    app::proton_builds(&settings).await
}

/// Runs a release's tool: no play session, playtime or last played.
#[tauri::command]
#[specta::specta]
pub async fn game_tool_run(
    service: State<'_, LaunchService>,
    entry_id: String,
    action_id: String,
) -> Result<ToolOutcome, CommandError> {
    app::game_tool_run(&service, &entry_id, &action_id).await
}

#[cfg(all(test, target_os = "linux"))]
#[path = "actions_tests.rs"]
mod tests;
