//! Launch actions (#177/#178): a game's releases with their play and tool
//! actions, editing them, and running a tool. Targets are checked in the
//! core; the webview never sends a command line.
use crate::error::{CommandError, report};
use caskade_core::{
    accounts::AgeRestriction,
    bus::{Bus, Event},
    db::LibraryStore,
    install::{self, ProtonBuilds},
    launch::{self, LaunchError, LaunchService, ReleaseActions},
    model::{EntryActionSettings, GameAction},
    settings::SettingsService,
};
use serde::Serialize;
use specta::Type;
use tauri::State;

/// Each release of the game with its actions: its own launch first, then
/// the ones the user added.
#[tauri::command]
#[specta::specta]
pub async fn game_actions(
    store: State<'_, LibraryStore>,
    game_id: String,
) -> Result<Vec<ReleaseActions>, CommandError> {
    launch::game_actions(&store, &game_id).await.map_err(report)
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
    let saved = launch::save_action(&store, &game_id, &action)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id });
    Ok(saved)
}

#[tauri::command]
#[specta::specta]
pub async fn game_action_delete(
    store: State<'_, LibraryStore>,
    bus: State<'_, Bus>,
    game_id: String,
    id: String,
) -> Result<bool, CommandError> {
    let deleted = launch::delete_action(&store, &game_id, &id)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id });
    Ok(deleted)
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
    release_of(&store, &game_id, &entry_id).await?;
    store
        .order_game_actions(&entry_id, &ids)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id });
    Ok(())
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
    release_of(&store, &game_id, &entry_id).await?;
    let saved = store
        .save_entry_action_settings(&entry_id, &settings)
        .await
        .map_err(report)?;
    bus.emit(Event::LibraryGameUpdated { game_id });
    Ok(saved)
}

/// The Proton builds installed on this device that an action can choose,
/// and the one an action without its own runs now.
#[tauri::command]
#[specta::specta]
pub async fn proton_builds(
    settings: State<'_, SettingsService>,
) -> Result<ProtonBuilds, CommandError> {
    install::proton_builds(&settings).await.map_err(report)
}

/// A tool started, or the age limit holds the game back until an admin's
/// PIN approves it (`game_age_approve`).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutcome {
    Started,
    AgeRestricted { restriction: AgeRestriction },
}

/// Runs a release's tool: no play session, playtime or last played.
#[tauri::command]
#[specta::specta]
pub async fn game_tool_run(
    service: State<'_, LaunchService>,
    entry_id: String,
    action_id: String,
) -> Result<ToolOutcome, CommandError> {
    match service.run_tool(&entry_id, &action_id).await {
        Ok(()) => Ok(ToolOutcome::Started),
        Err(LaunchError::AgeRestricted(restriction)) => Ok(ToolOutcome::AgeRestricted {
            restriction: *restriction,
        }),
        Err(error) => Err(report(error)),
    }
}

async fn release_of(
    store: &LibraryStore,
    game_id: &str,
    entry_id: &str,
) -> Result<(), CommandError> {
    match store.get_entry(entry_id).await.map_err(report)? {
        Some(entry) if entry.game_id == game_id => Ok(()),
        _ => Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
            "This copy of the game is no longer in the library."
        )))),
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "actions_tests.rs"]
mod tests;
