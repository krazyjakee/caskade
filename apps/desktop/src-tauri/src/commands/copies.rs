//! Copies of games between the user's devices (#233): Copy to this
//! computer, and the copies going on both ways for Settings → Devices.
//! Forwarded to `caskade_app::commands::copies`, but for taking other
//! devices' send requests, which go to the webview's install confirmations.
use caskade_app::commands::copies as app;
use caskade_core::db::LibraryStore;
use caskade_core::devices::Devices;
use caskade_core::install::{InstallService, JobSnapshot};
use caskade_core::transfer::Transfers;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::error::CommandError;

pub use app::{TransferDirection, TransferState, TransferView};

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

/// Takes other devices' requests to copy a game here (#235): queued in the
/// install confirmations, or started at once into the default storage when
/// `core.sync.acceptCopies` is on.
pub fn take_sends<R: Runtime>(app: &AppHandle<R>, transfers: &Transfers, devices: Devices) {
    let handle = app.clone();
    transfers.on_send(move |request| {
        let handle = handle.clone();
        let devices = devices.clone();
        caskade_app::runtime::spawn(async move {
            let device_name = devices
                .status()
                .await
                .devices
                .into_iter()
                .find(|device| device.id == request.device_id)
                .map(|device| device.name)
                .unwrap_or_default();
            let send = super::install_requests::PeerSend {
                device_id: request.device_id,
                device_name,
                entry_id: request.entry_id,
                game_id: request.game_id,
                title: request.title,
                size_bytes: request.size,
            };
            let accept = match handle.try_state::<caskade_core::settings::SettingsService>() {
                Some(settings) => {
                    settings
                        .read_or_default::<caskade_core::sync::policy::SyncSettings>("core.sync")
                        .await
                        .accept_copies
                }
                None => false,
            };
            if accept
                && let Some(install) = handle.try_state::<InstallService>()
                && install.copy_from_device(send.target(), None).await.is_ok()
            {
                return;
            }
            if let Some(requests) = handle.try_state::<super::install_requests::InstallRequests>()
                && let Err(error) = requests.queue_peer(&crate::events::sink(handle.clone()), send)
            {
                tracing::warn!(%error, "a copy from another device wasn't queued");
            }
        });
    });
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
