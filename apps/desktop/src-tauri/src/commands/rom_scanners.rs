//! Settings → Emulators → ROM scanners IPC (#119) over
//! `caskade_core::emulation::ScannerService`.

use crate::error::{CommandError, conflict, report};
use caskade_core::{
    db::LibraryStore,
    emulation::{
        EmulationService, ReviewChoice, RomFolderSetup, RomScanner, ScanJob, ScanMode, ScanSummary,
        ScannerView,
    },
    launch::EmulatorLaunches,
};

type State<'a> = tauri::State<'a, EmulationService>;

fn no_scan() -> String {
    caskade_core::tr!("This ROM scan is no longer available.")
}

/// Every scanner with its last scan, what it imports and any Flatpak hint.
#[tauri::command]
#[specta::specta]
pub async fn rom_scanners_list(
    state: State<'_>,
    store: tauri::State<'_, LibraryStore>,
) -> Result<Vec<ScannerView>, CommandError> {
    let launches = EmulatorLaunches::new(store.inner().clone(), state.inner().clone());
    state
        .scanners()
        .list(&|app_id, dir| launches.flatpak_sees(app_id, dir))
        .await
        .map_err(report)
}

/// Creates (empty id) or updates a scanner.
#[tauri::command]
#[specta::specta]
pub async fn rom_scanner_save(
    state: State<'_>,
    scanner: RomScanner,
) -> Result<RomScanner, CommandError> {
    state.scanners().save(scanner).await.map_err(report)
}

/// Gives each platform folder in a ROM collection a scanner (first-run
/// setup). Nothing is scanned until `rom_scan_all`.
#[tauri::command]
#[specta::specta]
pub async fn rom_folder_setup(
    state: State<'_>,
    folder: String,
) -> Result<RomFolderSetup, CommandError> {
    state
        .scanners()
        .setup_folder(std::path::Path::new(&folder))
        .await
        .map_err(report)
}

/// Deletes a scanner; its games stay in the library, not installed.
#[tauri::command]
#[specta::specta]
pub async fn rom_scanner_delete(state: State<'_>, id: String) -> Result<(), CommandError> {
    match state.scanners().delete(&id).await.map_err(report)? {
        true => Ok(()),
        false => Err(conflict(caskade_core::tr!(
            "This ROM scanner no longer exists."
        ))),
    }
}

/// Starts a scan: `review` stops at a review list, `import` imports.
/// Changes arrive as `emulationScanChanged`.
#[tauri::command]
#[specta::specta]
pub async fn rom_scan_start(
    state: State<'_>,
    scanner_id: String,
    mode: ScanMode,
) -> Result<ScanJob, CommandError> {
    state
        .scanners()
        .start(&scanner_id, mode)
        .await
        .map_err(report)
}

#[tauri::command]
#[specta::specta]
pub fn rom_scan_get(state: State<'_>, id: String) -> Result<ScanJob, CommandError> {
    state.scanners().get(&id).ok_or_else(|| conflict(no_scan()))
}

/// Retained scans, newest first, so a review survives a reload.
#[tauri::command]
#[specta::specta]
pub fn rom_scan_jobs(state: State<'_>) -> Vec<ScanJob> {
    state.scanners().jobs()
}

#[tauri::command]
#[specta::specta]
pub fn rom_scan_cancel(state: State<'_>, id: String) -> Result<ScanJob, CommandError> {
    state
        .scanners()
        .cancel(&id)
        .ok_or_else(|| conflict(no_scan()))
}

/// Stores the choices of the reviewed revision and imports with them.
#[tauri::command]
#[specta::specta]
pub async fn rom_scan_submit(
    state: State<'_>,
    id: String,
    revision: u32,
    choices: Vec<ReviewChoice>,
) -> Result<ScanJob, CommandError> {
    state
        .scanners()
        .submit(&id, revision, choices)
        .await
        .map_err(report)
}

/// "Scan all": every scanner that takes part in library updates imports.
#[tauri::command]
#[specta::specta]
pub async fn rom_scan_all(state: State<'_>) -> Result<Vec<ScanSummary>, CommandError> {
    state.scanners().scan_all().await.map_err(report)
}

/// Exclusions every scanner applies (#135).
#[tauri::command]
#[specta::specta]
pub async fn rom_exclusions(state: State<'_>) -> Result<Vec<String>, CommandError> {
    state.scanners().global_exclusions().await.map_err(report)
}

/// Replaces the global exclusions and returns them as stored (trimmed,
/// without blanks or repeats); the next scan uses them.
#[tauri::command]
#[specta::specta]
pub async fn rom_exclusions_set(
    state: State<'_>,
    patterns: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    state
        .scanners()
        .set_global_exclusions(patterns)
        .await
        .map_err(report)
}

#[cfg(test)]
#[path = "rom_scanners_tests.rs"]
mod tests;
