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

use std::sync::Mutex;

use crate::error::{CommandError, report};
use caskade_core::devices::{
    DeviceKind, Devices, DevicesStatus, PairedDevice, Pairing, PairingLinkInfo, PairingLinkTarget,
    inspect_link,
};
use caskade_core::sync::SyncService;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

/// Devices changed: a pairing, a link state, a nearby device, a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct DevicesChanged(pub DevicesStatus);

/// A `caskade://pair` link was opened; Settings → Devices asks an admin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct PairingLinkOpened(pub PairingLinkTarget);

/// The last pairing link opened from outside, until accepted or dismissed.
#[derive(Default)]
pub struct OpenedPairingLink(Mutex<Option<String>>);

impl OpenedPairingLink {
    fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    fn peek(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, link: String) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(link);
    }
}

/// Starts the devices service (identity, endpoint, pairing) and library sync
/// (#230), and forwards their changes to the webview: each device's sync
/// state (syncing, last sync, the last failure) is in `DevicesChanged`.
pub fn setup<R: Runtime>(app: &AppHandle<R>, devices: Devices, sync: SyncService) {
    let mut changes = devices.subscribe();
    let handle = app.clone();
    let watched = devices.clone();
    tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            changes.borrow_and_update();
            let status = watched.status().await;
            if let Err(error) = DevicesChanged(status).emit(&handle) {
                tracing::debug!(%error, "devices status not sent");
            }
        }
    });
    crate::background::supervise("devices", {
        let devices = devices.clone();
        move || devices.start()
    });
    crate::background::supervise("library sync", {
        let sync = sync.clone();
        move || sync.start()
    });
    app.manage(devices);
    app.manage(sync);
    app.manage(OpenedPairingLink::default());
}

/// Whether `link` is a `caskade://pair` link.
pub fn is_pair_link(link: &tauri::Url) -> bool {
    link.scheme() == "caskade"
        && format!("{}{}", link.host_str().unwrap_or_default(), link.path()).trim_matches('/')
            == "pair"
}

/// Holds an opened pairing link for an admin to accept, and shows the
/// window.
pub fn open_pair_link<R: Runtime>(app: &AppHandle<R>, link: &tauri::Url) {
    if hold_pair_link(app, link) {
        crate::main_window::raise(app);
    }
}

/// Holds an opened pairing link and announces it; `false` when it isn't one.
/// Never logs the link: it's a credential.
fn hold_pair_link<R: Runtime>(app: &AppHandle<R>, link: &tauri::Url) -> bool {
    let Ok(target) = inspect_link(link.as_str()) else {
        tracing::info!("an opened pairing link wasn't one");
        return false;
    };
    if let Some(opened) = app.try_state::<OpenedPairingLink>() {
        opened.set(link.as_str().to_owned());
    }
    if let Err(error) = PairingLinkOpened(target).emit(app) {
        tracing::debug!(%error, "pairing link not announced");
    }
    true
}

#[tauri::command]
#[specta::specta]
pub async fn devices_status(devices: State<'_, Devices>) -> Result<DevicesStatus, CommandError> {
    Ok(devices.status().await)
}

/// Renames this device; an empty name goes back to the host name.
#[tauri::command]
#[specta::specta]
pub async fn devices_rename(devices: State<'_, Devices>, name: String) -> Result<(), CommandError> {
    devices.rename(&name).await.map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn devices_set_kind(
    devices: State<'_, Devices>,
    kind: DeviceKind,
) -> Result<(), CommandError> {
    devices.set_kind(kind).await.map_err(report)
}

/// Starts pairing with a nearby device: both devices show the code.
#[tauri::command]
#[specta::specta]
pub async fn devices_pair_nearby(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<Pairing, CommandError> {
    devices.pair_nearby(&device_id).await.map_err(report)
}

/// This user confirms the codes match.
#[tauri::command]
#[specta::specta]
pub async fn devices_confirm_pairing(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<(), CommandError> {
    devices.confirm_pairing(&device_id).await.map_err(report)
}

/// Cancels a pairing, or dismisses a finished one.
#[tauri::command]
#[specta::specta]
pub async fn devices_cancel_pairing(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<(), CommandError> {
    devices.cancel_pairing(&device_id).await.map_err(report)
}

/// A link that pairs another device with this one from anywhere: once, for
/// ten minutes.
#[tauri::command]
#[specta::specta]
pub async fn devices_pairing_link(
    devices: State<'_, Devices>,
) -> Result<PairingLinkInfo, CommandError> {
    devices.pairing_link().await.map_err(report)
}

/// Pairs with the device a pasted pairing link names.
#[tauri::command]
#[specta::specta]
pub async fn devices_accept_pairing_link(
    devices: State<'_, Devices>,
    link: String,
) -> Result<PairedDevice, CommandError> {
    devices.accept_pairing_link(&link).await.map_err(report)
}

/// The pairing link opened from outside, if one waits.
#[tauri::command]
#[specta::specta]
pub async fn devices_opened_link(
    opened: State<'_, OpenedPairingLink>,
) -> Result<Option<PairingLinkTarget>, CommandError> {
    Ok(opened.peek().and_then(|link| inspect_link(&link).ok()))
}

/// Pairs with the device the opened link names.
#[tauri::command]
#[specta::specta]
pub async fn devices_accept_opened_link(
    devices: State<'_, Devices>,
    opened: State<'_, OpenedPairingLink>,
) -> Result<PairedDevice, CommandError> {
    let Some(link) = opened.peek() else {
        return Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
            "no pairing link is waiting"
        ))));
    };
    let paired = devices.accept_pairing_link(&link).await.map_err(report)?;
    opened.take();
    Ok(paired)
}

#[tauri::command]
#[specta::specta]
pub async fn devices_dismiss_opened_link(
    opened: State<'_, OpenedPairingLink>,
) -> Result<(), CommandError> {
    opened.take();
    Ok(())
}

/// Forgets a paired device; it's refused from now on and told when it can
/// be reached. Nothing synced is deleted.
#[tauri::command]
#[specta::specta]
pub async fn devices_forget(
    devices: State<'_, Devices>,
    device_id: String,
) -> Result<(), CommandError> {
    devices.forget(&device_id).await.map_err(report)
}

/// Syncs with a paired device now, instead of at the next poll. The result
/// shows in `DevicesChanged`.
#[tauri::command]
#[specta::specta]
pub async fn devices_sync_now(
    sync: State<'_, SyncService>,
    device_id: String,
) -> Result<(), CommandError> {
    sync.sync_now(&device_id);
    Ok(())
}

/// Makes a new identity after the keychain lost the old one: paired devices
/// must pair again. Only after the user agreed.
#[tauri::command]
#[specta::specta]
pub async fn devices_create_identity(devices: State<'_, Devices>) -> Result<(), CommandError> {
    devices.create_identity().await.map_err(report)
}

/// Reads the keychain again after it didn't answer.
#[tauri::command]
#[specta::specta]
pub async fn devices_retry_identity(devices: State<'_, Devices>) -> Result<(), CommandError> {
    devices.retry_identity().await.map_err(report)
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
