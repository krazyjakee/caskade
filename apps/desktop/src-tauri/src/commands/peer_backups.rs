//! Backups kept on the user's other devices (#243): what each paired
//! device holds, and fetching one here to restore it through the usual
//! inspect and staged restore (`backup_inspect`, `backup_restore`).
//! Forwarded to `caskade_app::commands::peer_backups`, with the services
//! that may not be up yet.
use crate::error::CommandError;
use caskade_app::commands::peer_backups as app;
use caskade_backup::peers::PeerBackups;
use caskade_core::{devices::Devices, sync::SyncService};
use tauri::{AppHandle, Manager, Runtime};

pub use app::DeviceBackups;

/// What paired devices keep, as each last said when they synced.
#[tauri::command]
#[specta::specta]
pub async fn backups_on_devices<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<DeviceBackups>, CommandError> {
    let peers = app.try_state::<PeerBackups>();
    let devices = app.try_state::<Devices>();
    app::backups_on_devices(peers.as_deref(), devices.as_deref()).await
}

/// Fetches the backup `hash` from the paired device that keeps it, and
/// returns where it is here, to inspect and restore.
#[tauri::command]
#[specta::specta]
pub async fn backup_fetch_from_device<R: Runtime>(
    app: AppHandle<R>,
    hash: String,
) -> Result<String, CommandError> {
    let peers = app.try_state::<PeerBackups>();
    let sync = app.try_state::<SyncService>();
    app::backup_fetch_from_device(peers.as_deref(), sync.as_deref(), &hash).await
}
