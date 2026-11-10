//! Settings → Emulators → ROM scanners IPC (#119) over
//! `caskade_core::emulation::ScannerService`. Forwarded to
//! `caskade_app::commands::rom_scanners`.

use crate::error::CommandError;
use caskade_app::commands::rom_scanners as app;
use caskade_core::{
    db::LibraryStore,
    emulation::{
        EmulationService, ReviewChoice, RomFolderSetup, RomScanner, ScanJob, ScanMode, ScanSummary,
        ScannerView,
    },
};

type State<'a> = tauri::State<'a, EmulationService>;

/// Every scanner with its last scan, what it imports and any Flatpak hint.
#[tauri::command]
#[specta::specta]
pub async fn rom_scanners_list(
    state: State<'_>,
    store: tauri::State<'_, LibraryStore>,
) -> Result<Vec<ScannerView>, CommandError> {
    app::rom_scanners_list(&state, &store).await
}

/// Creates (empty id) or updates a scanner.
#[tauri::command]
#[specta::specta]
pub async fn rom_scanner_save(
    state: State<'_>,
    scanner: RomScanner,
) -> Result<RomScanner, CommandError> {
    app::rom_scanner_save(&state, scanner).await
}

/// Gives each platform folder in a ROM collection a scanner (first-run
/// setup). Nothing is scanned until `rom_scan_all`.
#[tauri::command]
#[specta::specta]
pub async fn rom_folder_setup(
    state: State<'_>,
    folder: String,
) -> Result<RomFolderSetup, CommandError> {
    app::rom_folder_setup(&state, &folder).await
}

/// Deletes a scanner; its games stay in the library, not installed.
#[tauri::command]
#[specta::specta]
pub async fn rom_scanner_delete(state: State<'_>, id: String) -> Result<(), CommandError> {
    app::rom_scanner_delete(&state, &id).await
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
    app::rom_scan_start(&state, &scanner_id, mode).await
}

#[tauri::command]
#[specta::specta]
pub fn rom_scan_get(state: State<'_>, id: String) -> Result<ScanJob, CommandError> {
    app::rom_scan_get(&state, &id)
}

/// Retained scans, newest first, so a review survives a reload.
#[tauri::command]
#[specta::specta]
pub fn rom_scan_jobs(state: State<'_>) -> Vec<ScanJob> {
    app::rom_scan_jobs(&state)
}

#[tauri::command]
#[specta::specta]
pub fn rom_scan_cancel(state: State<'_>, id: String) -> Result<ScanJob, CommandError> {
    app::rom_scan_cancel(&state, &id)
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
    app::rom_scan_submit(&state, &id, revision, choices).await
}

/// "Scan all": every scanner that takes part in library updates imports.
#[tauri::command]
#[specta::specta]
pub async fn rom_scan_all(state: State<'_>) -> Result<Vec<ScanSummary>, CommandError> {
    app::rom_scan_all(&state).await
}

/// Exclusions every scanner applies (#135).
#[tauri::command]
#[specta::specta]
pub async fn rom_exclusions(state: State<'_>) -> Result<Vec<String>, CommandError> {
    app::rom_exclusions(&state).await
}

/// Replaces the global exclusions and returns them as stored (trimmed,
/// without blanks or repeats); the next scan uses them.
#[tauri::command]
#[specta::specta]
pub async fn rom_exclusions_set(
    state: State<'_>,
    patterns: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    app::rom_exclusions_set(&state, patterns).await
}

#[cfg(test)]
#[path = "rom_scanners_tests.rs"]
mod tests;
