//! Stored jobs and native input selection. Execution stays in caskade-core.
pub use super::install_picker::InstallPicker;
pub use app::*;
use caskade_app::commands::install as app;
use caskade_core::{
    install::{InstallService, JobAnswer, JobSnapshot},
    launch::LaunchService,
    storage::StorageService,
};
/// The install service (`caskade_app`'s) and what it works with, managed,
/// as the app's start does (tests).
#[cfg(test)]
pub(crate) fn setup_with_sidecar<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: caskade_core::db::LibraryStore,
    bus: caskade_core::bus::Bus,
    paths: &caskade_core::AppPaths,
    settings: caskade_core::settings::SettingsService,
    sidecar: std::io::Result<std::path::PathBuf>,
) -> caskade_core::Result<()> {
    use tauri::Manager;
    let storage = app.try_state::<StorageService>();
    let (source, service) =
        app::services(store, bus, paths, settings, storage.as_deref(), sidecar)?;
    app.manage(source);
    app.manage(super::install_picker::native(app.clone()));
    app.manage(service);
    app.manage(super::install_requests::InstallRequests::default());
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn install_start(
    state: tauri::State<'_, InstallService>,
    input: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, String> {
    app::install_start(&state, input, root_id).await
}

#[tauri::command]
#[specta::specta]
pub fn jobs_list(state: tauri::State<'_, InstallService>) -> Vec<JobSnapshot> {
    app::jobs_list(&state)
}

#[tauri::command]
#[specta::specta]
pub fn job_get(state: tauri::State<'_, InstallService>, id: String) -> Result<JobSnapshot, String> {
    app::job_get(&state, id)
}

#[tauri::command]
#[specta::specta]
pub fn job_cancel(
    state: tauri::State<'_, InstallService>,
    id: String,
) -> Result<JobSnapshot, String> {
    app::job_cancel(&state, id)
}

/// Pauses an install or update while it downloads: it stops, keeps what
/// it downloaded, and stays paused across restarts until resumed (#131).
#[tauri::command]
#[specta::specta]
pub async fn job_pause(
    state: tauri::State<'_, InstallService>,
    id: String,
) -> Result<JobSnapshot, String> {
    app::job_pause(&state, id).await
}

/// Resumes a paused job from where its download stopped.
#[tauri::command]
#[specta::specta]
pub async fn job_resume(
    state: tauri::State<'_, InstallService>,
    id: String,
) -> Result<JobSnapshot, String> {
    app::job_resume(&state, id).await
}

/// Takes a finished job out of Activity. Its files stay.
#[tauri::command]
#[specta::specta]
pub fn job_remove(state: tauri::State<'_, InstallService>, id: String) -> Result<(), String> {
    app::job_remove(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn choose_install_files(
    picker: tauri::State<'_, InstallPicker>,
) -> Result<Vec<String>, String> {
    app::choose_install_files(&picker).await
}

#[tauri::command]
#[specta::specta]
pub async fn choose_install_folder(
    picker: tauri::State<'_, InstallPicker>,
) -> Result<Option<String>, String> {
    app::choose_install_folder(&picker).await
}

#[tauri::command]
#[specta::specta]
pub async fn job_answer(
    state: tauri::State<'_, InstallService>,
    id: String,
    revision: u32,
    answer: JobAnswer,
) -> Result<JobSnapshot, String> {
    app::job_answer(&state, id, revision, answer).await
}

#[tauri::command]
#[specta::specta]
/// Runs a stopped job again: with the alternative at `candidate_index`, or
/// without one as it ran before.
pub async fn job_retry(
    state: tauri::State<'_, InstallService>,
    id: String,
    candidate_index: Option<u32>,
) -> Result<JobSnapshot, String> {
    app::job_retry(&state, id, candidate_index).await
}

#[tauri::command]
#[specta::specta]
/// Recovers a failed or cancelled install or update (#158): run it again,
/// download again, install from another source or into another root.
pub async fn job_recover(
    state: tauri::State<'_, InstallService>,
    id: String,
    recovery: caskade_core::install::Recovery,
) -> Result<JobSnapshot, String> {
    app::job_recover(&state, id, recovery).await
}

#[tauri::command]
#[specta::specta]
pub async fn uninstall_game(
    state: tauri::State<'_, InstallService>,
    launch: tauri::State<'_, LaunchService>,
    entry_id: String,
) -> Result<JobSnapshot, String> {
    app::uninstall_game(&state, &launch, entry_id).await
}

/// Removes a game Caskade installed or handed off from the library for
/// good, uninstalling its files first. Returns the uninstall job when there
/// are files, which removes the game once it completes.
#[tauri::command]
#[specta::specta]
pub async fn remove_game(
    state: tauri::State<'_, InstallService>,
    launch: tauri::State<'_, LaunchService>,
    game_id: String,
) -> Result<Option<JobSnapshot>, crate::error::CommandError> {
    app::remove_game(&state, &launch, game_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn move_game(
    state: tauri::State<'_, InstallService>,
    storage: tauri::State<'_, StorageService>,
    entry_id: String,
    target_root_id: String,
) -> Result<JobSnapshot, String> {
    app::move_game(&state, &storage, entry_id, target_root_id).await
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
