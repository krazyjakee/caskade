//! Device-local save locations for the game editor (#238).
use crate::error::{CommandError, report};
use caskade_core::{
    db::LibraryStore,
    emulation::EmulationService,
    ludusavi::Ludusavi,
    saves::{
        LocationSpec, SaveMatch, SavePreferences, SaveState, Saves,
        versions::{BeforePlay, SaveVersion, SaveVersions},
    },
};
use tauri::State;

/// The editor needs locations and counts, not saved file or registry contents.
#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocationView {
    spec: LocationSpec,
    tags: Vec<String>,
    file_count: usize,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocationsState {
    entry_id: String,
    revision: u32,
    preferences: SavePreferences,
    matched: SaveMatch,
    automatic: Vec<SaveLocationView>,
    locations: Vec<SaveLocationView>,
    warnings: Vec<String>,
}

impl From<SaveState> for SaveLocationsState {
    fn from(state: SaveState) -> Self {
        let location = |location: caskade_core::saves::SaveLocation| SaveLocationView {
            spec: location.spec,
            tags: location.tags,
            file_count: location.files.len(),
        };
        Self {
            entry_id: state.entry_id,
            revision: state.revision,
            preferences: state.preferences,
            matched: state.matched,
            automatic: state.automatic.into_iter().map(location).collect(),
            locations: state.locations.into_iter().map(location).collect(),
            warnings: state.warnings,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn game_saves_state(
    store: State<'_, LibraryStore>,
    manifest: State<'_, Ludusavi>,
    emulation: State<'_, EmulationService>,
    entry_id: String,
) -> Result<SaveLocationsState, CommandError> {
    Saves::new(store.inner().clone(), manifest.inner().clone())
        .with_catalog(emulation.catalog())
        .state(&entry_id)
        .await
        .map(Into::into)
        .map_err(report)
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
    Saves::new(store.inner().clone(), manifest.inner().clone())
        .with_catalog(emulation.catalog())
        .edit(&entry_id, preferences, revision)
        .await
        .map(Into::into)
        .map_err(report)
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
    versions.before_play(&entry_id).await.map_err(report)
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
    versions
        .choose(&entry_id, &version_id)
        .await
        .map_err(report)
}
