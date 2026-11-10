//! Storage registry IPC. The source layer owns manifest adoption (#54).
//! Forwarded to `caskade_app::commands::storage`, but for the folder picker.
use crate::error::CommandError;
use caskade_app::commands::storage as app;
use caskade_core::{
    accounts::AccountService,
    install::{InstallService, JobSnapshot},
    storage::{RootSummary, StorageService},
};
use caskade_sources::storage::StorageSource;

#[tauri::command]
#[specta::specta]
pub async fn storage_list(
    state: tauri::State<'_, StorageService>,
) -> Result<Vec<RootSummary>, CommandError> {
    app::storage_list(&state).await
}
#[tauri::command]
#[specta::specta]
pub async fn storage_add(
    state: tauri::State<'_, StorageService>,
    source: tauri::State<'_, StorageSource>,
    path: String,
    label: Option<String>,
) -> Result<RootSummary, CommandError> {
    app::storage_add(&state, &source, path, label).await
}
#[tauri::command]
#[specta::specta]
pub async fn storage_edit(
    state: tauri::State<'_, StorageService>,
    id: String,
    label: Option<String>,
    folder_template: String,
) -> Result<RootSummary, CommandError> {
    app::storage_edit(&state, &id, label, folder_template).await
}
#[tauri::command]
#[specta::specta]
pub async fn storage_default(
    state: tauri::State<'_, StorageService>,
    id: String,
) -> Result<RootSummary, CommandError> {
    app::storage_default(&state, &id).await
}
#[tauri::command]
#[specta::specta]
pub async fn storage_remove(
    state: tauri::State<'_, StorageService>,
    id: String,
) -> Result<(), CommandError> {
    app::storage_remove(&state, &id).await
}
/// Moves every game on location `id` to `target_id`, then removes `id`, as
/// one job (#64). The gate checks the removal; moving games needs installs
/// too, like `move_game`.
#[tauri::command]
#[specta::specta]
pub async fn storage_move_and_remove(
    accounts: tauri::State<'_, AccountService>,
    install: tauri::State<'_, InstallService>,
    storage: tauri::State<'_, StorageService>,
    id: String,
    target_id: String,
) -> Result<JobSnapshot, CommandError> {
    app::storage_move_and_remove(&accounts, &install, &storage, id, target_id).await
}
/// Directory-only native picker, no webview filesystem capability.
#[tauri::command]
#[specta::specta]
pub async fn storage_choose_directory<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, CommandError> {
    use tauri_plugin_dialog::DialogExt;
    let (send, receive) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(caskade_core::tr!("Choose game storage location"))
        .pick_folder(move |folder| {
            let _ = send.send(folder);
        });
    receive
        .await
        .map_err(|_| caskade_core::tr!("The folder chooser closed without an answer."))?
        .map(super::install_picker::native_path)
        .transpose()
        .map_err(CommandError::from)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
