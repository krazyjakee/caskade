//! One-click emulator installs over
//! `caskade_core::emulation::EmulatorSetup`.
use caskade_core::emulation::{
    BiosFetch, EmulatorSetup, Installable, ManagedEmulator, MissingPlatform, SetupJob,
};

use crate::error::{CommandError, report};

/// Every emulator Caskade can install on this system.
pub async fn emulator_setup_installable(
    setup: &EmulatorSetup,
) -> Result<Vec<Installable>, CommandError> {
    setup.installable().await.map_err(report)
}

/// Platforms with games that no emulator runs yet.
pub async fn emulator_setup_missing(
    setup: &EmulatorSetup,
) -> Result<Vec<MissingPlatform>, CommandError> {
    setup.missing().await.map_err(report)
}

/// Installs (or reinstalls) an emulator; progress arrives as
/// `emulationSetupChanged`.
pub async fn emulator_setup_install(
    setup: &EmulatorSetup,
    definition_id: &str,
) -> Result<SetupJob, CommandError> {
    crate::permissions::enforce("emulator_setup_install", serde_json::Value::default)?;
    setup.install(definition_id).await.map_err(report)
}

/// Installs the platform's recommended emulator.
pub async fn emulator_setup_install_for(
    setup: &EmulatorSetup,
    platform_id: &str,
) -> Result<SetupJob, CommandError> {
    crate::permissions::enforce("emulator_setup_install_for", serde_json::Value::default)?;
    setup.install_for(platform_id).await.map_err(report)
}

/// Installs since startup.
pub fn emulator_setup_jobs(setup: &EmulatorSetup) -> Vec<SetupJob> {
    setup.jobs()
}

/// Cancels an install.
pub fn emulator_setup_cancel(
    setup: &EmulatorSetup,
    job_id: &str,
) -> Result<SetupJob, CommandError> {
    crate::permissions::enforce("emulator_setup_cancel", serde_json::Value::default)?;
    setup.cancel(job_id).map_err(report)
}

/// Checks a Caskade-installed emulator for a newer version now.
pub async fn emulator_setup_check(
    setup: &EmulatorSetup,
    emulator_id: &str,
) -> Result<ManagedEmulator, CommandError> {
    crate::permissions::enforce("emulator_setup_check", serde_json::Value::default)?;
    setup.check(emulator_id).await.map_err(report)
}

/// Stops offering the newest version found.
pub async fn emulator_setup_skip(
    setup: &EmulatorSetup,
    emulator_id: &str,
) -> Result<ManagedEmulator, CommandError> {
    crate::permissions::enforce("emulator_setup_skip", serde_json::Value::default)?;
    setup.skip(emulator_id, None).await.map_err(report)
}

/// Installs the newest version, keeping the user's files. Refused while a
/// game runs in the emulator.
pub async fn emulator_setup_update(
    setup: &EmulatorSetup,
    emulator_id: &str,
) -> Result<SetupJob, CommandError> {
    crate::permissions::enforce("emulator_setup_update", serde_json::Value::default)?;
    setup.update(emulator_id).await.map_err(report)
}

/// Fetches the platform's missing BIOS files from RomM into the folder its
/// default emulator reads.
pub async fn emulator_setup_fetch_bios(
    setup: &EmulatorSetup,
    platform_id: &str,
) -> Result<BiosFetch, CommandError> {
    crate::permissions::enforce("emulator_setup_fetch_bios", serde_json::Value::default)?;
    setup.fetch_bios(platform_id).await.map_err(report)
}
