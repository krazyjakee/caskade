//! Copies of installed games between paired devices (#233).
//!
//! The receiving device asks for a release on a `copy` session (a paired
//! device's own grant opens it). The sending device finds the release's
//! folder, checks it can be copied (a managed install, or a folder of its
//! own: not a launcher's), and offers it as an iroh-blobs collection
//! (`caskade_sync::copy`), saying how far it got while it reads the files.
//! The offer names the collection's root hash, its size and file count, the
//! game's title and its program relative to the folder; never a path.
//!
//! The receiver checks there's room, then pulls the collection over a
//! [`caskade_sync::copy::DirectDialer`] to the sender's direct addresses: no
//! relay can carry game files. Behind routers no plain dial gets through, and
//! the two make a way first (`caskade_sync::punch`). While only a relay
//! reaches the sender, or it can't be reached, the copy waits (`FetchFailure::Waiting`) and what
//! arrived stays verified in a store in Caskade's cache, so it resumes where
//! it stopped. The files are then exported to
//! `<cache>/copies/receive/incoming/<job>/` (the installer takes a folder
//! outside the storage root) and installed from there
//! (`InstallService::copy_from_device`).
//!
//! The sender keeps an offer for [`OFFER_LIFETIME`], so a copy that comes
//! back resumes without the files being read again; while it's being sent,
//! the release can't be moved or uninstalled.
//!
//! An emulated release goes as its ROM files, never its folder, which may be
//! a ROM scanner's holding every game of the platform (#279, [`rom`]): the
//! offer describes the platform and emulator profile, and the receiver
//! places the files as a game of that platform instead of installing them.
//! A web game goes as its offline copy (#76, [`web`]): the offer names its
//! page, and the receiver makes the bundle that game's offline copy. One
//! played from its page alone has nothing to copy, and is refused.

mod receive;
pub mod rom;
mod send;
pub mod web;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use caskade_sync::EndpointId;
use caskade_sync::copy::{CopyGate, CopyStore, Offered};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::db::LibraryStore;
use crate::devices::{ConnectionPurpose, Devices};
use crate::{Error, Result};

use send::Serve;

/// How long an offered folder stays offered to a device that may come back
/// for the rest.
pub const OFFER_LIFETIME: Duration = Duration::from_secs(6 * 60 * 60);
/// The longest frame read on a copy session.
const MAX_LINE: usize = 64 * 1024;
/// How long the receiver waits for each of the sender's frames: reading a
/// large game's files takes a while, and it says how far it got meanwhile.
const FRAME_WAIT: Duration = Duration::from_secs(300);
/// How long a direct connection gets to open.
const DIAL_WAIT: Duration = Duration::from_secs(15);
/// How long hole punching gets to move a session off a relay.
pub const PUNCH_WAIT: Duration = Duration::from_secs(5);
/// How long a dial with no hole made for it gets. Past that the other
/// device is behind a router, and the two make one (`caskade_sync::punch`).
pub const PLAIN_WAIT: Duration = Duration::from_secs(3);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    /// Receiver: send me this release (its entry id on the sender).
    Request { entry: String },
    /// Sender: reading the files.
    Preparing { done: u64, total: u64 },
    /// Sender: here it is.
    Offer {
        root: String,
        size: u64,
        files: u64,
        title: String,
        /// The program, relative to the folder and `/`-separated.
        launch: Option<String>,
        /// A Windows build.
        windows: bool,
        /// An emulated release: its ROM files, platform and emulator
        /// profile (#279).
        #[serde(default)]
        rom: Option<rom::RomCopy>,
        /// A web game's offline copy: its page (#76).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        web: Option<web::WebCopy>,
    },
    /// Sender: not this one, and why.
    Refused { reason: String },
    /// Receiver: everything arrived.
    Done,
    /// Both ways: no plain dial reaches you, so I opened a port for this copy
    /// at `addrs`. Empty: this side couldn't.
    Punch { addrs: Vec<std::net::SocketAddr> },
    /// A device that has the game: copy it from me (#235). `game` is its id
    /// there.
    Send {
        entry: String,
        game: String,
        title: String,
        size: Option<u64>,
    },
    /// The device took the request: it's waiting to be confirmed there, or
    /// the copy started.
    Queued,
}

/// Another device asks this one to copy a game from it (#235).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRequest {
    pub device_id: String,
    /// The release on that device.
    pub entry_id: String,
    /// The game here it is, when the two devices have matched it.
    pub game_id: Option<String>,
    pub title: String,
    pub size: Option<u64>,
}

/// A transfer as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRecord {
    pub id: String,
    pub device_id: String,
    pub title: String,
    pub incoming: bool,
    pub done: u64,
    pub total: u64,
    pub started_at_ms: i64,
}

struct Offer {
    offered: Offered,
    tag: String,
    what: Described,
    peers: HashSet<EndpointId>,
}

/// What an offer says about the release it sends.
#[derive(Clone)]
struct Described {
    title: String,
    /// The program, relative to the folder and `/`-separated.
    launch: Option<String>,
    windows: bool,
    rom: Option<rom::RomCopy>,
    web: Option<web::WebCopy>,
}

struct Outgoing {
    entry: String,
    title: String,
    root: String,
    size: u64,
    started_at_ms: i64,
}

struct Incoming {
    device_id: String,
    title: String,
    done: u64,
    total: u64,
    started_at_ms: i64,
}

/// Copies to and from this device. Cheap to clone.
#[derive(Clone)]
pub struct Transfers {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    devices: Devices,
    gate: CopyGate,
    sending: CopyStore,
    offers: Mutex<HashMap<String, Offer>>,
    outgoing: Mutex<HashMap<(EndpointId, String), Outgoing>>,
    incoming: Mutex<HashMap<String, Incoming>>,
    /// What's arriving, and where it's put together.
    receiving: CopyStore,
    incoming_dir: PathBuf,
    changed: broadcast::Sender<()>,
    /// Where requests to copy a game here go (#235).
    on_send: std::sync::OnceLock<Arc<dyn Fn(SendRequest) + Send + Sync>>,
    /// Installs the emulator a received ROM plays with (#279).
    emulators: std::sync::OnceLock<Arc<dyn rom::EmulatorInstaller>>,
    /// Where an emulated release's ROM files are put together to send.
    outgoing_dir: PathBuf,
}

fn sync_error(error: caskade_sync::Error) -> Error {
    Error::Invalid(error.to_string())
}

/// A folder name for a title: letters, digits, spaces and a few marks.
pub fn folder_name(title: &str) -> String {
    let name: String = title
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\''))
        .collect();
    let name = name.trim().trim_matches('.').to_owned();
    if name.is_empty() { "Game".into() } else { name }
}

impl Transfers {
    /// Copies for `devices`, offering from a store under `cache`. Call
    /// before the devices' endpoint starts: it serves the offers.
    pub async fn new(store: LibraryStore, devices: Devices, cache: &Path) -> Result<Self> {
        let sending = CopyStore::open(&cache.join("copies").join("send"))
            .await
            .map_err(sync_error)?;
        let receiving = CopyStore::open(&cache.join("copies").join("receive").join("store"))
            .await
            .map_err(sync_error)?;
        let incoming_dir = cache.join("copies").join("receive").join("incoming");
        let outgoing_dir = cache.join("copies").join("send").join("roms");
        let gate = CopyGate::default();
        devices.set_copy_provider(sending.protocol(&gate));
        let transfers = Self {
            inner: Arc::new(Inner {
                store,
                devices: devices.clone(),
                gate,
                sending,
                offers: Mutex::default(),
                outgoing: Mutex::default(),
                incoming: Mutex::default(),
                receiving,
                incoming_dir,
                changed: broadcast::channel(16).0,
                on_send: std::sync::OnceLock::new(),
                emulators: std::sync::OnceLock::new(),
                outgoing_dir,
            }),
        };
        devices.set_copy_handler(Arc::new(Serve(Arc::downgrade(&transfers.inner))));
        Ok(transfers)
    }

    /// Installs the emulator a received ROM plays with when this device has
    /// none for it (emulator setup).
    pub fn set_emulator_installer(&self, installer: Arc<dyn rom::EmulatorInstaller>) {
        let _ = self.inner.emulators.set(installer);
    }

    pub fn emulator_installer(&self) -> Option<Arc<dyn rom::EmulatorInstaller>> {
        self.inner.emulators.get().cloned()
    }

    /// Where another device's request to copy a game here goes: the app
    /// queues it for confirmation, or starts it when copies are accepted
    /// without asking (#235).
    pub fn on_send(&self, handler: impl Fn(SendRequest) + Send + Sync + 'static) {
        let _ = self.inner.on_send.set(Arc::new(handler));
    }

    /// Asks `device_id` to copy release `entry_id` (installed here) from
    /// this device (#235). Answers once the device took the request.
    pub async fn send_to(&self, device_id: &str, entry_id: &str) -> Result<()> {
        let entry = self.inner.store.get_entry(entry_id).await?.ok_or_else(|| {
            Error::Invalid(crate::tr!("that game isn't installed on this device"))
        })?;
        let (_, title, _, _) = self.source(entry_id).await?;
        let mut session = self
            .inner
            .devices
            .connect(device_id, ConnectionPurpose::Copy)
            .await?;
        session
            .writer()
            .write(&Message::Send {
                entry: entry_id.to_owned(),
                game: entry.game_id.clone(),
                title,
                size: entry.size,
            })
            .await
            .map_err(sync_error)?;
        let answer = tokio::time::timeout(FRAME_WAIT, session.reader().read(MAX_LINE)).await;
        session.end().await;
        match answer {
            Ok(Ok(Message::Queued)) => Ok(()),
            Ok(Ok(Message::Refused { reason })) => Err(Error::Invalid(reason)),
            _ => Err(Error::Invalid(crate::tr!(
                "the other device didn't take the copy"
            ))),
        }
    }

    /// A signal on every change to [`Transfers::transfers`].
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.inner.changed.subscribe()
    }

    /// Copies going on, both ways.
    pub fn transfers(&self) -> Vec<TransferRecord> {
        let mut records: Vec<TransferRecord> = crate::lock(&self.inner.outgoing)
            .iter()
            .map(|((peer, _), outgoing)| TransferRecord {
                id: format!("send:{peer}:{}", outgoing.entry),
                device_id: peer.to_string(),
                title: outgoing.title.clone(),
                incoming: false,
                done: self
                    .inner
                    .gate
                    .sent(*peer, &outgoing.root)
                    .min(outgoing.size),
                total: outgoing.size,
                started_at_ms: outgoing.started_at_ms,
            })
            .collect();
        records.extend(
            crate::lock(&self.inner.incoming)
                .iter()
                .map(|(job, incoming)| TransferRecord {
                    id: job.clone(),
                    device_id: incoming.device_id.clone(),
                    title: incoming.title.clone(),
                    incoming: true,
                    done: incoming.done,
                    total: incoming.total,
                    started_at_ms: incoming.started_at_ms,
                }),
        );
        records.sort_by_key(|record| record.started_at_ms);
        records
    }

    fn changed(&self) {
        let _ = self.inner.changed.send(());
    }
}

/// What one-off shares (#246) build on: the same store, gate and direct
/// dialer as copies. Shares fetch into the send store, so this device's one
/// provider serves what they have to the share's other recipients, gated
/// per collection like copies.
impl Transfers {
    /// The store copies are offered from (and shares fetched into).
    pub fn send_store(&self) -> CopyStore {
        self.inner.sending.clone()
    }

    /// Offers `folder`'s files, kept under `tag` until released.
    pub async fn offer_folder(
        &self,
        folder: &Path,
        tag: &str,
        progress: impl Fn(u64, u64) + Send,
    ) -> Result<Offered> {
        self.inner
            .sending
            .offer(folder, tag, progress)
            .await
            .map_err(sync_error)
    }

    /// Lets `peer` fetch the collection `root`.
    pub fn allow(&self, peer: EndpointId, root: &str) -> Result<()> {
        self.inner.gate.allow(peer, root).map_err(sync_error)
    }

    /// Stops `peer` fetching `root`, a transfer in flight included.
    pub fn revoke(&self, peer: EndpointId, root: &str) {
        self.inner.gate.revoke(peer, root);
    }

    /// Bytes of `root` sent to `peer` so far.
    pub fn sent(&self, peer: EndpointId, root: &str) -> u64 {
        self.inner.gate.sent(peer, root)
    }

    /// Fetches the collection `root` from `peer` at its direct `addrs`
    /// (never through a relay) into the send store, under `tag`.
    pub async fn fetch_direct(
        &self,
        peer: EndpointId,
        addrs: &[std::net::SocketAddr],
        root: &str,
        tag: &str,
        progress: impl FnMut(u64) + Send,
    ) -> Result<()> {
        let dialer = self.inner.devices.direct_dialer().await?;
        let connection = tokio::time::timeout(DIAL_WAIT, dialer.connect(peer, addrs))
            .await
            .map_err(|_| Error::Invalid(crate::tr!("couldn't reach the other device directly")))?
            .map_err(sync_error)?;
        self.inner
            .sending
            .fetch(connection, root, tag, progress)
            .await
            .map_err(sync_error)
    }

    /// Writes the collection `root`'s files under `target`, from the send
    /// store.
    pub async fn export(&self, root: &str, target: &Path) -> Result<()> {
        self.inner
            .sending
            .export(root, target)
            .await
            .map_err(sync_error)
    }

    /// [`export`](Self::export), copying the files: the send store keeps
    /// its own, to serve them on (#249).
    pub async fn export_copy(&self, root: &str, target: &Path) -> Result<()> {
        self.inner
            .sending
            .export_copy(root, target)
            .await
            .map_err(sync_error)
    }

    /// Lets go of what `tag` kept in the send store.
    pub async fn release(&self, tag: &str) -> Result<()> {
        self.inner.sending.release(tag).await.map_err(sync_error)
    }
}

/// A size for a message: "3.2 GB", "820 MB".
pub fn readable_size(bytes: u64) -> String {
    const GB: f64 = 1e9;
    const MB: f64 = 1e6;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", (bytes / MB).max(1.0))
    }
}

/// How many more bytes a copy of `size`, `have` of them already in
/// `incoming`, needs to arrive there and be installed under `root`: what's
/// still to come in the cache and the install in the root, twice the game
/// where both are on one disk. 0 when there's room.
pub fn shortfall(incoming: &Path, root: &Path, size: u64, have: u64) -> u64 {
    let _ = std::fs::create_dir_all(incoming);
    let cache_free = fs2::available_space(incoming).unwrap_or(u64::MAX);
    let root_free = fs2::available_space(root).unwrap_or(u64::MAX);
    let to_come = size.saturating_sub(have);
    let same_disk =
        fs2::total_space(incoming).ok() == fs2::total_space(root).ok() && cache_free == root_free;
    if same_disk {
        to_come.saturating_add(size).saturating_sub(root_free)
    } else {
        to_come
            .saturating_sub(cache_free)
            .max(size.saturating_sub(root_free))
    }
}

#[cfg(test)]
mod size_tests {
    use super::readable_size;

    #[test]
    fn a_size_reads_as_megabytes_or_gigabytes() {
        assert_eq!(readable_size(3_485_671_424), "3.5 GB");
        assert_eq!(readable_size(820_000_000), "820 MB");
        assert_eq!(readable_size(10), "1 MB");
    }
}
