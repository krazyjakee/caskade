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
use crate::events::{Event, Events};
use caskade_core::devices::{
    DeviceKind, Devices, DevicesStatus, PairedDevice, Pairing, PairingLinkInfo, PairingLinkTarget,
    inspect_link,
};
use caskade_core::sync::SyncService;
use serde::{Deserialize, Serialize};
use specta::Type;
/// Devices changed: a pairing, a link state, a nearby device, a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DevicesChanged(pub DevicesStatus);

/// A `caskade://pair` link was opened; Settings → Devices asks an admin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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

pub async fn devices_status(devices: &Devices) -> Result<DevicesStatus, CommandError> {
    crate::permissions::enforce("devices_status", serde_json::Value::default)?;
    Ok(devices.status().await)
}

/// A game's copies on the user's other devices (#232): the device, how
/// it's reached, the size, and whether the copy could come here.
pub async fn game_other_devices(
    store: &caskade_core::db::LibraryStore,
    devices: &Devices,
    presence: &caskade_core::sync::presence::Presence,
    game_id: &str,
) -> Result<Vec<caskade_core::sync::copies::PeerCopy>, CommandError> {
    crate::permissions::enforce("game_other_devices", serde_json::Value::default)?;
    caskade_core::sync::copies::peer_copies(store, devices, Some(presence), game_id)
        .await
        .map_err(report)
}

/// Renames this device; an empty name goes back to the host name.
pub async fn devices_rename(devices: &Devices, name: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_rename", serde_json::Value::default)?;
    devices.rename(name).await.map_err(report)
}

pub async fn devices_set_kind(devices: &Devices, kind: DeviceKind) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_set_kind", serde_json::Value::default)?;
    devices.set_kind(kind).await.map_err(report)
}

/// Starts pairing with a nearby device: both devices show the code.
pub async fn devices_pair_nearby(
    devices: &Devices,
    device_id: &str,
) -> Result<Pairing, CommandError> {
    crate::permissions::enforce("devices_pair_nearby", serde_json::Value::default)?;
    devices.pair_nearby(device_id).await.map_err(report)
}

/// This user confirms the codes match.
pub async fn devices_confirm_pairing(
    devices: &Devices,
    device_id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_confirm_pairing", serde_json::Value::default)?;
    devices.confirm_pairing(device_id).await.map_err(report)
}

/// Cancels a pairing, or dismisses a finished one.
pub async fn devices_cancel_pairing(
    devices: &Devices,
    device_id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_cancel_pairing", serde_json::Value::default)?;
    devices.cancel_pairing(device_id).await.map_err(report)
}

/// A link that pairs another device with this one from anywhere: once, for
/// ten minutes.
pub async fn devices_pairing_link(devices: &Devices) -> Result<PairingLinkInfo, CommandError> {
    crate::permissions::enforce("devices_pairing_link", serde_json::Value::default)?;
    devices.pairing_link().await.map_err(report)
}

/// Pairs with the device a pasted pairing link names.
pub async fn devices_accept_pairing_link(
    devices: &Devices,
    link: &str,
) -> Result<PairedDevice, CommandError> {
    crate::permissions::enforce("devices_accept_pairing_link", serde_json::Value::default)?;
    devices.accept_pairing_link(link).await.map_err(report)
}

/// The pairing link opened from outside, if one waits.
pub async fn devices_opened_link(
    opened: &OpenedPairingLink,
) -> Result<Option<PairingLinkTarget>, CommandError> {
    crate::permissions::enforce("devices_opened_link", serde_json::Value::default)?;
    Ok(opened.peek().and_then(|link| inspect_link(&link).ok()))
}

/// Pairs with the device the opened link names.
pub async fn devices_accept_opened_link(
    devices: &Devices,
    opened: &OpenedPairingLink,
) -> Result<PairedDevice, CommandError> {
    crate::permissions::enforce("devices_accept_opened_link", serde_json::Value::default)?;
    let Some(link) = opened.peek() else {
        return Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
            "no pairing link is waiting"
        ))));
    };
    let paired = devices.accept_pairing_link(&link).await.map_err(report)?;
    opened.take();
    Ok(paired)
}

pub async fn devices_dismiss_opened_link(opened: &OpenedPairingLink) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_dismiss_opened_link", serde_json::Value::default)?;
    opened.take();
    Ok(())
}

/// Forgets a paired device; it's refused from now on and told when it can
/// be reached. Nothing synced is deleted.
pub async fn devices_forget(devices: &Devices, device_id: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_forget", serde_json::Value::default)?;
    devices.forget(device_id).await.map_err(report)
}

/// Syncs with a paired device now, instead of at the next poll. The result
/// shows in `DevicesChanged`.
pub async fn devices_sync_now(sync: &SyncService, device_id: &str) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_sync_now", serde_json::Value::default)?;
    sync.sync_now(device_id);
    Ok(())
}

/// Makes a new identity after the keychain lost the old one: paired devices
/// must pair again. Only after the user agreed.
pub async fn devices_create_identity(devices: &Devices) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_create_identity", serde_json::Value::default)?;
    devices.create_identity().await.map_err(report)
}

/// Reads the keychain again after it didn't answer.
pub async fn devices_retry_identity(devices: &Devices) -> Result<(), CommandError> {
    crate::permissions::enforce("devices_retry_identity", serde_json::Value::default)?;
    devices.retry_identity().await.map_err(report)
}

/// Starts the devices service (identity, endpoint, pairing) and library sync
/// (#230), and tells the UI of their changes: each device's sync state
/// (syncing, last sync, the last failure) is in `DevicesChanged`.
pub fn setup(events: Events, devices: &Devices, sync: &SyncService) {
    let mut changes = devices.subscribe();
    let watched = devices.clone();
    crate::runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            changes.borrow_and_update();
            let status = watched.status().await;
            if let Err(error) = events.emit(Event::DevicesChanged(DevicesChanged(status))) {
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
}

/// Whether `link` is a `caskade://pair` link.
pub fn is_pair_link(link: &url::Url) -> bool {
    link.scheme() == "caskade"
        && format!("{}{}", link.host_str().unwrap_or_default(), link.path()).trim_matches('/')
            == "pair"
}

/// Holds an opened pairing link for an admin to accept and announces it;
/// `false` when it isn't one. Never logs the link: it's a credential.
pub fn hold_pair_link(
    events: &Events,
    opened: Option<&OpenedPairingLink>,
    link: &url::Url,
) -> bool {
    let Ok(target) = inspect_link(link.as_str()) else {
        tracing::info!("an opened pairing link wasn't one");
        return false;
    };
    if let Some(opened) = opened {
        opened.set(link.as_str().to_owned());
    }
    if let Err(error) = events.emit(Event::PairingLinkOpened(PairingLinkOpened(target))) {
        tracing::debug!(%error, "pairing link not announced");
    }
    true
}
