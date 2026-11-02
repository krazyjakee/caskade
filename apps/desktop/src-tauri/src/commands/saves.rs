//! Device-local save locations for the game editor (#238).
use crate::error::{CommandError, report};
use caskade_core::{
    db::LibraryStore,
    emulation::EmulationService,
    ludusavi::Ludusavi,
    saves::{
        LocationSpec, SaveMatch, SavePreferences, SaveState, Saves,
        versions::{BeforePlay, SaveHistory, SaveVersion, SaveVersions},
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

/// A game's save versions for its Saves panel (#241), newest first.
#[tauri::command]
#[specta::specta]
pub async fn game_save_history(
    versions: State<'_, SaveVersions>,
    game_id: String,
) -> Result<SaveHistory, CommandError> {
    versions.history(&game_id).await.map_err(report)
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
    versions
        .restore(&entry_id, &version_id)
        .await
        .map_err(report)
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
    let state = versions.saves().state(&entry_id).await.map_err(report)?;
    let missing = || {
        report(caskade_core::Error::Invalid(caskade_core::tr!(
            "That save location isn't on this device."
        )))
    };
    let location = state.locations.get(location as usize).ok_or_else(missing)?;
    let path = std::path::PathBuf::from(&location.spec.path);
    let first = location.files.first().map(std::path::PathBuf::from);
    // The folder itself, a file's folder, or where its first file is.
    let folder = [
        Some(path.clone()),
        path.parent().map(std::path::Path::to_path_buf),
        first
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| candidate.is_absolute() && candidate.is_dir())
    .ok_or_else(missing)?;
    super::library::open_file_manager([&folder], &folder)
}

/// Whether the Ludusavi manifest is cached, and when it was last checked.
#[tauri::command]
#[specta::specta]
pub async fn saves_manifest_status(
    manifest: State<'_, Ludusavi>,
) -> Result<caskade_core::ludusavi::ManifestStatus, CommandError> {
    manifest.status().await.map_err(report)
}

/// Checks the manifest's source now (Update now), from the configured URL.
#[tauri::command]
#[specta::specta]
pub async fn saves_manifest_update(
    manifest: State<'_, Ludusavi>,
    settings: State<'_, caskade_core::settings::SettingsService>,
) -> Result<caskade_core::ludusavi::ManifestStatus, CommandError> {
    let configured = settings
        .read_or_default::<caskade_core::settings::SavesSettings>("core.saves")
        .await;
    let manifest = manifest.for_url(&configured.manifest_url);
    manifest.update_now().await.map_err(report)?;
    manifest.status().await.map_err(report)
}

/// Game ids whose save versions changed here (#241).
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event,
)]
pub struct SaveVersionsChanged(pub String);

/// Sends each game whose versions or save in place changed to the webview.
pub fn forward_changes<R: tauri::Runtime>(app: &tauri::AppHandle<R>, versions: &SaveVersions) {
    use tauri_specta::Event as _;
    let mut changes = versions.changes();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(game_id) => {
                    if let Err(error) = SaveVersionsChanged(game_id).emit(&handle) {
                        tracing::warn!(%error, "save versions change not delivered");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
#[path = "saves_tests.rs"]
mod tests;
