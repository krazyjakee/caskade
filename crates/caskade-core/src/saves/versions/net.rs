//! Save versions between devices (#239): their files as `sync::blobs`, and
//! the short wait for other devices before a launch.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::blobs::Incoming;
use super::{RemoteSaves, SaveVersions};
use crate::devices::{DeviceLink, Devices};
use crate::sync::SyncService;
use crate::sync::blobs::{BlobKind, BlobWriter};
use crate::sync::policy::Category;
use crate::{Error, Result};

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
        tokio::task::spawn_blocking(move || blobs.read(&hash, offset, len))
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))?
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
        tokio::task::spawn_blocking(move || incoming.finish())
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))?
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

#[async_trait]
impl RemoteSaves for SyncRefresh {
    async fn refresh(&self, within: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + within;
        let status = self.devices.status().await;
        // Each device in reach, and when this device asked it to sync. One
        // already syncing may have started before the newest save existed:
        // it's asked again once it's done.
        // One known to be offline (nothing heard from it for minutes, though
        // library sync reaches each device every minute) isn't waited for:
        // dialing it would hold the launch until the deadline. It still
        // counts as not reached. One that went out of reach since fails to
        // connect, or runs into the deadline: either way the launch goes
        // ahead.
        let mut all = true;
        let mut asked: Vec<(String, Option<i64>)> = Vec::new();
        for device in &status.devices {
            if device.link == DeviceLink::Offline {
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
                        *requested = Some(crate::sync::clock::now_ms());
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
