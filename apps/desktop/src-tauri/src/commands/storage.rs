//! Storage registry IPC. The source layer owns manifest adoption (#54).
use crate::error::{CommandError, report};
use caskade_core::storage::{CancellationToken, RootSummary, StorageService};
use caskade_sources::storage::StorageSource;

#[tauri::command]
#[specta::specta]
pub async fn storage_list(
    state: tauri::State<'_, StorageService>,
) -> Result<Vec<RootSummary>, CommandError> {
    state.list_roots().await.map_err(report)
}
#[tauri::command]
#[specta::specta]
pub async fn storage_add(
    state: tauri::State<'_, StorageService>,
    source: tauri::State<'_, StorageSource>,
    path: String,
    label: Option<String>,
) -> Result<RootSummary, CommandError> {
    let root = state
        .register_root(path.into(), label)
        .await
        .map_err(report)?;
    let adopted = source
        .adopt_root(&root.root, CancellationToken::new())
        .await
        .map_err(report)?;
    let mut summary = state
        .list_roots()
        .await
        .map_err(report)?
        .into_iter()
        .find(|r| r.root.id == root.root.id)
        .ok_or_else(|| caskade_core::tr!("The storage location was added, but it disappeared from the list before it could be shown. Reload Settings → Storage."))?;
    summary
        .capabilities
        .warnings
        .extend(adopted.warnings.into_iter().map(|w| w.message));
    if let Some(error) = adopted.error {
        summary.capabilities.warnings.push(error);
    }
    for game in adopted.games {
        if let Some(error) = game.error {
            summary.capabilities.warnings.push(error)
        }
    }
    Ok(summary)
}
#[tauri::command]
#[specta::specta]
pub async fn storage_edit(
    state: tauri::State<'_, StorageService>,
    id: String,
    label: Option<String>,
    folder_template: String,
) -> Result<RootSummary, CommandError> {
    state
        .edit_root(&id, label, folder_template)
        .await
        .map_err(report)
}
#[tauri::command]
#[specta::specta]
pub async fn storage_default(
    state: tauri::State<'_, StorageService>,
    id: String,
) -> Result<RootSummary, CommandError> {
    state.set_default(&id).await.map_err(report)
}
#[tauri::command]
#[specta::specta]
pub async fn storage_remove(
    state: tauri::State<'_, StorageService>,
    id: String,
) -> Result<(), CommandError> {
    state.remove_keep_files(&id).await.map_err(report)
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
