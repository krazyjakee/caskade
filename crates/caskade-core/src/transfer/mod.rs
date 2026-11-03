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
//! relay can carry game files. While only a relay reaches the sender, or it
//! can't be reached, the copy waits (`FetchFailure::Waiting`) and what
//! arrived stays verified in a store in Caskade's cache, so it resumes where
//! it stopped. The files are then exported to
//! `<cache>/copies/receive/incoming/<job>/` (the installer takes a folder
//! outside the storage root) and installed from there
//! (`InstallService::copy_from_device`).
//!
//! The sender keeps an offer for [`OFFER_LIFETIME`], so a copy that comes
//! back resumes without the files being read again; while it's being sent,
//! the release can't be moved or uninstalled.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::copy::{CopyGate, CopyStore, Offered};
use caskade_sync::{EndpointId, Session, Sessions};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::db::LibraryStore;
use crate::devices::{ConnectionPurpose, Devices};
use crate::install::{FetchFailure, Fetched, PeerCopyTarget, PeerFetcher, PeerProgress};
use crate::model::{GameRuntime, InstallLocation, InstallState, LaunchSpec};
use crate::{Error, Result};

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
    },
    /// Sender: not this one, and why.
    Refused { reason: String },
    /// Receiver: everything arrived.
    Done,
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
    title: String,
    launch: Option<String>,
    windows: bool,
    peers: HashSet<EndpointId>,
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
}

fn sync_error(error: caskade_sync::Error) -> Error {
    Error::Invalid(error.to_string())
}

/// A folder name for a title: letters, digits, spaces and a few marks.
fn folder_name(title: &str) -> String {
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
            }),
        };
        devices.set_copy_handler(Arc::new(Serve(Arc::downgrade(&transfers.inner))));
        Ok(transfers)
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
        let mut records: Vec<TransferRecord> = self
            .inner
            .outgoing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
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
            self.inner
                .incoming
                .lock()
                .unwrap_or_else(|e| e.into_inner())
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

    /// Lets go of what `tag` kept in the send store.
    pub async fn release(&self, tag: &str) -> Result<()> {
        self.inner.sending.release(tag).await.map_err(sync_error)
    }
}

/// `copy` sessions from paired devices: they ask for a release.
struct Serve(Weak<Inner>);

#[async_trait]
impl Sessions for Serve {
    async fn opened(&self, session: Session) {
        let Some(inner) = self.0.upgrade() else {
            return session.end().await;
        };
        Transfers { inner }.serve(session).await;
    }
}

impl Transfers {
    async fn serve(&self, mut session: Session) {
        let peer = session.peer();
        let entry = match session.reader().read(MAX_LINE).await {
            Ok(Message::Request { entry }) => entry,
            Ok(Message::Send {
                entry,
                game,
                title,
                size,
            }) => return self.requested(session, entry, game, title, size).await,
            _ => return session.end().await,
        };
        match self.offer(&entry, peer, &mut session).await {
            Ok(()) => {
                // Until the device says it has everything, or goes.
                loop {
                    match tokio::time::timeout(OFFER_LIFETIME, session.reader().read(MAX_LINE))
                        .await
                    {
                        Ok(Ok(Message::Done)) => {
                            self.withdraw(&entry, peer).await;
                            break;
                        }
                        Ok(Ok(_)) => continue,
                        // Gone: the offer stays a while, to resume.
                        _ => break,
                    }
                }
                session.end().await;
            }
            Err(reason) => {
                let _ = session
                    .writer()
                    .write(&Message::Refused {
                        reason: reason.to_string(),
                    })
                    .await;
                session.end().await;
            }
        }
    }

    /// Another device asks this one to copy a game from it: queued for
    /// confirmation here, or started (the app decides, `on_send`).
    async fn requested(
        &self,
        mut session: Session,
        entry: String,
        game: String,
        title: String,
        size: Option<u64>,
    ) {
        let peer = session.peer().to_string();
        let answer = match self.inner.on_send.get() {
            Some(handler) => {
                let game_id = self
                    .inner
                    .store
                    .sync_mapped_game(&peer, &game)
                    .await
                    .ok()
                    .flatten();
                handler(SendRequest {
                    device_id: peer,
                    entry_id: entry,
                    game_id,
                    title,
                    size,
                });
                Message::Queued
            }
            None => Message::Refused {
                reason: crate::tr!("this device doesn't take copies"),
            },
        };
        let _ = session.writer().write(&answer).await;
        session.end().await;
    }

    /// The folder release `entry` is in, its program relative to it, and
    /// whether it's a Windows build; refused when it can't be copied.
    async fn source(&self, entry: &str) -> Result<(PathBuf, String, Option<String>, bool)> {
        let not_here = || Error::Invalid(crate::tr!("that game isn't installed on this device"));
        let entry = self
            .inner
            .store
            .get_entry(entry)
            .await?
            .ok_or_else(not_here)?;
        let source = entry.source_id.as_str();
        if source.starts_with("peer:")
            || source.starts_with("store:")
            || source.starts_with("stream:")
        {
            return Err(not_here());
        }
        if !matches!(entry.state, InstallState::Installed) {
            return Err(Error::Invalid(crate::tr!(
                "the game isn't fully installed on this device"
            )));
        }
        let folder = match &entry.location {
            Some(InstallLocation::Managed { root_id, rel_dir }) => {
                let root = self.inner.store.get_root(root_id).await?.ok_or_else(|| {
                    Error::Invalid(crate::tr!("the game's storage isn't available"))
                })?;
                root.path.join(rel_dir)
            }
            Some(InstallLocation::External { path })
                if matches!(
                    entry.launch,
                    Some(LaunchSpec::Executable { .. } | LaunchSpec::Emulator { .. })
                ) =>
            {
                if path.is_dir() {
                    path.clone()
                } else {
                    path.parent().map(Path::to_path_buf).ok_or_else(not_here)?
                }
            }
            Some(InstallLocation::External { .. }) => {
                return Err(Error::Invalid(crate::tr!(
                    "it starts through its store's launcher here; install it from the store there"
                )));
            }
            None => return Err(not_here()),
        };
        if !folder.is_dir() {
            return Err(Error::Invalid(crate::tr!("the game's folder isn't there")));
        }
        let launch = match &entry.launch {
            Some(LaunchSpec::Executable { path, .. }) => {
                let relative = if path.is_absolute() {
                    path.strip_prefix(&folder).ok().map(Path::to_path_buf)
                } else {
                    Some(path.clone())
                };
                relative.and_then(|path| {
                    let parts: Vec<String> = path
                        .components()
                        .filter_map(|part| match part {
                            std::path::Component::Normal(part) => part.to_str().map(str::to_owned),
                            _ => None,
                        })
                        .collect();
                    (!parts.is_empty()).then(|| parts.join("/"))
                })
            }
            _ => None,
        };
        let title = self
            .inner
            .store
            .game(&entry.game_id)
            .await?
            .map(|game| game.title)
            .unwrap_or_default();
        let windows = matches!(entry.runtime, GameRuntime::Windows { .. });
        Ok((folder, title, launch, windows))
    }

    /// Offers release `entry` to `peer` on `session`: reads its files (or
    /// takes an earlier offer of it), lets `peer` fetch them, and says so.
    async fn offer(&self, entry: &str, peer: EndpointId, session: &mut Session) -> Result<()> {
        let earlier = {
            let offers = self.inner.offers.lock().unwrap_or_else(|e| e.into_inner());
            offers.get(entry).map(|offer| {
                (
                    offer.offered.clone(),
                    offer.title.clone(),
                    offer.launch.clone(),
                    offer.windows,
                )
            })
        };
        let (offered, title, launch, windows) = match earlier {
            Some(earlier) => earlier,
            None => {
                let (folder, title, launch, windows) = self.source(entry).await?;
                let tag = format!("copy/{entry}");
                let (tx, mut rx) = mpsc::unbounded_channel();
                let reading = self.inner.sending.offer(&folder, &tag, move |done, total| {
                    let _ = tx.send((done, total));
                });
                tokio::pin!(reading);
                let mut last = std::time::Instant::now();
                let offered = loop {
                    tokio::select! {
                        offered = &mut reading => break offered.map_err(sync_error)?,
                        Some((done, total)) = rx.recv() => {
                            if last.elapsed() >= Duration::from_millis(500) {
                                last = std::time::Instant::now();
                                let _ = session
                                    .writer()
                                    .write(&Message::Preparing { done, total })
                                    .await;
                            }
                        }
                    }
                };
                self.inner
                    .offers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(
                        entry.to_owned(),
                        Offer {
                            offered: offered.clone(),
                            tag: tag.clone(),
                            title: title.clone(),
                            launch: launch.clone(),
                            windows,
                            peers: HashSet::new(),
                        },
                    );
                (offered, title, launch, windows)
            }
        };
        self.inner
            .gate
            .allow(peer, &offered.root)
            .map_err(sync_error)?;
        if let Some(offer) = self
            .inner
            .offers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(entry)
        {
            offer.peers.insert(peer);
        }
        self.inner
            .outgoing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry((peer, entry.to_owned()))
            .or_insert_with(|| Outgoing {
                entry: entry.to_owned(),
                title: title.clone(),
                root: offered.root.clone(),
                size: offered.size,
                started_at_ms: crate::now_ms(),
            });
        self.changed();
        // Offered for a while, then withdrawn whatever happened.
        let this = self.clone();
        let (key, until) = (entry.to_owned(), OFFER_LIFETIME);
        tokio::spawn(async move {
            tokio::time::sleep(until).await;
            this.withdraw(&key, peer).await;
        });
        session
            .writer()
            .write(&Message::Offer {
                root: offered.root,
                size: offered.size,
                files: offered.files,
                title,
                launch,
                windows,
            })
            .await
            .map_err(sync_error)
    }

    /// `peer` has release `entry`, or its offer ran out: withdrawn, and let
    /// go once nobody else is fetching it.
    async fn withdraw(&self, entry: &str, peer: EndpointId) {
        let released = {
            let mut offers = self.inner.offers.lock().unwrap_or_else(|e| e.into_inner());
            let Some(offer) = offers.get_mut(entry) else {
                return;
            };
            self.inner.gate.revoke(peer, &offer.offered.root);
            offer.peers.remove(&peer);
            if offer.peers.is_empty() {
                offers.remove(entry).map(|offer| offer.tag)
            } else {
                None
            }
        };
        self.inner
            .outgoing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(peer, entry.to_owned()));
        if let Some(tag) = released {
            let _ = self.inner.sending.release(&tag).await;
        }
        self.changed();
    }
}

fn waiting(why: String) -> FetchFailure {
    FetchFailure::Waiting(why)
}

#[async_trait]
impl PeerFetcher for Transfers {
    async fn fetch(
        &self,
        target: &PeerCopyTarget,
        root: &Path,
        job: &str,
        cancel: caskade_installer::exec::CancellationToken,
        progress: Arc<dyn Fn(PeerProgress) + Send + Sync>,
    ) -> std::result::Result<Fetched, FetchFailure> {
        let device = target.device_name.as_str();
        let peer: EndpointId = target
            .device_id
            .parse()
            .map_err(|_| FetchFailure::Failed(Error::Invalid(crate::tr!("that isn't a device"))))?;
        let unreachable = || crate::tr!("Waiting for {device} to be in reach", device = device);
        let mut session = self
            .inner
            .devices
            .connect(&target.device_id, ConnectionPurpose::Copy)
            .await
            .map_err(|_| waiting(unreachable()))?;
        session
            .writer()
            .write(&Message::Request {
                entry: target.entry_id.clone(),
            })
            .await
            .map_err(|_| waiting(unreachable()))?;
        let (root_hash, size, title, launch) = loop {
            let frame = tokio::time::timeout(FRAME_WAIT, session.reader().read(MAX_LINE)).await;
            match frame {
                Ok(Ok(Message::Preparing { done, total })) => {
                    progress(PeerProgress::Preparing { done, total });
                }
                Ok(Ok(Message::Offer {
                    root,
                    size,
                    title,
                    launch,
                    ..
                })) => break (root, size, title, launch),
                Ok(Ok(Message::Refused { reason })) => {
                    return Err(FetchFailure::Failed(Error::Invalid(reason)));
                }
                Ok(Ok(_)) => continue,
                _ => return Err(waiting(unreachable())),
            }
        };
        let store = self.inner.receiving.clone();
        let have = store.local_bytes(&root_hash).await.unwrap_or(0).min(size);
        // What's still to come in the cache, and the install in the root:
        // twice the game where both are on one disk.
        let _ = std::fs::create_dir_all(&self.inner.incoming_dir);
        let cache_free = fs2::available_space(&self.inner.incoming_dir).unwrap_or(u64::MAX);
        let root_free = fs2::available_space(root).unwrap_or(u64::MAX);
        let to_come = size.saturating_sub(have);
        let same_disk = fs2::total_space(&self.inner.incoming_dir).ok()
            == fs2::total_space(root).ok()
            && cache_free == root_free;
        let short = if same_disk {
            to_come.saturating_add(size).saturating_sub(root_free)
        } else {
            to_come
                .saturating_sub(cache_free)
                .max(size.saturating_sub(root_free))
        };
        if short > 0 {
            return Err(FetchFailure::Failed(Error::Invalid(crate::tr!(
                "there isn't enough room for the copy: {short} more bytes are needed",
                short = short
            ))));
        }
        let addrs = session.direct_addrs();
        if addrs.is_empty() {
            return Err(waiting(crate::tr!(
                "Only a relay reaches {device}: the copy waits for a direct connection",
                device = device
            )));
        }
        let dialer = self
            .inner
            .devices
            .direct_dialer()
            .await
            .map_err(FetchFailure::Failed)?;
        let connection = match tokio::time::timeout(DIAL_WAIT, dialer.connect(peer, &addrs)).await {
            Ok(Ok(connection)) => connection,
            _ => {
                return Err(waiting(crate::tr!(
                    "{device} can't be reached directly yet: the copy waits for a direct connection",
                    device = device
                )));
            }
        };
        self.inner
            .incoming
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                job.to_owned(),
                Incoming {
                    device_id: target.device_id.clone(),
                    title: title.clone(),
                    done: have,
                    total: size,
                    started_at_ms: crate::now_ms(),
                },
            );
        self.changed();
        progress(PeerProgress::Receiving {
            done: have,
            total: size,
        });
        let tag = format!("copy/{job}");
        let this = self.clone();
        let key = job.to_owned();
        let report = progress.clone();
        let fetching = store.fetch(connection, &root_hash, &tag, move |bytes| {
            let done = have.saturating_add(bytes).min(size);
            if let Some(incoming) = this
                .inner
                .incoming
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&key)
            {
                incoming.done = done;
            }
            report(PeerProgress::Receiving { done, total: size });
        });
        let fetched = tokio::select! {
            _ = cancel.cancelled() => Err(caskade_sync::Error::Copy { reason: "cancelled".into() }),
            fetched = fetching => fetched,
        };
        let finished = |this: &Self| {
            this.inner
                .incoming
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(job);
            this.changed();
        };
        if let Err(error) = fetched {
            finished(self);
            tracing::debug!(%error, "a copy stopped; what arrived stays");
            return Err(waiting(unreachable()));
        }
        let _ = session.writer().write(&Message::Done).await;
        session.end().await;
        let staging = self.inner.incoming_dir.join(job);
        let folder = staging.join(folder_name(&title));
        let _ = std::fs::remove_dir_all(&staging);
        let exported = store.export(&root_hash, &folder).await;
        finished(self);
        exported.map_err(|error| FetchFailure::Failed(sync_error(error)))?;
        Ok(Fetched {
            folder,
            title,
            launch,
        })
    }

    async fn finished(&self, _root: &Path, job: &str) {
        let _ = tokio::fs::remove_dir_all(self.inner.incoming_dir.join(job)).await;
        let _ = self.inner.receiving.release(&format!("copy/{job}")).await;
    }

    fn sending(&self, entry_id: &str) -> bool {
        self.inner
            .outgoing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|outgoing| outgoing.entry == entry_id)
    }
}
