//! Settings → Emulators → ROM scanners (#119) over
//! `caskade_core::emulation::ScannerService`.
use caskade_core::{
    db::LibraryStore,
    emulation::{
        EmulationService, ReviewChoice, RomFolderSetup, RomScanner, ScanJob, ScanMode, ScanSummary,
        ScannerView,
    },
    launch::EmulatorLaunches,
};

use crate::error::{CommandError, conflict, report};

fn no_scan() -> String {
    caskade_core::tr!("This ROM scan is no longer available.")
}

/// Every scanner with its last scan, what it imports and any Flatpak hint.
pub async fn rom_scanners_list(
    emulation: &EmulationService,
    store: &LibraryStore,
) -> Result<Vec<ScannerView>, CommandError> {
    let launches = EmulatorLaunches::new(store.clone(), emulation.clone());
    emulation
        .scanners()
        .list(&|app_id, dir| launches.flatpak_sees(app_id, dir))
        .await
        .map_err(report)
}

/// Creates (empty id) or updates a scanner.
pub async fn rom_scanner_save(
    emulation: &EmulationService,
    scanner: RomScanner,
) -> Result<RomScanner, CommandError> {
    crate::permissions::enforce("rom_scanner_save", serde_json::Value::default)?;
    emulation.scanners().save(scanner).await.map_err(report)
}

/// Gives each platform folder in a ROM collection a scanner (first-run
/// setup). Nothing is scanned until [`rom_scan_all`].
pub async fn rom_folder_setup(
    emulation: &EmulationService,
    folder: &str,
) -> Result<RomFolderSetup, CommandError> {
    crate::permissions::enforce("rom_folder_setup", serde_json::Value::default)?;
    emulation
        .scanners()
        .setup_folder(std::path::Path::new(folder))
        .await
        .map_err(report)
}

/// Deletes a scanner; its games stay in the library, not installed.
pub async fn rom_scanner_delete(
    emulation: &EmulationService,
    id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("rom_scanner_delete", serde_json::Value::default)?;
    match emulation.scanners().delete(id).await.map_err(report)? {
        true => Ok(()),
        false => Err(conflict(caskade_core::tr!(
            "This ROM scanner no longer exists."
        ))),
    }
}

/// Starts a scan: `review` stops at a review list, `import` imports.
/// Changes arrive as `emulationScanChanged`.
pub async fn rom_scan_start(
    emulation: &EmulationService,
    scanner_id: &str,
    mode: ScanMode,
) -> Result<ScanJob, CommandError> {
    crate::permissions::enforce("rom_scan_start", serde_json::Value::default)?;
    emulation
        .scanners()
        .start(scanner_id, mode)
        .await
        .map_err(report)
}

/// Retained scans, newest first, so a review survives a reload.
pub fn rom_scan_jobs(emulation: &EmulationService) -> Vec<ScanJob> {
    emulation.scanners().jobs()
}

/// Cancels a scan.
pub fn rom_scan_cancel(emulation: &EmulationService, id: &str) -> Result<ScanJob, CommandError> {
    crate::permissions::enforce("rom_scan_cancel", serde_json::Value::default)?;
    emulation
        .scanners()
        .cancel(id)
        .ok_or_else(|| conflict(no_scan()))
}

/// Stores the choices of the reviewed revision and imports with them.
pub async fn rom_scan_submit(
    emulation: &EmulationService,
    id: &str,
    revision: u32,
    choices: Vec<ReviewChoice>,
) -> Result<ScanJob, CommandError> {
    crate::permissions::enforce("rom_scan_submit", serde_json::Value::default)?;
    emulation
        .scanners()
        .submit(id, revision, choices)
        .await
        .map_err(report)
}

/// "Scan all": every scanner that takes part in library updates imports.
pub async fn rom_scan_all(emulation: &EmulationService) -> Result<Vec<ScanSummary>, CommandError> {
    crate::permissions::enforce("rom_scan_all", serde_json::Value::default)?;
    emulation.scanners().scan_all().await.map_err(report)
}

/// Exclusions every scanner applies (#135).
pub async fn rom_exclusions(emulation: &EmulationService) -> Result<Vec<String>, CommandError> {
    emulation
        .scanners()
        .global_exclusions()
        .await
        .map_err(report)
}

/// Replaces the global exclusions and returns them as stored (trimmed,
/// without blanks or repeats); the next scan uses them.
pub async fn rom_exclusions_set(
    emulation: &EmulationService,
    patterns: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    crate::permissions::enforce("rom_exclusions_set", serde_json::Value::default)?;
    emulation
        .scanners()
        .set_global_exclusions(patterns)
        .await
        .map_err(report)
}
