//! Device-local save locations for the game editor (#238).
use crate::error::{CommandError, report};
use caskade_core::{
    db::LibraryStore,
    emulation::EmulationService,
    ludusavi::Ludusavi,
    saves::{LocationSpec, SaveMatch, SavePreferences, SaveState, Saves},
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
