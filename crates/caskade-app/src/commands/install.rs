//! Stored jobs and native input selection. Execution stays in caskade-core.
pub use super::install_picker::InstallPicker;
use crate::error::{CommandError, conflict, refused, report};
use caskade_core::{
    install::{InstallService, JobAnswer, JobSnapshot},
    launch::LaunchService,
    storage::StorageService,
};
pub async fn install_start(
    state: &InstallService,
    input: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("install_start", serde_json::Value::default)?;
    if input.trim().is_empty() || input.len() > 32_768 || input.contains('\0') {
        return Err(refused(caskade_core::tr!(
            "input must be a nonempty path or URL"
        )));
    }
    state.install(input, root_id).await.map_err(report)
}

pub fn jobs_list(state: &InstallService) -> Vec<JobSnapshot> {
    state.jobs()
}

pub fn job_get(state: &InstallService, id: String) -> Result<JobSnapshot, CommandError> {
    state
        .job(&id)
        .ok_or_else(|| conflict(caskade_core::tr!("This job is no longer in Activity.")))
}

pub fn job_cancel(state: &InstallService, id: String) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("job_cancel", serde_json::Value::default)?;
    state.cancel(&id).map_err(report)
}

/// Pauses an install or update while it downloads: it stops, keeps what
/// it downloaded, and stays paused across restarts until resumed (#131).
pub async fn job_pause(state: &InstallService, id: String) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("job_pause", serde_json::Value::default)?;
    state.pause(&id).await.map_err(report)
}

/// Resumes a paused job from where its download stopped.
pub async fn job_resume(state: &InstallService, id: String) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("job_resume", || serde_json::json!({ "id": id }))?;
    state.resume(&id).await.map_err(report)
}

/// Takes a finished job out of Activity. Its files stay.
pub fn job_remove(state: &InstallService, id: String) -> Result<(), CommandError> {
    crate::permissions::enforce("job_remove", serde_json::Value::default)?;
    state.remove(&id).map_err(report)
}

pub async fn choose_install_files(picker: &InstallPicker) -> Result<Vec<String>, CommandError> {
    (picker.files)().await.map_err(CommandError::from)
}

pub async fn choose_install_folder(picker: &InstallPicker) -> Result<Option<String>, CommandError> {
    (picker.folder)().await.map_err(CommandError::from)
}

pub async fn job_answer(
    state: &InstallService,
    id: String,
    revision: u32,
    answer: JobAnswer,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("job_answer", || serde_json::json!({ "id": id }))?;
    state.answer(&id, revision, answer).await.map_err(report)
}

/// Runs a stopped job again: with the alternative at `candidate_index`, or
/// without one as it ran before.
pub async fn job_retry(
    state: &InstallService,
    id: String,
    candidate_index: Option<u32>,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("job_retry", || serde_json::json!({ "id": id }))?;
    state.retry(&id, candidate_index).await.map_err(report)
}

/// Recovers a failed or cancelled install or update (#158): run it again,
/// download again, install from another source or into another root.
pub async fn job_recover(
    state: &InstallService,
    id: String,
    recovery: caskade_core::install::Recovery,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("job_recover", || serde_json::json!({ "id": id }))?;
    state.recover(&id, recovery).await.map_err(report)
}

pub async fn uninstall_game(
    state: &InstallService,
    launch: &LaunchService,
    entry_id: String,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("uninstall_game", serde_json::Value::default)?;
    state
        .uninstall(entry_id, launch.clone())
        .await
        .map_err(report)
}

/// Removes a game Caskade installed or handed off from the library for
/// good, uninstalling its files first. Returns the uninstall job when there
/// are files, which removes the game once it completes.
pub async fn remove_game(
    state: &InstallService,
    launch: &LaunchService,
    game_id: String,
) -> Result<Option<JobSnapshot>, CommandError> {
    crate::permissions::enforce("remove_game", serde_json::Value::default)?;
    state
        .remove_game(&game_id, launch.clone())
        .await
        .map_err(report)
}

pub async fn move_game(
    state: &InstallService,
    storage: &StorageService,
    entry_id: String,
    target_root_id: String,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("move_game", serde_json::Value::default)?;
    state
        .move_game(entry_id, target_root_id, storage.clone())
        .await
        .map_err(report)
}

/// The 7-Zip sidecar next to Caskade's executable.
pub fn sidecar() -> std::io::Result<std::path::PathBuf> {
    std::env::current_exe()
        .map(|exe| exe.with_file_name(if cfg!(windows) { "7zz.exe" } else { "7zz" }))
}

/// The install service, with `sidecar` for archives that need 7-Zip, and
/// the storage source that repairs and remounts installs; the source is
/// registered on the bus. Needs the storage locations.
pub fn services(
    store: caskade_core::db::LibraryStore,
    bus: caskade_core::bus::Bus,
    paths: &caskade_core::AppPaths,
    settings: caskade_core::settings::SettingsService,
    storage: Option<&StorageService>,
    sidecar: std::io::Result<std::path::PathBuf>,
) -> caskade_core::Result<(caskade_sources::storage::StorageSource, InstallService)> {
    let storage = storage
        .ok_or_else(|| {
            caskade_core::Error::Invalid(caskade_core::tr!("storage locations unavailable"))
        })?
        .clone();
    let repair = storage
        .reconciler()
        .map_err(|e| caskade_core::Error::Invalid(e.to_string()))?;
    let source = caskade_sources::storage::StorageSource::new(store.clone(), bus.clone(), repair)
        .with_remounts(storage.clone(), caskade_discovery::HostEnv::live(), paths);
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
    crate::runtime::block_on(async {
        service.watch_first_exits();
    });
    Ok((source, service))
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
