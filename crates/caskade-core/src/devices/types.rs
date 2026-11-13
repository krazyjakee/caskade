//! What the devices service shows: this device, its paired and nearby
//! devices, and a pairing under way.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::DeviceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceOs {
    Linux,
    Windows,
    Mac,
}

/// How a device is reached right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceLink {
    Lan,
    Direct,
    Relay,
    Offline,
}

/// Where this device's identity stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IdentityStatus {
    /// Being read from the keychain.
    Starting,
    Ready,
    /// The keychain lost this device's key. A new identity needs asking:
    /// paired devices must pair again.
    Missing,
    /// The keychain entry isn't a key; the same choice as `missing`.
    Unreadable,
    /// The keychain didn't answer; sync waits.
    KeychainUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThisDevice {
    /// Its endpoint id, once the identity is ready.
    pub id: Option<String>,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    /// Addresses on this network other devices reach it at.
    pub lan_addresses: Vec<String>,
    /// Whether the internet can be reached (a relay answers); `None` when
    /// it can't be told (Never use relays is on, or the endpoint isn't up).
    /// Devices on this network sync without it.
    pub internet: Option<bool>,
    pub never_relay: bool,
    pub port: Option<u16>,
    pub identity: IdentityStatus,
    /// The endpoint is running: other devices can reach this one.
    pub listening: bool,
}

/// A device paired with this one. Its grants stay in the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    pub link: DeviceLink,
    /// Where it was last reached.
    pub addresses: Vec<String>,
    pub last_sync_ms: Option<i64>,
    pub paired_at_ms: i64,
    /// Library sync with it is running now.
    pub syncing: bool,
    /// Why the last sync failed, if it did.
    pub sync_error: Option<String>,
}

/// A Caskade device on this network that isn't paired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NearbyDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PairingState {
    /// Codes on screen, waiting for one or both users.
    Waiting,
    Paired,
    /// Cancelled on either device; nothing was stored.
    Cancelled,
    /// The connection failed or timed out; nothing was stored.
    Failed,
}

/// A code pairing: both devices show `code`, and both users confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    pub code: String,
    /// The other device started it.
    pub incoming: bool,
    /// This device's user confirmed.
    pub confirmed: bool,
    pub state: PairingState,
}

/// A pairing link to show or copy. It works once, until `expires_at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingLinkInfo {
    pub url: String,
    pub expires_at_ms: i64,
}

/// A pairing link opened from outside the app (a `caskade://pair` link),
/// shown for an admin to accept. The link itself stays in the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingLinkTarget {
    /// The device it pairs with.
    pub device_id: String,
    pub expires_at_ms: i64,
    pub expired: bool,
}

/// Everything Settings → Devices shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DevicesStatus {
    pub this_device: ThisDevice,
    pub devices: Vec<PairedDevice>,
    pub nearby: Vec<NearbyDevice>,
    pub pairing: Option<Pairing>,
}

impl From<caskade_sync::DeviceKind> for DeviceKind {
    fn from(kind: caskade_sync::DeviceKind) -> Self {
        use caskade_sync::DeviceKind as K;
        match kind {
            K::Desktop => Self::Desktop,
            K::Laptop => Self::Laptop,
            K::Handheld => Self::Handheld,
            K::Tv => Self::Tv,
        }
    }
}

impl From<DeviceKind> for caskade_sync::DeviceKind {
    fn from(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::Desktop => Self::Desktop,
            DeviceKind::Laptop => Self::Laptop,
            DeviceKind::Handheld => Self::Handheld,
            DeviceKind::Tv => Self::Tv,
        }
    }
}

impl From<caskade_sync::DeviceOs> for DeviceOs {
    fn from(os: caskade_sync::DeviceOs) -> Self {
        use caskade_sync::DeviceOs as O;
        match os {
            O::Linux => Self::Linux,
            O::Windows => Self::Windows,
            O::Mac => Self::Mac,
        }
    }
}

impl From<caskade_sync::Link> for DeviceLink {
    fn from(link: caskade_sync::Link) -> Self {
        use caskade_sync::Link as L;
        match link {
            L::Lan => Self::Lan,
            L::Direct => Self::Direct,
            L::Relay => Self::Relay,
            L::Offline => Self::Offline,
        }
    }
}
