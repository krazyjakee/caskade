//! Save versions between devices (#239): their files as `sync::blobs`, and
//! the short wait for other devices before a launch.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::blobs::Incoming;
use super::{RemoteSaves, SaveVersions};
use crate::Result;
use crate::devices::{DeviceLink, Devices, PairedDevice};
use crate::sync::SyncService;
use crate::sync::blobs::{BlobKind, BlobWriter};
use crate::sync::policy::Category;

/// Save files as blobs: the versions' file lists, then their files.
pub struct SaveBlobs(pub SaveVersions);

#[async_trait]
impl BlobKind for SaveBlobs {
    fn name(&self) -> &'static str {
        "save"
    }

    fn category(&self) -> Category {
        Category::Saves
    }

    async fn wanted(&self) -> Result<Vec<String>> {
        self.0.wanted().await
    }

    async fn has(&self, hash: &str) -> bool {
        self.0.blobs().has(hash)
    }

    async fn read(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let blobs = self.0.blobs().clone();
        let hash = hash.to_owned();
        tokio::task::spawn_blocking(move || blobs.read(&hash, offset, len)).await?
    }

    async fn receive(&self, hash: &str) -> Result<Box<dyn BlobWriter>> {
        Ok(Box::new(Receiving(Some(self.0.blobs().receive(hash)?))))
    }
}

struct Receiving(Option<Incoming>);

#[async_trait]
impl BlobWriter for Receiving {
    async fn write(&mut self, chunk: &[u8]) -> Result<()> {
        match self.0.as_mut() {
            Some(incoming) => incoming.write(chunk),
            None => Ok(()),
        }
    }

    async fn finish(mut self: Box<Self>) -> Result<bool> {
        let Some(incoming) = self.0.take() else {
            return Ok(false);
        };
        tokio::task::spawn_blocking(move || incoming.finish()).await?
    }
}

/// Asks every paired device in reach to sync now, and waits for them.
pub struct SyncRefresh {
    sync: SyncService,
    devices: Devices,
}

impl SyncRefresh {
    pub fn new(sync: SyncService, devices: Devices) -> Arc<Self> {
        Arc::new(Self { sync, devices })
    }
}

/// Whether the check before a launch skips `device`: offline (nothing
/// heard from it for minutes, though library sync tries each device every
/// minute) and its last attempt failed, so dialing it would only hold the
/// launch until the deadline. One not tried yet (just paired, or Caskade
/// just started) is asked: it may be in reach on another network.
fn out_of_reach(device: &PairedDevice) -> bool {
    device.link == DeviceLink::Offline && device.sync_error.is_some()
}

#[async_trait]
impl RemoteSaves for SyncRefresh {
    async fn refresh(&self, within: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + within;
        let status = self.devices.status().await;
        // Each device in reach, and when this device asked it to sync. One
        // already syncing may have started before the newest save existed:
        // it's asked again once it's done.
        // One known to be out of reach isn't waited for (`out_of_reach`),
        // but still counts as not reached. One that went out of reach since
        // fails to connect, or runs into the deadline: either way the
        // launch goes ahead.
        let mut all = true;
        let mut asked: Vec<(String, Option<i64>)> = Vec::new();
        for device in &status.devices {
            if out_of_reach(device) {
                all = false;
            } else {
                asked.push((device.id.clone(), None));
            }
        }
        let begun = tokio::time::Instant::now();
        while !asked.is_empty() {
            let status = self.devices.status().await;
            // An error from before this attempt shows until it starts.
            let settled = begun.elapsed() > Duration::from_millis(500);
            let mut failed = false;
            asked.retain_mut(|(id, requested)| {
                let Some(device) = status.devices.iter().find(|device| device.id == *id) else {
                    return false;
                };
                match requested {
                    None if device.syncing => true,
                    None => {
                        *requested = Some(crate::now_ms());
                        self.sync.sync_now(id);
                        true
                    }
                    Some(at) => {
                        if device.syncing {
                            return true;
                        }
                        if device.last_sync_ms.is_some_and(|done| done >= *at) {
                            return false;
                        }
                        if settled && device.sync_error.is_some() {
                            failed = true;
                            return false;
                        }
                        true
                    }
                }
            });
            all &= !failed;
            if asked.is_empty() {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{DeviceKind, DeviceOs};

    fn device(link: DeviceLink, sync_error: Option<&str>) -> PairedDevice {
        PairedDevice {
            id: "deck".into(),
            name: "Deck".into(),
            kind: DeviceKind::default(),
            os: DeviceOs::Linux,
            link,
            addresses: Vec::new(),
            last_sync_ms: None,
            paired_at_ms: 0,
            syncing: false,
            sync_error: sync_error.map(Into::into),
        }
    }

    #[test]
    fn only_a_device_that_failed_and_reads_offline_is_skipped() {
        assert!(out_of_reach(&device(
            DeviceLink::Offline,
            Some("timed out")
        )));
        // Not tried yet (just paired, or just started): asked.
        assert!(!out_of_reach(&device(DeviceLink::Offline, None)));
        // In reach, whatever failed before.
        assert!(!out_of_reach(&device(DeviceLink::Lan, Some("timed out"))));
        assert!(!out_of_reach(&device(DeviceLink::Relay, None)));
    }
}
