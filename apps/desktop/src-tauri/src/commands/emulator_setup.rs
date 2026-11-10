//! One-click emulator installs over
//! `caskade_core::emulation::EmulatorSetup`. Forwarded to
//! `caskade_app::commands::emulator_setup`.

use crate::error::CommandError;
use caskade_app::commands::emulator_setup as app;
use caskade_core::emulation::{
    BiosFetch, EmulatorSetup, Installable, ManagedEmulator, MissingPlatform, SetupJob,
};

type State<'a> = tauri::State<'a, EmulatorSetup>;

/// Every emulator Caskade can install on this system.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_installable(
    state: State<'_>,
) -> Result<Vec<Installable>, CommandError> {
    app::emulator_setup_installable(&state).await
}

/// Platforms with games that no emulator runs yet.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_missing(
    state: State<'_>,
) -> Result<Vec<MissingPlatform>, CommandError> {
    app::emulator_setup_missing(&state).await
}

/// Installs (or reinstalls) an emulator; progress arrives as
/// `emulationSetupChanged`.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_install(
    state: State<'_>,
    definition_id: String,
) -> Result<SetupJob, CommandError> {
    app::emulator_setup_install(&state, &definition_id).await
}

/// Installs the platform's recommended emulator.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_install_for(
    state: State<'_>,
    platform_id: String,
) -> Result<SetupJob, CommandError> {
    app::emulator_setup_install_for(&state, &platform_id).await
}

/// Installs since startup.
#[tauri::command]
#[specta::specta]
pub fn emulator_setup_jobs(state: State<'_>) -> Vec<SetupJob> {
    app::emulator_setup_jobs(&state)
}

#[tauri::command]
#[specta::specta]
pub fn emulator_setup_cancel(state: State<'_>, job_id: String) -> Result<SetupJob, CommandError> {
    app::emulator_setup_cancel(&state, &job_id)
}

/// Checks a Caskade-installed emulator for a newer version now.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_check(
    state: State<'_>,
    emulator_id: String,
) -> Result<ManagedEmulator, CommandError> {
    app::emulator_setup_check(&state, &emulator_id).await
}

/// Stops offering the newest version found.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_skip(
    state: State<'_>,
    emulator_id: String,
) -> Result<ManagedEmulator, CommandError> {
    app::emulator_setup_skip(&state, &emulator_id).await
}

/// Installs the newest version, keeping the user's files. Refused while a
/// game runs in the emulator.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_update(
    state: State<'_>,
    emulator_id: String,
) -> Result<SetupJob, CommandError> {
    app::emulator_setup_update(&state, &emulator_id).await
}

/// Fetches the platform's missing BIOS files from RomM into the folder its
/// default emulator reads.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_fetch_bios(
    state: State<'_>,
    platform_id: String,
) -> Result<BiosFetch, CommandError> {
    app::emulator_setup_fetch_bios(&state, &platform_id).await
}
