//! Settings → Backup (#193): Back up now, checking and restoring a backup,
//! and the restart that applies a restore. The status follows
//! `BackupChanged`; automatic backups run in the core's watch.

use crate::error::{CommandError, ErrorCode, report};
use caskade_backup::{
    BackupPlan, BackupStatus, Backups, Component, ExportOptions, Inspection, Journal,
};
use caskade_core::{install::InstallService, launch::LaunchService};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

/// The backup status changed: a job's progress, its outcome, a staged
/// restore, the schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct BackupChanged(pub BackupStatus);

/// Starts the backups' watch and forwards their status to the webview.
pub fn setup<R: Runtime>(app: &AppHandle<R>, backups: Backups) {
    let mut changes = backups.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let status = changes.borrow_and_update().clone();
            if let Some(status) = status
                && let Err(error) = BackupChanged(status).emit(&handle)
            {
                tracing::debug!(%error, "backup status not sent");
            }
        }
    });
    crate::background::supervise("automatic backups", {
        let backups = backups.clone();
        move || backups.watch()
    });
    app.manage(backups);
}

#[tauri::command]
#[specta::specta]
pub async fn backup_status(backups: State<'_, Backups>) -> Result<BackupStatus, CommandError> {
    Ok(backups.status().await)
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
    let destination = match destination.filter(|d| !d.trim().is_empty()) {
        Some(destination) => PathBuf::from(destination),
        None => backups.destination().await,
    };
    backups
        .plan(&ExportOptions {
            destination,
            components: components.into_iter().collect(),
            set_id: None,
        })
        .await
        .map_err(report)
}

/// Back up now, with the settings' folder and components. Returns once it
/// starts; `BackupChanged` follows it to its outcome.
#[tauri::command]
#[specta::specta]
pub async fn backup_start(backups: State<'_, Backups>) -> Result<(), CommandError> {
    if backups.status().await.running.is_some() {
        return Err(report(caskade_backup::BackupError::Busy));
    }
    let backups = backups.inner().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = backups.back_up_now().await {
            tracing::warn!(%error, "the backup failed");
        }
    });
    Ok(())
}

/// Cancels the backup, check or restore under way.
#[tauri::command]
#[specta::specta]
pub fn backup_cancel(backups: State<'_, Backups>) {
    backups.cancel();
}

/// Checks a backup and says what restoring it would do.
#[tauri::command]
#[specta::specta]
pub async fn backup_inspect(
    backups: State<'_, Backups>,
    path: String,
) -> Result<Inspection, CommandError> {
    backups
        .inspect_job(&PathBuf::from(path))
        .await
        .map_err(report)
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
    backups
        .restore_job(caskade_backup::RestoreOptions {
            archive: PathBuf::from(path),
            components: components.into_iter().collect(),
        })
        .await
        .map_err(report)
}

/// Forgets the restore waiting for the restart.
#[tauri::command]
#[specta::specta]
pub async fn backup_restore_discard(backups: State<'_, Backups>) -> Result<(), CommandError> {
    backups.discard_restore_job().await.map_err(report)
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
    if backups.staged_restore().map_err(report)?.is_none() {
        return Err(CommandError::new(
            ErrorCode::Invalid,
            caskade_core::tr!("No restore is waiting for a restart."),
        ));
    }
    if let Some(launch) = app.try_state::<LaunchService>()
        && !launch.running_sessions().await.map_err(report)?.is_empty()
    {
        return Err(CommandError::new(
            ErrorCode::Conflict,
            caskade_core::tr!("Close your running games first: restoring restarts Caskade."),
        ));
    }
    if let Some(install) = app.try_state::<InstallService>()
        && install.jobs().iter().any(|job| !job.status.is_finished())
    {
        return Err(CommandError::new(
            ErrorCode::Conflict,
            caskade_core::tr!(
                "Wait for installs, moves and updates to finish, or cancel them: restoring restarts Caskade."
            ),
        ));
    }
    if backups.status().await.running.is_some() {
        return Err(report(caskade_backup::BackupError::Busy));
    }
    crate::tray::restart(&app);
    Ok(())
}

/// Shows a backup in the file manager: only a file named like one, in the
/// backup folder or the last backup's.
#[tauri::command]
#[specta::specta]
pub async fn backup_reveal(backups: State<'_, Backups>, path: String) -> Result<(), CommandError> {
    let path = PathBuf::from(path);
    let named = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(caskade_backup::PREFIX)
                && name.ends_with(&format!(".{}", caskade_backup::EXTENSION))
        });
    let status = backups.status().await;
    let folders = [
        Some(backups.destination().await),
        status
            .last
            .and_then(|last| last.path)
            .and_then(|last| last.parent().map(PathBuf::from)),
    ];
    let folder = path.parent().map(PathBuf::from);
    if !named || folder.is_none() || !folders.contains(&folder) {
        return Err(CommandError::new(
            ErrorCode::Invalid,
            caskade_core::tr!("That isn't a backup in the backup folder."),
        ));
    }
    let folder = folder.unwrap_or_default();
    // Explorer and Finder select the file; xdg-open opens its folder.
    let args: Vec<std::ffi::OsString> = if cfg!(target_os = "windows") {
        vec![format!("/select,{}", path.display()).into()]
    } else if cfg!(target_os = "macos") {
        vec!["-R".into(), path.into_os_string()]
    } else {
        vec![folder.clone().into_os_string()]
    };
    super::library::open_file_manager(args, &folder)
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
