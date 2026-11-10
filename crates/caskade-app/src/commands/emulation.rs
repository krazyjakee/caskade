//! Settings → Emulators (#116): emulators, profiles and platform settings
//! over `caskade_core::emulation::EmulationService`.
use caskade_core::emulation::{
    ArgsOverride, ArgvPreview, DetectImport, DetectJob, EmulationCatalog, EmulationOverview,
    EmulationService, Emulator, EmulatorTarget, EmulatorView, GameEmulation, PlatformSettings,
    PlatformView, ProfileDraft,
};

use crate::error::{CommandError, conflict, refused, report};

/// Platforms, emulator definitions and the variables reference.
pub fn emulation_catalog(emulation: &EmulationService) -> EmulationCatalog {
    emulation.catalog_view()
}

/// The Settings → Emulators header stats.
pub async fn emulation_overview(
    emulation: &EmulationService,
) -> Result<EmulationOverview, CommandError> {
    emulation.overview().await.map_err(report)
}

/// Every emulator, with its profiles.
pub async fn emulators_list(
    emulation: &EmulationService,
) -> Result<Vec<EmulatorView>, CommandError> {
    emulation.emulators().await.map_err(report)
}

/// Creates (empty id) or updates an emulator and its profiles.
pub async fn emulator_save(
    emulation: &EmulationService,
    emulator: Emulator,
) -> Result<EmulatorView, CommandError> {
    emulation.save_emulator(emulator).await.map_err(report)
}

/// Deletes an emulator; a conflict when it's already gone.
pub async fn emulator_delete(emulation: &EmulationService, id: &str) -> Result<(), CommandError> {
    match emulation.delete_emulator(id).await.map_err(report)? {
        true => Ok(()),
        false => Err(conflict(caskade_core::tr!(
            "This emulator no longer exists."
        ))),
    }
}

/// The command line of a profile being edited, before it is saved.
pub fn profile_preview(emulation: &EmulationService, draft: &ProfileDraft) -> ArgvPreview {
    emulation.preview(draft)
}

/// Splits arguments typed as one string with shell-word rules.
pub fn args_split(text: &str) -> Result<Vec<String>, CommandError> {
    EmulationService::split_args(text).map_err(report)
}

/// Every platform, with its settings.
pub async fn emulation_platforms(
    emulation: &EmulationService,
) -> Result<Vec<PlatformView>, CommandError> {
    emulation.platforms().await.map_err(report)
}

/// Changes a platform's settings.
pub async fn platform_settings_set(
    emulation: &EmulationService,
    settings: PlatformSettings,
) -> Result<PlatformView, CommandError> {
    emulation
        .set_platform_settings(settings)
        .await
        .map_err(report)
}

/// Starts detecting emulators in standard locations and, if given, a
/// folder the user chose (an absolute path). Results arrive as
/// `emulationDetectChanged`.
pub async fn emulation_detect_start(
    emulation: &EmulationService,
    folder: Option<String>,
) -> Result<DetectJob, CommandError> {
    let folder = folder.map(std::path::PathBuf::from);
    if folder
        .as_deref()
        .is_some_and(|folder| !folder.is_absolute())
    {
        return Err(refused(caskade_core::tr!("choose a folder")));
    }
    emulation.detect_start(folder).await.map_err(report)
}

/// A detection, while it's kept.
pub fn emulation_detect_get(
    emulation: &EmulationService,
    id: &str,
) -> Result<DetectJob, CommandError> {
    emulation.detect_get(id).ok_or_else(|| {
        conflict(caskade_core::tr!(
            "This detection is no longer available. Run it again."
        ))
    })
}

/// The most recent detection, e.g. the one the last library scan ran.
pub fn emulation_detect_latest(emulation: &EmulationService) -> Option<DetectJob> {
    emulation.detect_latest()
}

/// Cancels a detection.
pub fn emulation_detect_cancel(
    emulation: &EmulationService,
    id: &str,
) -> Result<DetectJob, CommandError> {
    emulation.detect_cancel(id).map_err(report)
}

/// Imports the ticked results of the reviewed revision of a detection.
pub async fn emulation_detect_import(
    emulation: &EmulationService,
    id: &str,
    revision: u32,
    keys: &[String],
) -> Result<DetectImport, CommandError> {
    emulation
        .detect_import(id, revision, keys)
        .await
        .map_err(report)
}

/// An emulated game's emulator, discs, BIOS and whether it can launch.
pub async fn emulation_game(
    emulation: &EmulationService,
    entry_id: &str,
) -> Result<GameEmulation, CommandError> {
    emulation.game(entry_id).await.map_err(report)
}

/// Pins, un-pins or asks for a game's emulator, and sets its arguments.
pub async fn emulation_game_set(
    emulation: &EmulationService,
    entry_id: &str,
    target: EmulatorTarget,
    args: ArgsOverride,
) -> Result<GameEmulation, CommandError> {
    emulation
        .set_game(entry_id, target, args)
        .await
        .map_err(report)
}
