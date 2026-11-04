//! Settings → Emulators IPC (#116): emulators, profiles and platform
//! settings over `caskade_core::emulation::EmulationService`.

use crate::error::{CommandError, conflict, refused, report};
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
pub async fn emulation_overview(state: State<'_>) -> Result<EmulationOverview, CommandError> {
    state.overview().await.map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn emulators_list(state: State<'_>) -> Result<Vec<EmulatorView>, CommandError> {
    state.emulators().await.map_err(report)
}

/// Creates (empty id) or updates an emulator and its profiles.
#[tauri::command]
#[specta::specta]
pub async fn emulator_save(
    state: State<'_>,
    emulator: Emulator,
) -> Result<EmulatorView, CommandError> {
    state.save_emulator(emulator).await.map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn emulator_delete(state: State<'_>, id: String) -> Result<(), CommandError> {
    match state.delete_emulator(&id).await.map_err(report)? {
        true => Ok(()),
        false => Err(conflict(caskade_core::tr!(
            "This emulator no longer exists."
        ))),
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
pub fn args_split(text: String) -> Result<Vec<String>, CommandError> {
    EmulationService::split_args(&text).map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn emulation_platforms(state: State<'_>) -> Result<Vec<PlatformView>, CommandError> {
    state.platforms().await.map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn platform_settings_set(
    state: State<'_>,
    settings: PlatformSettings,
) -> Result<PlatformView, CommandError> {
    state.set_platform_settings(settings).await.map_err(report)
}

/// Starts detecting emulators in standard locations and, if given, a
/// folder the user chose. Results arrive as `emulationDetectChanged`.
#[tauri::command]
#[specta::specta]
pub async fn emulation_detect_start(
    state: State<'_>,
    folder: Option<String>,
) -> Result<DetectJob, CommandError> {
    let folder = folder.map(std::path::PathBuf::from);
    if folder
        .as_deref()
        .is_some_and(|folder| !folder.is_absolute())
    {
        return Err(refused(caskade_core::tr!("choose a folder")));
    }
    state.detect_start(folder).await.map_err(report)
}

#[tauri::command]
#[specta::specta]
pub fn emulation_detect_get(state: State<'_>, id: String) -> Result<DetectJob, CommandError> {
    state.detect_get(&id).ok_or_else(|| {
        conflict(caskade_core::tr!(
            "This detection is no longer available. Run it again."
        ))
    })
}

/// The most recent detection, e.g. the one the last library scan ran.
#[tauri::command]
#[specta::specta]
pub fn emulation_detect_latest(state: State<'_>) -> Option<DetectJob> {
    state.detect_latest()
}

#[tauri::command]
#[specta::specta]
pub fn emulation_detect_cancel(state: State<'_>, id: String) -> Result<DetectJob, CommandError> {
    state.detect_cancel(&id).map_err(report)
}

/// Imports the ticked results of the reviewed revision of a detection.
#[tauri::command]
#[specta::specta]
pub async fn emulation_detect_import(
    state: State<'_>,
    id: String,
    revision: u32,
    keys: Vec<String>,
) -> Result<DetectImport, CommandError> {
    state
        .detect_import(&id, revision, &keys)
        .await
        .map_err(report)
}

/// An emulated game's emulator, discs, BIOS and whether it can launch.
#[tauri::command]
#[specta::specta]
pub async fn emulation_game(
    state: State<'_>,
    entry_id: String,
) -> Result<GameEmulation, CommandError> {
    state.game(&entry_id).await.map_err(report)
}

/// Pins, un-pins or asks for a game's emulator, and sets its arguments.
#[tauri::command]
#[specta::specta]
pub async fn emulation_game_set(
    state: State<'_>,
    entry_id: String,
    target: EmulatorTarget,
    args: ArgsOverride,
) -> Result<GameEmulation, CommandError> {
    state
        .set_game(&entry_id, target, args)
        .await
        .map_err(report)
}

#[cfg(test)]
#[path = "emulation_tests.rs"]
mod tests;
