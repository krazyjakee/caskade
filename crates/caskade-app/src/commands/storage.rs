//! Storage locations. The source layer owns manifest adoption (#54).
use caskade_core::{
    accounts::{AccountService, Permission},
    install::{InstallService, JobSnapshot},
    storage::{CancellationToken, RootSummary, StorageService},
};
use caskade_sources::storage::StorageSource;

use crate::error::{CommandError, report};

/// Every storage location.
pub async fn storage_list(storage: &StorageService) -> Result<Vec<RootSummary>, CommandError> {
    storage.list_roots().await.map_err(report)
}

/// Adds the folder at `path` as a storage location and adopts the games
/// already in it; what went wrong comes back as the location's warnings.
pub async fn storage_add(
    storage: &StorageService,
    source: &StorageSource,
    path: String,
    label: Option<String>,
) -> Result<RootSummary, CommandError> {
    crate::permissions::enforce("storage_add", serde_json::Value::default)?;
    let root = storage
        .register_root(path.into(), label)
        .await
        .map_err(report)?;
    let adopted = source
        .adopt_root(&root.root, CancellationToken::new())
        .await
        .map_err(report)?;
    let mut summary = storage
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

/// Changes a location's label and folder template.
pub async fn storage_edit(
    storage: &StorageService,
    id: &str,
    label: Option<String>,
    folder_template: String,
) -> Result<RootSummary, CommandError> {
    crate::permissions::enforce("storage_edit", serde_json::Value::default)?;
    storage
        .edit_root(id, label, folder_template)
        .await
        .map_err(report)
}

/// Makes a location the default for new installs.
pub async fn storage_default(
    storage: &StorageService,
    id: &str,
) -> Result<RootSummary, CommandError> {
    crate::permissions::enforce("storage_default", serde_json::Value::default)?;
    storage.set_default(id).await.map_err(report)
}

/// Removes a location, keeping its files.
pub async fn storage_remove(storage: &StorageService, id: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("storage_remove", serde_json::Value::default)?;
    storage.remove_keep_files(id).await.map_err(report)
}

/// Moves every game on location `id` to `target_id`, then removes `id`, as
/// one job (#64). Moving games needs installs, like `move_game`.
pub async fn storage_move_and_remove(
    accounts: &AccountService,
    install: &InstallService,
    storage: &StorageService,
    id: String,
    target_id: String,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("storage_move_and_remove", serde_json::Value::default)?;
    accounts.require(Permission::Install).map_err(report)?;
    install
        .move_location(id, target_id, storage.clone())
        .await
        .map_err(report)
}
