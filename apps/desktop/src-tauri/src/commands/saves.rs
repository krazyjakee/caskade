//! Device-local save locations for the game editor (#238).
use crate::error::CommandError;
pub use app::*;
use caskade_app::commands::saves as app;
use caskade_core::{
    db::LibraryStore,
    emulation::EmulationService,
    ludusavi::Ludusavi,
    saves::{
        SavePreferences,
        versions::{BeforePlay, SaveHistory, SaveVersion, SaveVersions},
    },
};
use tauri::State;
#[tauri::command]
#[specta::specta]
pub async fn game_saves_state(
    store: State<'_, LibraryStore>,
    manifest: State<'_, Ludusavi>,
    emulation: State<'_, EmulationService>,
    entry_id: String,
) -> Result<SaveLocationsState, CommandError> {
    app::game_saves_state(&store, &manifest, &emulation, entry_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn game_saves_edit(
    store: State<'_, LibraryStore>,
    manifest: State<'_, Ludusavi>,
    emulation: State<'_, EmulationService>,
    entry_id: String,
    preferences: SavePreferences,
    revision: u32,
) -> Result<SaveLocationsState, CommandError> {
    app::game_saves_edit(
        &store,
        &manifest,
        &emulation,
        entry_id,
        preferences,
        revision,
    )
    .await
}

/// Before a launch (#239): keeps a save changed here, then puts a newer one
/// from another device in place, or says both devices played. Waits for
/// other devices a few seconds at most.
#[tauri::command]
#[specta::specta]
pub async fn game_saves_before_play(
    versions: State<'_, SaveVersions>,
    entry_id: String,
) -> Result<BeforePlay, CommandError> {
    app::game_saves_before_play(&versions, entry_id).await
}

/// Settles a [`SaveCheck::Conflict`](caskade_core::saves::versions::SaveCheck):
/// the chosen version's files go in place, and the others stay as versions.
#[tauri::command]
#[specta::specta]
pub async fn game_saves_choose(
    versions: State<'_, SaveVersions>,
    entry_id: String,
    version_id: String,
) -> Result<SaveVersion, CommandError> {
    app::game_saves_choose(&versions, entry_id, version_id).await
}

/// A game's save versions for its Saves panel (#241), newest first.
#[tauri::command]
#[specta::specta]
pub async fn game_save_history(
    versions: State<'_, SaveVersions>,
    game_id: String,
) -> Result<SaveHistory, CommandError> {
    app::game_save_history(&versions, game_id).await
}

/// Puts an older version's files in place for the release (#241); the save
/// there now is kept as a version first.
#[tauri::command]
#[specta::specta]
pub async fn game_saves_restore(
    versions: State<'_, SaveVersions>,
    entry_id: String,
    version_id: String,
) -> Result<SaveVersion, CommandError> {
    app::game_saves_restore(&versions, entry_id, version_id).await
}

/// Shows one of the release's save locations (by its place in the list)
/// in the file manager. Takes the entry, never a path.
#[tauri::command]
#[specta::specta]
pub async fn game_saves_reveal(
    versions: State<'_, SaveVersions>,
    entry_id: String,
    location: u32,
) -> Result<(), CommandError> {
    app::game_saves_reveal(&versions, entry_id, location).await
}

/// Whether the Ludusavi manifest is cached, and when it was last checked.
#[tauri::command]
#[specta::specta]
pub async fn saves_manifest_status(
    manifest: State<'_, Ludusavi>,
) -> Result<caskade_core::ludusavi::ManifestStatus, CommandError> {
    app::saves_manifest_status(&manifest).await
}

/// Checks the manifest's source now (Update now), from the configured URL.
#[tauri::command]
#[specta::specta]
pub async fn saves_manifest_update(
    manifest: State<'_, Ludusavi>,
    settings: State<'_, caskade_core::settings::SettingsService>,
) -> Result<caskade_core::ludusavi::ManifestStatus, CommandError> {
    app::saves_manifest_update(&manifest, &settings).await
}

#[cfg(test)]
#[path = "saves_tests.rs"]
mod tests;
