//! Settings → Emulators IPC (#116): emulators, profiles and platform
//! settings over `caskade_core::emulation::EmulationService`.

use super::text;
use caskade_core::emulation::{
    ArgsOverride, ArgvPreview, DetectImport, DetectJob, EmulationCatalog, EmulationOverview,
    EmulationService, Emulator, EmulatorTarget, EmulatorView, GameEmulation, PlatformSettings,
    PlatformView, ProfileDraft,
};

type State<'a> = tauri::State<'a, EmulationService>;

/// Platforms, emulator definitions and the variables reference.
#[tauri::command]
#[specta::specta]
pub fn emulation_catalog(state: State<'_>) -> EmulationCatalog {
    state.catalog_view()
}

/// The Settings → Emulators header stats.
#[tauri::command]
#[specta::specta]
pub async fn emulation_overview(state: State<'_>) -> Result<EmulationOverview, String> {
    state.overview().await.map_err(text)
}

#[tauri::command]
#[specta::specta]
pub async fn emulators_list(state: State<'_>) -> Result<Vec<EmulatorView>, String> {
    state.emulators().await.map_err(text)
}

/// Creates (empty id) or updates an emulator and its profiles.
#[tauri::command]
#[specta::specta]
pub async fn emulator_save(state: State<'_>, emulator: Emulator) -> Result<EmulatorView, String> {
    state.save_emulator(emulator).await.map_err(text)
}

#[tauri::command]
#[specta::specta]
pub async fn emulator_delete(state: State<'_>, id: String) -> Result<(), String> {
    match state.delete_emulator(&id).await.map_err(text)? {
        true => Ok(()),
        false => Err(caskade_core::tr!("unknown emulator {id}", id = id)),
    }
}

/// The command line of a profile being edited, before it is saved.
#[tauri::command]
#[specta::specta]
pub fn profile_preview(state: State<'_>, draft: ProfileDraft) -> ArgvPreview {
    state.preview(&draft)
}

/// Splits arguments typed as one string with shell-word rules.
#[tauri::command]
#[specta::specta]
pub fn args_split(text: String) -> Result<Vec<String>, String> {
    EmulationService::split_args(&text).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn emulation_platforms(state: State<'_>) -> Result<Vec<PlatformView>, String> {
    state.platforms().await.map_err(text)
}

#[tauri::command]
#[specta::specta]
pub async fn platform_settings_set(
    state: State<'_>,
    settings: PlatformSettings,
) -> Result<PlatformView, String> {
    state.set_platform_settings(settings).await.map_err(text)
}

/// Starts detecting emulators in standard locations and, if given, a
/// folder the user chose. Results arrive as `emulationDetectChanged`.
#[tauri::command]
#[specta::specta]
pub async fn emulation_detect_start(
    state: State<'_>,
    folder: Option<String>,
) -> Result<DetectJob, String> {
    let folder = folder.map(std::path::PathBuf::from);
    if folder
        .as_deref()
        .is_some_and(|folder| !folder.is_absolute())
    {
        return Err(caskade_core::tr!("choose a folder"));
    }
    state.detect_start(folder).await.map_err(text)
}

#[tauri::command]
#[specta::specta]
pub fn emulation_detect_get(state: State<'_>, id: String) -> Result<DetectJob, String> {
    state
        .detect_get(&id)
        .ok_or_else(|| caskade_core::tr!("unknown detection {id}", id = id))
}

/// The most recent detection, e.g. the one the last library scan ran.
#[tauri::command]
#[specta::specta]
pub fn emulation_detect_latest(state: State<'_>) -> Option<DetectJob> {
    state.detect_latest()
}

#[tauri::command]
#[specta::specta]
pub fn emulation_detect_cancel(state: State<'_>, id: String) -> Result<DetectJob, String> {
    state.detect_cancel(&id).map_err(text)
}

/// Imports the ticked results of the reviewed revision of a detection.
#[tauri::command]
#[specta::specta]
pub async fn emulation_detect_import(
    state: State<'_>,
    id: String,
    revision: u32,
    keys: Vec<String>,
) -> Result<DetectImport, String> {
    state
        .detect_import(&id, revision, &keys)
        .await
        .map_err(text)
}

/// An emulated game's emulator, discs, BIOS and whether it can launch.
#[tauri::command]
#[specta::specta]
pub async fn emulation_game(state: State<'_>, entry_id: String) -> Result<GameEmulation, String> {
    state.game(&entry_id).await.map_err(text)
}

/// Pins, un-pins or asks for a game's emulator, and sets its arguments.
#[tauri::command]
#[specta::specta]
pub async fn emulation_game_set(
    state: State<'_>,
    entry_id: String,
    target: EmulatorTarget,
    args: ArgsOverride,
) -> Result<GameEmulation, String> {
    state.set_game(&entry_id, target, args).await.map_err(text)
}

#[cfg(test)]
#[path = "emulation_tests.rs"]
mod tests;
