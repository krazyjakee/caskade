//! One-click emulator installs (EMULATOR-SETUP-PLAN §7) over
//! `caskade_core::emulation::EmulatorSetup`.

use crate::error::{CommandError, report};
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
    state.installable().await.map_err(report)
}

/// Platforms with games that no emulator runs yet.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_missing(
    state: State<'_>,
) -> Result<Vec<MissingPlatform>, CommandError> {
    state.missing().await.map_err(report)
}

/// Installs (or reinstalls) an emulator; progress arrives as
/// `emulationSetupChanged`.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_install(
    state: State<'_>,
    definition_id: String,
) -> Result<SetupJob, CommandError> {
    state.install(&definition_id).await.map_err(report)
}

/// Installs the platform's recommended emulator.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_install_for(
    state: State<'_>,
    platform_id: String,
) -> Result<SetupJob, CommandError> {
    state.install_for(&platform_id).await.map_err(report)
}

/// Installs since startup.
#[tauri::command]
#[specta::specta]
pub fn emulator_setup_jobs(state: State<'_>) -> Vec<SetupJob> {
    state.jobs()
}

#[tauri::command]
#[specta::specta]
pub fn emulator_setup_cancel(state: State<'_>, job_id: String) -> Result<SetupJob, CommandError> {
    state.cancel(&job_id).map_err(report)
}

/// Checks a Caskade-installed emulator for a newer version now.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_check(
    state: State<'_>,
    emulator_id: String,
) -> Result<ManagedEmulator, CommandError> {
    state.check(&emulator_id).await.map_err(report)
}

/// Stops offering the newest version found.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_skip(
    state: State<'_>,
    emulator_id: String,
) -> Result<ManagedEmulator, CommandError> {
    state.skip(&emulator_id, None).await.map_err(report)
}

/// Installs the newest version, keeping the user's files. Refused while a
/// game runs in the emulator.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_update(
    state: State<'_>,
    emulator_id: String,
) -> Result<SetupJob, CommandError> {
    state.update(&emulator_id).await.map_err(report)
}

/// Fetches the platform's missing BIOS files from RomM into the folder its
/// default emulator reads.
#[tauri::command]
#[specta::specta]
pub async fn emulator_setup_fetch_bios(
    state: State<'_>,
    platform_id: String,
) -> Result<BiosFetch, CommandError> {
    state.fetch_bios(&platform_id).await.map_err(report)
}
