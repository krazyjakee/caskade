//! Backups kept on the user's other devices (#243): what each paired
//! device holds, and fetching one here to restore it through the usual
//! inspect and staged restore (`backup_inspect`, `backup_restore`).
use caskade_backup::peers::{HeldBackup, PeerBackups};
use caskade_core::{devices::Devices, sync::SyncService};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::Duration;

use crate::error::{CommandError, report};

/// How long a fetch waits for the backup to arrive.
const FETCH: Duration = Duration::from_secs(10 * 60);

/// The backups one paired device keeps, of this device and others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBackups {
    pub device_id: String,
    pub device_name: String,
    /// Newest first.
    pub backups: Vec<HeldBackup>,
}

/// What paired devices keep, as each last said when they synced; nothing
/// before devices are ready.
pub async fn backups_on_devices(
    peers: Option<&PeerBackups>,
    devices: Option<&Devices>,
) -> Result<Vec<DeviceBackups>, CommandError> {
    let (Some(peers), Some(devices)) = (peers, devices) else {
        return Ok(Vec::new());
    };
    let paired = devices.status().await.devices;
    Ok(peers
        .held()
        .into_iter()
        .filter(|(_, backups)| !backups.is_empty())
        .filter_map(|(device_id, backups)| {
            let device = paired.iter().find(|device| device.id == device_id)?;
            Some(DeviceBackups {
                device_name: device.name.clone(),
                device_id,
                backups,
            })
        })
        .collect())
}

/// Fetches the backup `hash` from the paired device that keeps it, and
/// returns where it is here, to inspect and restore.
pub async fn backup_fetch_from_device(
    peers: Option<&PeerBackups>,
    sync: Option<&SyncService>,
    hash: &str,
) -> Result<String, CommandError> {
    let (Some(peers), Some(sync)) = (peers, sync) else {
        return Err(report(caskade_core::Error::Invalid(caskade_core::tr!(
            "Devices aren't ready on this device yet."
        ))));
    };
    let holders: Vec<String> = peers
        .held()
        .into_iter()
        .filter(|(_, backups)| backups.iter().any(|backup| backup.hash == hash))
        .map(|(device, _)| device)
        .collect();
    let fetching = peers.fetch(hash, FETCH);
    // Ask the holders to sync now, and again while the fetch waits.
    let nudging = async {
        loop {
            for holder in &holders {
                sync.sync_now(holder);
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    };
    let path = tokio::select! {
        path = fetching => path.map_err(report)?,
        () = nudging => unreachable!("the nudges never end"),
    };
    Ok(path.to_string_lossy().into_owned())
}
