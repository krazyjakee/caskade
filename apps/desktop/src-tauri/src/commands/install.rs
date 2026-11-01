//! Stored jobs and native input selection. Execution stays in caskade-core.
pub use super::install_picker::InstallPicker;
use caskade_core::{
    install::{InstallService, JobAnswer, JobSnapshot},
    launch::LaunchService,
    storage::StorageService,
};

/// The bundled utility is configured only at the native boundary, never from
/// downloaded metadata or a webview-supplied executable path. Without it
/// (missing, quarantined, not the pinned build) installs still work, except
/// for the archive formats only 7-Zip reads.
pub(crate) fn setup<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: caskade_core::db::LibraryStore,
    bus: caskade_core::bus::Bus,
    paths: &caskade_core::AppPaths,
    settings: caskade_core::settings::SettingsService,
) -> caskade_core::Result<()> {
    let sidecar = std::env::current_exe()
        .map(|exe| exe.with_file_name(if cfg!(windows) { "7zz.exe" } else { "7zz" }));
    setup_with_sidecar(app, store, bus, paths, settings, sidecar)
}

pub(crate) fn setup_with_sidecar<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: caskade_core::db::LibraryStore,
    bus: caskade_core::bus::Bus,
    paths: &caskade_core::AppPaths,
    settings: caskade_core::settings::SettingsService,
    sidecar: std::io::Result<std::path::PathBuf>,
) -> caskade_core::Result<()> {
    use tauri::Manager;
    let storage = app
        .try_state::<StorageService>()
        .ok_or_else(|| {
            caskade_core::Error::Invalid(caskade_core::tr!("storage locations unavailable"))
        })?
        .inner()
        .clone();
    let repair = storage
        .reconciler()
        .map_err(|e| caskade_core::Error::Invalid(e.to_string()))?;
    let source = caskade_sources::storage::StorageSource::new(store.clone(), bus.clone(), repair)
        .with_remounts(storage.clone(), caskade_discovery::HostEnv::live(), paths);
    source.register();
    app.manage(source);
    let service = InstallService::new(store, bus, caskade_core::install::InstallOptions::default());
    let service = match sidecar {
        Ok(path) => match service.clone().with_sidecar(path) {
            Ok(service) => service,
            Err(error) => {
                tracing::error!(%error, "7-Zip sidecar unusable; archives that need 7-Zip can't be installed");
                service
            }
        },
        Err(error) => {
            tracing::error!(%error, "7-Zip sidecar not found; archives that need 7-Zip can't be installed");
            service
        }
    }
    .with_storage(storage)
    .with_settings(settings);
    tauri::async_runtime::block_on(async {
        service.watch_first_exits();
    });
    app.manage(InstallPicker::native(app.clone()));
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
    if input.trim().is_empty() || input.len() > 32_768 || input.contains('\0') {
        return Err(caskade_core::tr!("input must be a nonempty path or URL"));
    }
    state
        .install(input, root_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn jobs_list(state: tauri::State<'_, InstallService>) -> Vec<JobSnapshot> {
    state.jobs()
}

#[tauri::command]
#[specta::specta]
pub fn job_get(state: tauri::State<'_, InstallService>, id: String) -> Result<JobSnapshot, String> {
    state
        .job(&id)
        .ok_or_else(|| caskade_core::tr!("unknown job"))
}

#[tauri::command]
#[specta::specta]
pub fn job_cancel(
    state: tauri::State<'_, InstallService>,
    id: String,
) -> Result<JobSnapshot, String> {
    state.cancel(&id).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn choose_install_files(
    picker: tauri::State<'_, InstallPicker>,
) -> Result<Vec<String>, String> {
    (picker.files)().await
}

#[tauri::command]
#[specta::specta]
pub async fn choose_install_folder(
    picker: tauri::State<'_, InstallPicker>,
) -> Result<Option<String>, String> {
    (picker.folder)().await
}

#[tauri::command]
#[specta::specta]
pub async fn job_answer(
    state: tauri::State<'_, InstallService>,
    id: String,
    revision: u32,
    answer: JobAnswer,
) -> Result<JobSnapshot, String> {
    state
        .answer(&id, revision, answer)
        .await
        .map_err(|e| e.to_string())
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
    state
        .retry(&id, candidate_index)
        .await
        .map_err(|e| e.to_string())
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
    state
        .recover(&id, recovery)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
#[specta::specta]
pub async fn fetch_start(
    state: tauri::State<'_, InstallService>,
    input: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, String> {
    state.fetch(input, root_id).await.map_err(|e| e.to_string())
}
#[tauri::command]
#[specta::specta]
pub async fn uninstall_game(
    state: tauri::State<'_, InstallService>,
    launch: tauri::State<'_, LaunchService>,
    entry_id: String,
) -> Result<JobSnapshot, String> {
    state
        .uninstall(entry_id, launch.inner().clone())
        .await
        .map_err(|e| e.to_string())
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
    state
        .remove_game(&game_id, launch.inner().clone())
        .await
        .map_err(crate::error::report)
}
#[tauri::command]
#[specta::specta]
pub async fn verify_game(
    state: tauri::State<'_, InstallService>,
    entry_id: String,
) -> Result<JobSnapshot, String> {
    state.verify(entry_id).await.map_err(|e| e.to_string())
}
#[tauri::command]
#[specta::specta]
pub async fn move_game(
    state: tauri::State<'_, InstallService>,
    storage: tauri::State<'_, StorageService>,
    entry_id: String,
    target_root_id: String,
) -> Result<JobSnapshot, String> {
    state
        .move_game(entry_id, target_root_id, storage.inner().clone())
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
