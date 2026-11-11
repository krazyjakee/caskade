//! Copies of games between the user's devices (#233): Copy to this
//! computer, and the copies going on both ways for Settings → Devices.

use caskade_core::db::LibraryStore;
use caskade_core::devices::{DeviceLink, Devices};
use caskade_core::install::{InstallService, JobSnapshot, PeerCopyTarget};
use caskade_core::transfer::Transfers;
use serde::Serialize;
use specta::Type;

use crate::error::{CommandError, report};
/// Which way a copy goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    /// To this device.
    Incoming,
    /// To another device.
    Outgoing,
}

/// How a copy is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TransferState {
    Running,
    /// Only a relay reaches the device: game files wait for a direct
    /// connection.
    WaitingDirect,
}

/// A copy between this device and another, as the app shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    pub title: String,
    pub device_name: String,
    pub direction: TransferDirection,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: u64,
    pub link: DeviceLink,
    pub state: TransferState,
}

/// Copies `game_id` from `device_id` to this computer, into the storage
/// root `root_id` (the default when absent), as an install job.
pub async fn game_copy_here(
    install: &InstallService,
    store: &LibraryStore,
    devices: &Devices,
    game_id: String,
    device_id: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, CommandError> {
    crate::permissions::enforce("game_copy_here", serde_json::Value::default)?;
    let copy = store
        .sync_peer_copies(&game_id)
        .await
        .map_err(report)?
        .into_iter()
        .find(|copy| copy.device_id == device_id)
        .ok_or_else(|| {
            report(caskade_core::Error::Invalid(caskade_core::tr!(
                "that device has no copy of the game"
            )))
        })?;
    let device_name = devices
        .status()
        .await
        .devices
        .into_iter()
        .find(|device| device.id == device_id)
        .map(|device| device.name)
        .ok_or_else(|| {
            report(caskade_core::Error::Invalid(caskade_core::tr!(
                "that device isn't paired"
            )))
        })?;
    let title = store
        .game(&game_id)
        .await
        .map_err(report)?
        .map(|game| game.title)
        .unwrap_or_default();
    install
        .copy_from_device(
            PeerCopyTarget {
                device_id,
                device_name,
                entry_id: copy.entry_id,
                game_id,
                title,
            },
            root_id,
        )
        .await
        .map_err(report)
}

/// Asks `device_id` to copy `game_id` from this computer (#235): the
/// device confirms it there, unless it accepts copies without asking.
pub async fn game_send_to(
    transfers: &Transfers,
    store: &LibraryStore,
    game_id: String,
    device_id: String,
) -> Result<(), CommandError> {
    crate::permissions::enforce("game_send_to", serde_json::Value::default)?;
    let entries = store.entries_for_game(&game_id).await.map_err(report)?;
    // A release installed here in a folder that can be copied: a managed
    // one first.
    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|entry| {
            let source = entry.source_id.as_str();
            !(source.starts_with("peer:")
                || source.starts_with("store:")
                || source.starts_with("stream:"))
                && entry.state == caskade_core::model::InstallState::Installed
                && entry.location.is_some()
        })
        .collect();
    candidates.sort_by_key(|entry| {
        !matches!(
            entry.location,
            Some(caskade_core::model::InstallLocation::Managed { .. })
        )
    });
    let mut last = None;
    for entry in candidates {
        match transfers.send_to(&device_id, &entry.id).await {
            Ok(()) => return Ok(()),
            Err(error) => last = Some(error),
        }
    }
    Err(report(last.unwrap_or_else(|| {
        caskade_core::Error::Invalid(caskade_core::tr!(
            "the game isn't installed on this computer in a folder that can be copied"
        ))
    })))
}

/// Copies going on to and from this device.
pub async fn devices_transfers(
    transfers: &Transfers,
    devices: &Devices,
) -> Result<Vec<TransferView>, CommandError> {
    crate::permissions::enforce("devices_transfers", serde_json::Value::default)?;
    let status = devices.status().await;
    let now = caskade_core::sync::clock::now_ms();
    Ok(transfers
        .transfers()
        .into_iter()
        .map(|record| {
            let device = status.devices.iter().find(|d| d.id == record.device_id);
            let link = device.map_or(DeviceLink::Offline, |d| d.link);
            let elapsed = u64::try_from(now.saturating_sub(record.started_at_ms))
                .unwrap_or(0)
                .max(1);
            TransferView {
                id: record.id,
                title: record.title,
                device_name: device.map(|d| d.name.clone()).unwrap_or_default(),
                direction: if record.incoming {
                    TransferDirection::Incoming
                } else {
                    TransferDirection::Outgoing
                },
                done_bytes: record.done,
                total_bytes: record.total,
                bytes_per_second: record.done.saturating_mul(1000) / elapsed,
                link,
                state: if link == DeviceLink::Relay {
                    TransferState::WaitingDirect
                } else {
                    TransferState::Running
                },
            }
        })
        .collect())
}

/// Takes the games other devices send here (#235): copied at once when
/// copies are accepted without asking, otherwise queued for confirmation
/// in `requests`, which tells `events`.
pub fn take_sends(
    events: crate::events::Events,
    transfers: &Transfers,
    devices: Devices,
    settings: caskade_core::settings::SettingsService,
    install: Option<InstallService>,
    requests: Option<super::install_requests::InstallRequests>,
) {
    transfers.on_send(move |request| {
        let (events, devices, settings) = (events.clone(), devices.clone(), settings.clone());
        let (install, requests) = (install.clone(), requests.clone());
        crate::runtime::spawn(async move {
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
            let accept = settings
                .read_or_default::<caskade_core::sync::policy::SyncSettings>("core.sync")
                .await
                .accept_copies;
            if accept
                && let Some(install) = &install
                && install.copy_from_device(send.target(), None).await.is_ok()
            {
                return;
            }
            if let Some(requests) = &requests
                && let Err(error) = requests.queue_peer(&events, send)
            {
                tracing::warn!(%error, "a copy from another device wasn't queued");
            }
        });
    });
}
