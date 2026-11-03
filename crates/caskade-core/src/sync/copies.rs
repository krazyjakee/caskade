//! A game's copies on the user's other devices (#232), for its page and its
//! menu: which device, how it's reached, the size, and whether the copy
//! could come here. Copying (#233) and playing there (#234) come later.

use serde::Serialize;
use specta::Type;

use super::presence::Presence;
use crate::Result;
use crate::db::{CopyLocation, LibraryStore, this_os};
use crate::devices::{DeviceKind, DeviceLink, DeviceOs, Devices};

/// Why a copy can't come to this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CopyBlocked {
    /// It starts through its store's launcher there: install it from the
    /// store here instead.
    LauncherOnly,
    /// A build for another system (a Linux or macOS build here on Windows).
    OtherSystem,
}

/// Another device's copy of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PeerCopy {
    pub device_id: String,
    pub device_name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    /// How the device is reached now.
    pub link: DeviceLink,
    pub size_bytes: Option<u64>,
    pub location: CopyLocation,
    /// Why it can't come here; `None` when it could.
    pub blocked: Option<CopyBlocked>,
    /// A Windows build that runs here through Proton.
    pub through_proton: bool,
    /// The game is running on that device now.
    pub playing: bool,
    /// Out of reach, it can be woken: its Wake-on-LAN addresses are known
    /// (#234).
    pub wakeable: bool,
}

/// `game`'s copies on paired devices, the best reached first.
pub async fn peer_copies(
    store: &LibraryStore,
    devices: &Devices,
    presence: Option<&Presence>,
    game: &str,
) -> Result<Vec<PeerCopy>> {
    let records = store.sync_peer_copies(game).await?;
    if records.is_empty() {
        return Ok(Vec::new());
    }
    let status = devices.status().await;
    let playing = match presence {
        Some(presence) => presence.playing(devices).await,
        None => Vec::new(),
    };
    let here = this_os();
    let mut wakeable = std::collections::HashSet::new();
    for record in &records {
        if !store
            .paired_device_macs(&record.device_id)
            .await?
            .is_empty()
        {
            wakeable.insert(record.device_id.clone());
        }
    }
    let mut copies: Vec<PeerCopy> = records
        .into_iter()
        .filter_map(|record| {
            let device = status.devices.iter().find(|d| d.id == record.device_id)?;
            let runs_here =
                record.copy.os == here || (here == "linux" && record.copy.os == "windows");
            let blocked = if !record.copy.copyable {
                Some(CopyBlocked::LauncherOnly)
            } else if !runs_here {
                Some(CopyBlocked::OtherSystem)
            } else {
                None
            };
            Some(PeerCopy {
                device_id: device.id.clone(),
                device_name: device.name.clone(),
                kind: device.kind,
                os: device.os,
                link: device.link,
                size_bytes: record.copy.size,
                location: record.copy.location,
                blocked,
                through_proton: here == "linux" && record.copy.os == "windows",
                playing: playing
                    .iter()
                    .any(|p| p.device_id == device.id && p.game_id == game),
                wakeable: wakeable.contains(&device.id),
            })
        })
        .collect();
    let order = |link: DeviceLink| match link {
        DeviceLink::Lan => 0,
        DeviceLink::Direct => 1,
        DeviceLink::Relay => 2,
        DeviceLink::Offline => 3,
    };
    copies.sort_by(|a, b| (order(a.link), &a.device_name).cmp(&(order(b.link), &b.device_name)));
    // One row per device: its first copy.
    copies.dedup_by(|a, b| a.device_id == b.device_id);
    Ok(copies)
}
