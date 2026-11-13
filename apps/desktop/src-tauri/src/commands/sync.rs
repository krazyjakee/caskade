//! Settings → Devices (#226; its UI is #231): this device, paired devices,
//! nearby ones, pairing by code or link, and forgetting. The status follows
//! `DevicesChanged`.
//!
//! Every change is admin-only in the core (`caskade_core::devices`), and the
//! permission table here says the same. No grant or device key crosses into
//! the webview; a `caskade://pair` link opened from outside stays here too,
//! and pairs only when an admin accepts it in the app
//! ([`devices_accept_opened_link`]): a web page can open such a link, so
//! opening one must not pair by itself.
//!
//! Forwarded to `caskade_app::commands::sync`.
use crate::error::CommandError;
use caskade_app::commands::sync as app;
use caskade_core::devices::{
    DeviceKind, Devices, DevicesStatus, PairedDevice, Pairing, PairingLinkInfo, PairingLinkTarget,
};
use caskade_core::sync::SyncService;
use tauri::State;

pub use app::OpenedPairingLink;
// The webview's events, which the tests listen for.
#[cfg(test)]
use {
    crate::events::{DevicesChanged, PairingLinkOpened},
    app::is_pair_link,
    tauri_specta::Event,
};

#[tauri::command]
#[specta::specta]
pub async fn devices_status(devices: State<'_, Devices>) -> Result<DevicesStatus, CommandError> {
    app::devices_status(&devices).await
}

/// A game's copies on the user's other devices (#232): the device, how
/// it's reached, the size, and whether the copy could come here.
#[tauri::command]
#[specta::specta]
pub async fn game_other_devices(
    store: State<'_, caskade_core::db::LibraryStore>,
    devices: State<'_, Devices>,
    presence: State<'_, caskade_core::sync::presence::Presence>,
    game_id: String,
) -> Result<Vec<caskade_core::sync::copies::PeerCopy>, CommandError> {
    app::game_other_devices(&store, &devices, &presence, &game_id).await
}

/// Renames this device; an empty name goes back to the host name.
#[tauri::command]
#[specta::specta]
pub async fn devices_rename(devices: State<'_, Devices>, name: String) -> Result<(), CommandError> {
    app::devices_rename(&devices, &name).await
}

#[tauri::command]
#[specta::specta]
pub async fn devices_set_kind(
    devices: State<'_, Devices>,
    kind: DeviceKind,
) -> Result<(), CommandError> {
    app::devices_set_kind(&devices, kind).await
}

/// Starts pairing with a nearby device: both devices show the code.
#[tauri::command]
#[specta::specta]
pub async fn devices_pair_nearby(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<Pairing, CommandError> {
    app::devices_pair_nearby(&devices, &device_id).await
}

/// This user confirms the codes match.
#[tauri::command]
#[specta::specta]
pub async fn devices_confirm_pairing(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<(), CommandError> {
    app::devices_confirm_pairing(&devices, &device_id).await
}

/// Cancels a pairing, or dismisses a finished one.
#[tauri::command]
#[specta::specta]
pub async fn devices_cancel_pairing(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<(), CommandError> {
    app::devices_cancel_pairing(&devices, &device_id).await
}

/// A link that pairs another device with this one from anywhere: once, for
/// ten minutes.
#[tauri::command]
#[specta::specta]
pub async fn devices_pairing_link(
    devices: State<'_, Devices>,
) -> Result<PairingLinkInfo, CommandError> {
    app::devices_pairing_link(&devices).await
}

/// Pairs with the device a pasted pairing link names.
#[tauri::command]
#[specta::specta]
pub async fn devices_accept_pairing_link(
    devices: State<'_, Devices>,
    link: String,
) -> Result<PairedDevice, CommandError> {
    app::devices_accept_pairing_link(&devices, &link).await
}

/// The pairing link opened from outside, if one waits.
#[tauri::command]
#[specta::specta]
pub async fn devices_opened_link(
    opened: State<'_, OpenedPairingLink>,
) -> Result<Option<PairingLinkTarget>, CommandError> {
    app::devices_opened_link(&opened).await
}

/// Pairs with the device the opened link names.
#[tauri::command]
#[specta::specta]
pub async fn devices_accept_opened_link(
    devices: State<'_, Devices>,
    opened: State<'_, OpenedPairingLink>,
) -> Result<PairedDevice, CommandError> {
    app::devices_accept_opened_link(&devices, &opened).await
}

#[tauri::command]
#[specta::specta]
pub async fn devices_dismiss_opened_link(
    opened: State<'_, OpenedPairingLink>,
) -> Result<(), CommandError> {
    app::devices_dismiss_opened_link(&opened).await
}

/// Forgets a paired device; it's refused from now on and told when it can
/// be reached. Nothing synced is deleted.
#[tauri::command]
#[specta::specta]
pub async fn devices_forget(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<(), CommandError> {
    app::devices_forget(&devices, &device_id).await
}

/// Syncs with a paired device now, instead of at the next poll. The result
/// shows in `DevicesChanged`.
#[tauri::command]
#[specta::specta]
pub async fn devices_sync_now(
    sync: State<'_, SyncService>,
    device_id: String,
) -> Result<(), CommandError> {
    app::devices_sync_now(&sync, &device_id).await
}

/// Makes a new identity after the keychain lost the old one: paired devices
/// must pair again. Only after the user agreed.
#[tauri::command]
#[specta::specta]
pub async fn devices_create_identity(devices: State<'_, Devices>) -> Result<(), CommandError> {
    app::devices_create_identity(&devices).await
}

/// Reads the keychain again after it didn't answer.
#[tauri::command]
#[specta::specta]
pub async fn devices_retry_identity(devices: State<'_, Devices>) -> Result<(), CommandError> {
    app::devices_retry_identity(&devices).await
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
