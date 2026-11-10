//! Settings → Emulators IPC (#116): emulators, profiles and platform
//! settings over `caskade_core::emulation::EmulationService`. Forwarded to
//! `caskade_app::commands::emulation`.

use crate::error::CommandError;
use caskade_app::commands::emulation as app;
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
    app::emulation_catalog(&state)
}

/// The Settings → Emulators header stats.
#[tauri::command]
#[specta::specta]
pub async fn emulation_overview(state: State<'_>) -> Result<EmulationOverview, CommandError> {
    app::emulation_overview(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn emulators_list(state: State<'_>) -> Result<Vec<EmulatorView>, CommandError> {
    app::emulators_list(&state).await
}

/// Creates (empty id) or updates an emulator and its profiles.
#[tauri::command]
#[specta::specta]
pub async fn emulator_save(
    state: State<'_>,
    emulator: Emulator,
) -> Result<EmulatorView, CommandError> {
    app::emulator_save(&state, emulator).await
}

#[tauri::command]
#[specta::specta]
pub async fn emulator_delete(state: State<'_>, id: String) -> Result<(), CommandError> {
    app::emulator_delete(&state, &id).await
}

/// The command line of a profile being edited, before it is saved.
#[tauri::command]
#[specta::specta]
pub fn profile_preview(state: State<'_>, draft: ProfileDraft) -> ArgvPreview {
    app::profile_preview(&state, &draft)
}

/// Splits arguments typed as one string with shell-word rules.
#[tauri::command]
#[specta::specta]
pub fn args_split(text: String) -> Result<Vec<String>, CommandError> {
    app::args_split(&text)
}

#[tauri::command]
#[specta::specta]
pub async fn emulation_platforms(state: State<'_>) -> Result<Vec<PlatformView>, CommandError> {
    app::emulation_platforms(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn platform_settings_set(
    state: State<'_>,
    settings: PlatformSettings,
) -> Result<PlatformView, CommandError> {
    app::platform_settings_set(&state, settings).await
}

/// Starts detecting emulators in standard locations and, if given, a
/// folder the user chose. Results arrive as `emulationDetectChanged`.
#[tauri::command]
#[specta::specta]
pub async fn emulation_detect_start(
    state: State<'_>,
    folder: Option<String>,
) -> Result<DetectJob, CommandError> {
    app::emulation_detect_start(&state, folder).await
}

#[tauri::command]
#[specta::specta]
pub fn emulation_detect_get(state: State<'_>, id: String) -> Result<DetectJob, CommandError> {
    app::emulation_detect_get(&state, &id)
}

/// The most recent detection, e.g. the one the last library scan ran.
#[tauri::command]
#[specta::specta]
pub fn emulation_detect_latest(state: State<'_>) -> Option<DetectJob> {
    app::emulation_detect_latest(&state)
}

#[tauri::command]
#[specta::specta]
pub fn emulation_detect_cancel(state: State<'_>, id: String) -> Result<DetectJob, CommandError> {
    app::emulation_detect_cancel(&state, &id)
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
    app::emulation_detect_import(&state, &id, revision, &keys).await
}

/// An emulated game's emulator, discs, BIOS and whether it can launch.
#[tauri::command]
#[specta::specta]
pub async fn emulation_game(
    state: State<'_>,
    entry_id: String,
) -> Result<GameEmulation, CommandError> {
    app::emulation_game(&state, &entry_id).await
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
    app::emulation_game_set(&state, &entry_id, target, args).await
}

#[cfg(test)]
#[path = "emulation_tests.rs"]
mod tests;
