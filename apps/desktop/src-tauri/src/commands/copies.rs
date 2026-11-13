//! Copies of games between the user's devices (#233): Copy to this
//! computer, and the copies going on both ways for Settings → Devices.
//! Forwarded to `caskade_app::commands::copies`, but for taking other
//! devices' send requests, which go to the webview's install confirmations.
use caskade_app::commands::copies as app;
use caskade_core::db::LibraryStore;
use caskade_core::devices::Devices;
use caskade_core::install::{InstallService, JobSnapshot};
use caskade_core::transfer::Transfers;
use tauri::State;

use crate::error::CommandError;

pub use app::TransferView;

/// Copies `game_id` from `device_id` to this computer, into the storage
/// root `root_id` (the default when absent), as an install job.
#[tauri::command]
#[specta::specta]
pub async fn game_copy_here(
    install: State<'_, InstallService>,
    store: State<'_, LibraryStore>,
    devices: State<'_, Devices>,
    game_id: String,
    device_id: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    app::game_copy_here(&install, &store, &devices, game_id, device_id, root_id).await
}

/// Asks `device_id` to copy `game_id` from this computer (#235): the
/// device confirms it there, unless it accepts copies without asking.
#[tauri::command]
#[specta::specta]
pub async fn game_send_to(
    transfers: State<'_, Transfers>,
    store: State<'_, LibraryStore>,
    game_id: String,
    device_id: String,
) -> Result<(), CommandError> {
    app::game_send_to(&transfers, &store, game_id, device_id).await
}

/// Copies going on to and from this device.
#[tauri::command]
#[specta::specta]
pub async fn devices_transfers(
    transfers: State<'_, Transfers>,
    devices: State<'_, Devices>,
) -> Result<Vec<TransferView>, CommandError> {
    app::devices_transfers(&transfers, &devices).await
}
