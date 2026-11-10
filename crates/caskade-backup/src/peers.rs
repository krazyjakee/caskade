//! Backups kept on another of the user's devices (#243), so losing a drive
//! doesn't lose the library, without a cloud.
//!
//! **Sending.** With `core.backups.keepOnDevice` naming a paired device,
//! every exchange with it offers this device's newest verified backups (as
//! many as the automatic set keeps), in the blob exchange's first-round note
//! (`sync::blobs`). The other device asks for the ones it lacks, and they
//! travel as `backup` blobs on the granted sync session. A backup made while
//! that device is away is offered again when it comes back, until it has it.
//!
//! **Keeping.** The holder keeps each device's backups apart from its own,
//! content-addressed in `<data>/peer-backups/`, with an index naming each:
//! the device it came from, its file name, size and when it was made. It
//! keeps the newest `core.backups.keep` per device.
//!
//! **Restoring.** Every exchange also lists what this device holds. A fresh
//! install paired with the holder sees its backups of every device
//! ([`PeerBackups::held`]), fetches one ([`PeerBackups::fetch`]), and
//! restores it through the existing staged restore.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::schedule::BackupSettings;
use caskade_core::saves::versions::{BlobStore, hash_file};
use caskade_core::settings::SettingsService;
use caskade_core::sync::blobs::{BlobKind, BlobWriter};
use caskade_core::sync::policy::Category;
use caskade_core::{Error, Result};

const SETTINGS: &str = "core.backups";
const INDEX: &str = "index.json";

/// A backup one device holds of another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HeldBackup {
    /// The device it's a backup of.
    pub origin: String,
    /// That device's name, as the holder knows it.
    pub origin_name: String,
    /// `sha256:…` of the archive.
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Offer {
    hash: String,
    name: String,
    size: u64,
    created_ms: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Note {
    #[serde(default)]
    offers: Vec<Offer>,
    #[serde(default)]
    held: Vec<HeldBackup>,
}

/// Backups sent to and kept for paired devices. Cheap to clone.
#[derive(Clone)]
pub struct PeerBackups {
    inner: Arc<Inner>,
}

struct Inner {
    settings: SettingsService,
    /// Where this device's own backups go when the setting names no folder.
    default_destination: PathBuf,
    store: BlobStore,
    dir: PathBuf,
    /// Own backups by path: size, modification time and hash.
    hashes: Mutex<HashMap<PathBuf, (u64, SystemTime, String)>>,
    /// What each device offered this one to keep, by hash.
    offered: Mutex<HashMap<String, (String, Offer)>>,
    /// What each paired device holds, as it last said.
    listed: Mutex<HashMap<String, Vec<HeldBackup>>>,
    /// Backups asked for to restore, by hash: asked for in every exchange
    /// until they arrive, and kept from collection while this runs, so the
    /// restore can read them. A fetch that gives up takes its hash back.
    requested: Mutex<HashSet<String>>,
    arrived: tokio::sync::Notify,
    /// Paired devices, for their names.
    devices: std::sync::OnceLock<caskade_core::devices::Devices>,
    index_lock: tokio::sync::Mutex<()>,
}

impl PeerBackups {
    /// `dir` is `<data>/peer-backups`; `default_destination` is where this
    /// device's own backups go when `core.backups` names no folder
    /// (`Backups::default_destination`).
    pub fn new(
        settings: SettingsService,
        dir: impl Into<PathBuf>,
        default_destination: impl Into<PathBuf>,
    ) -> Self {
        let dir = dir.into();
        Self {
            inner: Arc::new(Inner {
                settings,
                default_destination: default_destination.into(),
                store: BlobStore::new(dir.join("blobs")),
                dir,
                hashes: Mutex::default(),
                offered: Mutex::default(),
                listed: Mutex::default(),
                requested: Mutex::default(),
                arrived: tokio::sync::Notify::new(),
                devices: std::sync::OnceLock::new(),
                index_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Names paired devices in what this device holds for them.
    pub fn set_devices(&self, devices: caskade_core::devices::Devices) {
        let _ = self.inner.devices.set(devices);
    }

    async fn name_of(&self, device: &str) -> String {
        match self.inner.devices.get() {
            Some(devices) => devices
                .status()
                .await
                .devices
                .into_iter()
                .find(|paired| paired.id == device)
                .map(|paired| paired.name)
                .unwrap_or_default(),
            None => String::new(),
        }
    }

    async fn settings(&self) -> BackupSettings {
        self.inner.settings.read_or_default(SETTINGS).await
    }

    /// This device's newest verified backups, as many as the automatic set
    /// keeps, newest first.
    async fn own(&self) -> Vec<(PathBuf, Offer)> {
        let settings = self.settings().await;
        let keep = usize::try_from(settings.keep.max(1)).unwrap_or(usize::MAX);
        let destination = if settings.destination.trim().is_empty() {
            self.inner.default_destination.clone()
        } else {
            PathBuf::from(settings.destination)
        };
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let Ok(entries) = std::fs::read_dir(&destination) else {
                return Vec::new();
            };
            let mut files: Vec<(SystemTime, PathBuf, u64)> = entries
                .flatten()
                .filter(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with("caskade-backup-") && name.ends_with(".zip")
                })
                .filter_map(|entry| {
                    let meta = entry.metadata().ok().filter(|meta| meta.is_file())?;
                    Some((meta.modified().ok()?, entry.path(), meta.len()))
                })
                .collect();
            files.sort_by_key(|file| std::cmp::Reverse(file.0));
            files.truncate(keep);
            let mut hashes = inner.hashes.lock().unwrap_or_else(|e| e.into_inner());
            files
                .into_iter()
                .filter_map(|(modified, path, size)| {
                    let hash = match hashes.get(&path) {
                        Some((known, at, hash)) if *known == size && *at == modified => {
                            hash.clone()
                        }
                        _ => {
                            let (hash, _) = hash_file(&path).ok()?;
                            hashes.insert(path.clone(), (size, modified, hash.clone()));
                            hash
                        }
                    };
                    let created_ms = modified
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|age| i64::try_from(age.as_millis()).unwrap_or(i64::MAX))
                        .unwrap_or_default();
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some((
                        path,
                        Offer {
                            hash,
                            name,
                            size,
                            created_ms,
                        },
                    ))
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    fn index_path(&self) -> PathBuf {
        self.inner.dir.join(INDEX)
    }

    /// What this device holds for others.
    fn index(&self) -> Vec<HeldBackup> {
        std::fs::read(self.index_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write_index(&self, held: &[HeldBackup]) -> Result<()> {
        std::fs::create_dir_all(&self.inner.dir)?;
        let partial = self.inner.dir.join(".index.partial");
        std::fs::write(&partial, serde_json::to_vec(held)?)?;
        std::fs::rename(partial, self.index_path())?;
        Ok(())
    }

    /// What this device holds of other devices, newest first.
    pub fn holding(&self) -> Vec<HeldBackup> {
        let mut held = self.index();
        held.sort_by_key(|backup| std::cmp::Reverse(backup.created_ms));
        held
    }

    /// Backups paired devices hold, as each last said, by holder; newest
    /// first.
    pub fn held(&self) -> BTreeMap<String, Vec<HeldBackup>> {
        self.inner
            .listed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(holder, held)| {
                let mut held = held.clone();
                held.sort_by_key(|backup| std::cmp::Reverse(backup.created_ms));
                (holder.clone(), held)
            })
            .collect()
    }

    /// Fetches the backup `hash` from whichever paired device holds it,
    /// waiting up to `within` for it to arrive. Returns its path here, ready
    /// to restore.
    ///
    /// It's asked for from the call, before the future is first polled: an
    /// exchange started after this returns asks for it.
    pub fn fetch(
        &self,
        hash: &str,
        within: Duration,
    ) -> impl Future<Output = Result<PathBuf>> + Send + 'static {
        let request = Request::new(self.clone(), hash);
        async move { request.wait(within).await }
    }

    /// Keeps an arrived backup: a device's own, under its origin, pruned to
    /// the newest it should keep.
    async fn kept(&self, hash: &str) -> Result<()> {
        // One arrival at a time: two exchanges with a device can run at once
        // and both bring the same backup. The second must find the first's
        // index, or it would collect the blob as unasked for.
        let _held = self.inner.index_lock.lock().await;
        let offered = self
            .inner
            .offered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(hash);
        let requested = self
            .inner
            .requested
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(hash);
        if let Some((origin, offer)) = offered {
            let keep = usize::try_from(self.settings().await.keep.max(1)).unwrap_or(usize::MAX);
            let origin_name = self.name_of(&origin).await;
            let mut held = self.index();
            held.retain(|known| known.hash != hash);
            held.push(HeldBackup {
                origin: origin.clone(),
                origin_name,
                hash: offer.hash,
                name: offer.name,
                size: offer.size,
                created_ms: offer.created_ms,
            });
            // The newest `keep` of each device.
            held.sort_by_key(|backup| std::cmp::Reverse(backup.created_ms));
            let mut counts: HashMap<String, usize> = HashMap::new();
            held.retain(|backup| {
                let count = counts.entry(backup.origin.clone()).or_default();
                *count += 1;
                *count <= keep
            });
            self.write_index(&held)?;
            self.collect(&held);
        } else if !requested {
            // Neither offered nor asked for: not kept.
            self.collect(&self.index());
        }
        self.inner.arrived.notify_waiters();
        Ok(())
    }

    /// Drops stored blobs nothing names: neither held nor asked for.
    fn collect(&self, held: &[HeldBackup]) {
        let mut keep: HashSet<String> = held.iter().map(|backup| backup.hash.clone()).collect();
        keep.extend(
            self.inner
                .requested
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned(),
        );
        if let Err(error) = self.inner.store.collect_garbage(&keep) {
            tracing::warn!(%error, "old backups from other devices weren't removed");
        }
    }

    /// The path of a backup this device can serve: its own, or one it holds.
    async fn serving(&self, hash: &str) -> Option<PathBuf> {
        if self.inner.store.has(hash) {
            return self.inner.store.path(hash);
        }
        self.own()
            .await
            .into_iter()
            .find(|(_, offer)| offer.hash == hash)
            .map(|(path, _)| path)
    }
}

#[async_trait]
impl BlobKind for PeerBackups {
    fn name(&self) -> &'static str {
        "backup"
    }

    fn category(&self) -> Category {
        Category::Library
    }

    fn own_setting(&self) -> bool {
        true
    }

    async fn announce(&self, peer: &str) -> Option<Value> {
        let offers = if self.settings().await.keep_on_device.as_deref() == Some(peer) {
            self.own()
                .await
                .into_iter()
                .map(|(_, offer)| offer)
                .collect()
        } else {
            Vec::new()
        };
        let held = self.holding();
        if offers.is_empty() && held.is_empty() {
            return None;
        }
        serde_json::to_value(Note { offers, held }).ok()
    }

    async fn heard(&self, peer: &str, note: Value) {
        let Ok(note) = serde_json::from_value::<Note>(note) else {
            return;
        };
        self.inner
            .listed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(peer.to_owned(), note.held);
        let mut offered = self.inner.offered.lock().unwrap_or_else(|e| e.into_inner());
        offered.retain(|_, (origin, _)| origin != peer);
        for offer in note.offers {
            if !self.inner.store.has(&offer.hash) {
                offered.insert(offer.hash.clone(), (peer.to_owned(), offer));
            }
        }
    }

    async fn wanted(&self) -> Result<Vec<String>> {
        let mut wanted: Vec<String> = self
            .inner
            .offered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        wanted.extend(
            self.inner
                .requested
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned(),
        );
        wanted.retain(|hash| !self.inner.store.has(hash));
        Ok(wanted)
    }

    async fn has(&self, hash: &str) -> bool {
        self.serving(hash).await.is_some()
    }

    async fn read(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let path = self.serving(hash).await.ok_or_else(|| {
            Error::Invalid(caskade_core::tr!("That backup isn't on this device."))
        })?;
        tokio::task::spawn_blocking(move || read_at(&path, offset, len))
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))?
    }

    async fn receive(&self, hash: &str) -> Result<Box<dyn BlobWriter>> {
        Ok(Box::new(Receiving {
            incoming: Some(self.inner.store.receive(hash)?),
            hash: hash.to_owned(),
            backups: self.clone(),
        }))
    }
}

/// A fetch's request: asked for while it waits, taken back if it gives up
/// before the backup arrived.
struct Request {
    backups: PeerBackups,
    hash: String,
}

impl Request {
    fn new(backups: PeerBackups, hash: &str) -> Self {
        backups
            .inner
            .requested
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(hash.to_owned());
        Self {
            backups,
            hash: hash.to_owned(),
        }
    }

    async fn wait(&self, within: Duration) -> Result<PathBuf> {
        let inner = &self.backups.inner;
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let arrived = inner.arrived.notified();
            if inner.store.has(&self.hash) {
                break;
            }
            if tokio::time::timeout_at(deadline, arrived).await.is_err() {
                return Err(Error::Invalid(caskade_core::tr!(
                    "The backup didn't arrive from the other device. Make sure it's on and in reach, then try again."
                )));
            }
        }
        inner.store.path(&self.hash).ok_or_else(|| {
            Error::Invalid(caskade_core::tr!("The backup that arrived can't be read."))
        })
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        let inner = &self.backups.inner;
        if !inner.store.has(&self.hash) {
            inner
                .requested
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.hash);
        }
    }
}

fn read_at(path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = Vec::with_capacity(len);
    file.take(len as u64).read_to_end(&mut buffer)?;
    Ok(buffer)
}

struct Receiving {
    incoming: Option<caskade_core::saves::versions::Incoming>,
    hash: String,
    backups: PeerBackups,
}

#[async_trait]
impl BlobWriter for Receiving {
    async fn write(&mut self, chunk: &[u8]) -> Result<()> {
        match self.incoming.as_mut() {
            Some(incoming) => incoming.write(chunk),
            None => Ok(()),
        }
    }

    async fn finish(mut self: Box<Self>) -> Result<bool> {
        let Some(incoming) = self.incoming.take() else {
            return Ok(false);
        };
        let kept = tokio::task::spawn_blocking(move || incoming.finish())
            .await
            .map_err(|error| Error::Io(std::io::Error::other(error)))??;
        if kept {
            self.backups.kept(&self.hash).await?;
        }
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caskade_core::db::LibraryStore;
    use caskade_core::saves::versions::hash_bytes;

    async fn peers() -> (PeerBackups, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsService::new(LibraryStore::in_memory().await.unwrap());
        let peers = PeerBackups::new(settings, temp.path().join("peer-backups"), temp.path());
        (peers, temp)
    }

    async fn arrive(peers: &PeerBackups, hash: &str, bytes: &[u8]) -> Box<dyn BlobWriter> {
        let mut writer = peers.receive(hash).await.unwrap();
        writer.write(bytes).await.unwrap();
        writer
    }

    #[tokio::test]
    async fn a_fetched_backup_that_arrives_twice_stays() {
        let (peers, _temp) = peers().await;
        let bytes = b"a backup";
        let hash = hash_bytes(bytes);
        let fetching = peers.fetch(&hash, Duration::from_secs(5));
        assert_eq!(peers.wanted().await.unwrap(), vec![hash.clone()]);

        // Two exchanges with the holder at once both bring it.
        let (first, second) = (
            arrive(&peers, &hash, bytes).await,
            arrive(&peers, &hash, bytes).await,
        );
        assert!(first.finish().await.unwrap());
        assert!(second.finish().await.unwrap());

        let path = fetching.await.unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(peers.wanted().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_fetch_that_gives_up_stops_asking() {
        let (peers, _temp) = peers().await;
        let hash = hash_bytes(b"never comes");
        let fetching = peers.fetch(&hash, Duration::from_millis(10));
        assert_eq!(peers.wanted().await.unwrap(), vec![hash]);
        assert!(fetching.await.is_err());
        assert!(peers.wanted().await.unwrap().is_empty());
    }
}
