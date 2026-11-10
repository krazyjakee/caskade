//! Settings → Backup (#193): Back up now, checking and restoring a backup,
//! and the restart that applies a restore. The status follows
//! `BackupChanged`; automatic backups run in the core's watch.
pub use app::*;
use caskade_app::commands::backup as app;

use crate::error::CommandError;
use caskade_backup::{BackupPlan, BackupStatus, Backups, Component, Inspection, Journal};
use caskade_core::{install::InstallService, launch::LaunchService};
use tauri::{AppHandle, Manager, Runtime, State};
#[tauri::command]
#[specta::specta]
pub async fn backup_status(backups: State<'_, Backups>) -> Result<BackupStatus, CommandError> {
    app::backup_status(&backups).await
}

/// What a backup to `destination` (the settings' folder when `None`) with
/// `components` would hold; writes nothing.
#[tauri::command]
#[specta::specta]
pub async fn backup_plan(
    backups: State<'_, Backups>,
    destination: Option<String>,
    components: Vec<Component>,
) -> Result<BackupPlan, CommandError> {
    app::backup_plan(&backups, destination, components).await
}

/// Back up now, with the settings' folder and components. Returns once it
/// starts; `BackupChanged` follows it to its outcome.
#[tauri::command]
#[specta::specta]
pub async fn backup_start(backups: State<'_, Backups>) -> Result<(), CommandError> {
    app::backup_start(&backups).await
}

/// Cancels the backup, check or restore under way.
#[tauri::command]
#[specta::specta]
pub fn backup_cancel(backups: State<'_, Backups>) {
    app::backup_cancel(&backups)
}

/// Checks a backup and says what restoring it would do.
#[tauri::command]
#[specta::specta]
pub async fn backup_inspect(
    backups: State<'_, Backups>,
    path: String,
) -> Result<Inspection, CommandError> {
    app::backup_inspect(&backups, path).await
}

/// Stages a restore of `components` (the library always) from the backup
/// at `path`; [`backup_restart`] applies it.
#[tauri::command]
#[specta::specta]
pub async fn backup_restore(
    backups: State<'_, Backups>,
    path: String,
    components: Vec<Component>,
) -> Result<Journal, CommandError> {
    app::backup_restore(&backups, path, components).await
}

/// Forgets the restore waiting for the restart.
#[tauri::command]
#[specta::specta]
pub async fn backup_restore_discard(backups: State<'_, Backups>) -> Result<(), CommandError> {
    app::backup_restore_discard(&backups).await
}

/// Restarts Caskade to apply the staged restore. Refused while a game
/// runs or an install, move or update is under way: they'd be cut off,
/// and the restored library wouldn't know them.
#[tauri::command]
#[specta::specta]
pub async fn backup_restart<R: Runtime>(
    app: AppHandle<R>,
    backups: State<'_, Backups>,
) -> Result<(), CommandError> {
    app::backup_restart_ready(
        &backups,
        app.try_state::<LaunchService>().as_deref(),
        app.try_state::<InstallService>().as_deref(),
    )
    .await?;
    crate::tray::restart(&app);
    Ok(())
}

/// Shows a backup in the file manager: only a file named like one, in the
/// backup folder or the last backup's.
#[tauri::command]
#[specta::specta]
pub async fn backup_reveal(backups: State<'_, Backups>, path: String) -> Result<(), CommandError> {
    app::backup_reveal(&backups, path).await
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
