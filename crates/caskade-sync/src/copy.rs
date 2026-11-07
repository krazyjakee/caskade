//! Game folders between paired devices (#233), over iroh-blobs.
//!
//! The sending device adds every file of a game's folder to a [`CopyStore`],
//! referenced where it is (only a BLAKE3 outboard is stored, about 1/1000th
//! of the file), as a collection named by the files' relative paths. The
//! receiving device fetches that collection into a store of its own: every
//! chunk is checked against the BLAKE3 tree as it arrives, and what arrived
//! stays verified in the store, so a dropped transfer resumes where it
//! stopped. Then it exports the files to a folder of its choosing.
//!
//! Who gets what is the [`CopyGate`]'s: the provider intercepts every
//! connection and request, and serves only a paired device the collections
//! offered to that device. iroh-blobs' own access is otherwise anyone's
//! who knows a hash.
//!
//! Game files never pass through a relay: the receiver pulls them over a
//! [`DirectDialer`], an endpoint with this device's key and no relay
//! transport at all, to the sender's direct addresses. Whatever paths iroh
//! would choose, that connection has no relay path to choose. A device behind
//! a router needs a way made for it first: [`crate::punch`].

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt as _;
pub use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, TransportAddr, endpoint::presets};
use iroh_blobs::api::blobs::{AddPathOptions, ExportMode, ExportOptions, ImportMode};
use iroh_blobs::api::remote::GetProgressItem;
use iroh_blobs::format::collection::Collection;
use iroh_blobs::protocol::ChunkRanges;
use iroh_blobs::provider::events::{
    AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage, RequestMode,
    RequestUpdate, ThrottleMode,
};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::store::fs::options::Options as StoreOptions;
use iroh_blobs::store::{GcConfig, ProtectOutcome};
use iroh_blobs::{BlobFormat, Hash, HashAndFormat};

pub use iroh_blobs::BlobsProtocol;

use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::punch::{OpenPort, addrs_of, bind_again, on_port};

/// Files one collection may hold.
pub const MAX_FILES: usize = 1_000_000;

fn store_error(error: impl std::fmt::Display) -> Error {
    Error::Copy {
        reason: error.to_string(),
    }
}

/// What an offer holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    /// The collection's root hash, as hex.
    pub root: String,
    pub size: u64,
    pub files: u64,
}

/// A folder's relative path as a collection name: `/`-separated, never
/// absolute, never leaving the folder.
fn name_of(relative: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Where a collection's file `name` goes under `target`, or `None` when the
/// name would leave it (an absolute path, `..`, an empty or odd segment). A
/// sender's names are a stranger's input here.
pub fn place(target: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.len() > 4096 || name.contains('\\') || name.contains('\0') {
        return None;
    }
    let mut path = target.to_path_buf();
    for segment in name.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            return None;
        }
        path.push(segment);
    }
    path.starts_with(target).then_some(path)
}

/// Every regular file under `folder`, with its relative path. Symbolic
/// links are left out: they may point outside the folder. So is the game's
/// own `.caskade`: this device's install record and its Wine or Proton prefix,
/// with the saves in it. The receiving device makes its own.
fn files_of(folder: &Path) -> Result<Vec<(PathBuf, String, u64)>> {
    let mut found = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(store_error)?;
        for entry in entries {
            let entry = entry.map_err(store_error)?;
            let kind = entry.file_type().map_err(store_error)?;
            let path = entry.path();
            if kind.is_dir() {
                if dir != folder || entry.file_name() != ".caskade" {
                    stack.push(path);
                }
            } else if kind.is_file() {
                let Some(name) = path.strip_prefix(folder).ok().and_then(name_of) else {
                    continue;
                };
                let size = entry.metadata().map_err(store_error)?.len();
                found.push((path, name, size));
                if found.len() > MAX_FILES {
                    return Err(store_error("the folder has too many files"));
                }
            }
        }
    }
    found.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(found)
}

/// How often a store collects what no tag keeps (#278).
const COLLECT_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// A blob store for copies: on the sending device the outboards of the
/// folders it offers, on the receiving one what has arrived so far.
///
/// Everything it keeps is kept by a tag: an offer's, a fetch's from its
/// start, or a temporary one while it's added. What no tag keeps is left
/// over, above all a collection exported by reference whose files have
/// since moved on (an install took them, then was removed), and is
/// collected every few seconds. A fetch of such a collection waits for that
/// first ([`CopyStore::settle`]), since the store would read the moved
/// files as its own.
#[derive(Debug, Clone)]
pub struct CopyStore {
    store: FsStore,
    /// Counts collections started.
    collections: Arc<tokio::sync::watch::Sender<u64>>,
    every: std::time::Duration,
}

impl CopyStore {
    /// Opens (or makes) a store in `dir`.
    pub async fn open(dir: &Path) -> Result<Self> {
        Self::open_collecting(dir, COLLECT_EVERY).await
    }

    /// [`open`](Self::open), collecting what no tag keeps every `every`.
    pub async fn open_collecting(dir: &Path, every: std::time::Duration) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(store_error)?;
        let collections = Arc::new(tokio::sync::watch::channel(0u64).0);
        let started = collections.clone();
        let mut options = StoreOptions::new(dir);
        options.gc = Some(GcConfig {
            interval: every,
            add_protected: Some(Arc::new(move |_: &mut HashSet<Hash>| {
                started.send_modify(|count| *count += 1);
                Box::pin(async { ProtectOutcome::Continue })
            })),
        });
        let store = FsStore::load_with_opts(dir.join("blobs.db"), options)
            .await
            .map_err(store_error)?;
        Ok(Self {
            store,
            collections,
            every,
        })
    }

    /// Waits until what an earlier transfer of `content` left behind is
    /// collected: data that no tag keeps, which may be files exported by
    /// reference that have since moved (#278). Returns at once when there's
    /// none, or a tag keeps it; otherwise after one whole collection that
    /// began after this call (two starts), or a while.
    pub(crate) async fn settle(&self, content: HashAndFormat) -> Result<()> {
        // Only the store's records: opening the leftovers would read moved
        // files as the store's own, which spoils them for good.
        let status = self
            .store
            .blobs()
            .status(content.hash)
            .await
            .map_err(store_error)?;
        if matches!(status, iroh_blobs::api::blobs::BlobStatus::NotFound) {
            return Ok(());
        }
        let mut tags = self.store.tags().list().await.map_err(store_error)?;
        while let Some(tag) = tags.next().await {
            if tag.map_err(store_error)?.hash_and_format() == content {
                return Ok(());
            }
        }
        let mut started = self.collections.subscribe();
        let first = *started.borrow_and_update();
        let collected = async {
            while *started.borrow_and_update() < first + 2 {
                if started.changed().await.is_err() {
                    break;
                }
            }
        };
        let patience = self.every * 3 + std::time::Duration::from_secs(5);
        if tokio::time::timeout(patience, collected).await.is_err() {
            tracing::warn!("leftovers of an earlier transfer weren't collected in time");
        }
        Ok(())
    }

    /// Adds every file under `folder`, referenced in place, as a collection
    /// kept under `tag` until [`CopyStore::release`]. `progress` gets the
    /// bytes added so far and the total.
    pub async fn offer(
        &self,
        folder: &Path,
        tag: &str,
        progress: impl Fn(u64, u64) + Send,
    ) -> Result<Offered> {
        let folder = folder.to_path_buf();
        let files = tokio::task::spawn_blocking(move || files_of(&folder))
            .await
            .map_err(store_error)??;
        let total: u64 = files.iter().map(|(_, _, size)| size).sum();
        let mut collection = Collection::default();
        let mut done = 0;
        let mut kept = Vec::with_capacity(files.len());
        for (path, name, size) in files {
            let tag = self
                .store
                .blobs()
                .add_path_with_opts(AddPathOptions {
                    path,
                    format: BlobFormat::Raw,
                    mode: ImportMode::TryReference,
                })
                .temp_tag()
                .await
                .map_err(store_error)?;
            collection.push(name, tag.hash());
            kept.push(tag);
            done += size;
            progress(done, total);
        }
        let count = collection.len() as u64;
        let root = collection.store(&self.store).await.map_err(store_error)?;
        self.store
            .tags()
            .set(tag, root.hash_and_format())
            .await
            .map_err(store_error)?;
        drop(kept);
        Ok(Offered {
            root: root.hash().to_hex(),
            size: total,
            files: count,
        })
    }

    /// The iroh-blobs store, for this crate's other transfers (shares'
    /// swarm, #249).
    pub(crate) fn blobs(&self) -> &FsStore {
        &self.store
    }

    /// Bytes of the collection `root` this store has, verified.
    pub async fn local_bytes(&self, root: &str) -> Result<u64> {
        let root = parse(root)?;
        self.settle(HashAndFormat::hash_seq(root)).await?;
        let local = self
            .store
            .remote()
            .local(HashAndFormat::hash_seq(root))
            .await
            .map_err(store_error)?;
        Ok(local.local_bytes())
    }

    /// Fetches what's missing of the collection `root` over `connection`,
    /// keeping it under `tag`. `progress` gets the bytes received in this
    /// fetch. Returns once everything is here and verified.
    pub async fn fetch(
        &self,
        connection: Connection,
        root: &str,
        tag: &str,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<()> {
        let root = parse(root)?;
        let content = HashAndFormat::hash_seq(root);
        self.settle(content).await?;
        // Kept from the start, so a cancelled fetch keeps what arrived.
        self.store
            .tags()
            .set(tag, content)
            .await
            .map_err(store_error)?;
        let mut items = self.store.remote().fetch(connection, content).stream();
        while let Some(item) = items.next().await {
            match item {
                GetProgressItem::Progress(bytes) => progress(bytes),
                GetProgressItem::Done(_) => return Ok(()),
                GetProgressItem::Error(error) => return Err(store_error(error)),
            }
        }
        Err(store_error("the transfer ended early"))
    }

    /// The files of the collection `root`: names and sizes.
    pub async fn files(&self, root: &str) -> Result<Vec<(String, u64)>> {
        let collection = Collection::load(parse(root)?, self.store.as_ref())
            .await
            .map_err(store_error)?;
        let mut files = Vec::new();
        for (name, hash) in collection.iter() {
            let size = self
                .store
                .remote()
                .local(HashAndFormat::raw(*hash))
                .await
                .map_err(store_error)?
                .local_bytes();
            files.push((name.clone(), size));
        }
        Ok(files)
    }

    /// Writes the collection `root`'s files under `target`, moving them out
    /// of the store where it can (the store then refers to them there).
    pub async fn export(&self, root: &str, target: &Path) -> Result<()> {
        self.export_as(root, target, ExportMode::TryReference).await
    }

    /// [`export`](Self::export), copying the files: the store keeps its own,
    /// to go on serving them (a share's recipient that helps others, #249).
    pub async fn export_copy(&self, root: &str, target: &Path) -> Result<()> {
        self.export_as(root, target, ExportMode::Copy).await
    }

    async fn export_as(&self, root: &str, target: &Path, mode: ExportMode) -> Result<()> {
        let collection = Collection::load(parse(root)?, self.store.as_ref())
            .await
            .map_err(store_error)?;
        for (name, hash) in collection.iter() {
            let path =
                place(target, name).ok_or_else(|| store_error("a file name leaves the folder"))?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(store_error)?;
            }
            self.store
                .blobs()
                .export_with_opts(ExportOptions {
                    hash: *hash,
                    mode,
                    target: path.clone(),
                })
                .finish()
                .await
                .map_err(store_error)?;
            #[cfg(unix)]
            mark_program(&path).map_err(store_error)?;
        }
        Ok(())
    }

    /// Lets go of what `tag` kept; the store cleans it up later.
    pub async fn release(&self, tag: &str) -> Result<()> {
        self.store.tags().delete(tag).await.map_err(store_error)?;
        Ok(())
    }

    /// Serves what `gate` allows, to whom it allows it.
    pub fn protocol(&self, gate: &CopyGate) -> BlobsProtocol {
        let mask = EventMask {
            connected: ConnectMode::Intercept,
            get: RequestMode::InterceptLog,
            get_many: RequestMode::Disabled,
            push: RequestMode::Disabled,
            observe: ObserveMode::None,
            // Each chunk asks first, so a revoked collection stops at once,
            // even part-way through a transfer.
            throttle: ThrottleMode::Intercept,
        };
        let (events, receiver) = EventSender::channel(64, mask);
        tokio::spawn(gate.clone().serve(receiver));
        BlobsProtocol::new(&self.store, Some(events))
    }
}

/// [`BlobsProtocol`] for a router that closes while the store goes on. Its own
/// handler shuts the whole store down with the router, and the store outlives
/// every endpoint and every transfer's port.
#[derive(Debug, Clone)]
pub(crate) struct OnRouter(pub BlobsProtocol);

impl ProtocolHandler for OnRouter {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        self.0.accept(connection).await
    }
}

fn parse(root: &str) -> Result<Hash> {
    Hash::from_str(root).map_err(|_| store_error("that isn't a collection"))
}

/// Which paired device may fetch which collection, and how much each has
/// been sent. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct CopyGate {
    inner: Arc<Mutex<GateState>>,
}

#[derive(Debug, Default)]
struct GateState {
    allowed: HashMap<EndpointId, HashSet<Hash>>,
    connections: HashMap<u64, EndpointId>,
    /// What each request in flight fetches: (connection, request) → root.
    requests: HashMap<(u64, u64), Hash>,
    /// Bytes sent per device and collection.
    sent: HashMap<(EndpointId, Hash), u64>,
}

impl CopyGate {
    /// Lets `peer` fetch the collection `root`.
    pub fn allow(&self, peer: EndpointId, root: &str) -> Result<()> {
        let root = parse(root)?;
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.allowed.entry(peer).or_default().insert(root);
        Ok(())
    }

    /// Stops `peer` fetching `root`.
    pub fn revoke(&self, peer: EndpointId, root: &str) {
        let Ok(root) = parse(root) else { return };
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(roots) = state.allowed.get_mut(&peer) {
            roots.remove(&root);
            if roots.is_empty() {
                state.allowed.remove(&peer);
            }
        }
        state.sent.remove(&(peer, root));
    }

    /// Bytes sent to `peer` of `root` so far, by the provider's count. Bytes
    /// a dropped connection took with it were sent, and are sent again when
    /// the copy resumes, so this can pass the collection's size.
    pub fn sent(&self, peer: EndpointId, root: &str) -> u64 {
        let Ok(root) = parse(root) else { return 0 };
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.sent.get(&(peer, root)).copied().unwrap_or(0)
    }

    fn admits(&self, connection: u64, peer: Option<EndpointId>) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match peer {
            Some(peer) if state.allowed.contains_key(&peer) => {
                state.connections.insert(connection, peer);
                true
            }
            _ => false,
        }
    }

    fn grants(&self, connection: u64, root: Hash) -> Option<EndpointId> {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let peer = *state.connections.get(&connection)?;
        state
            .allowed
            .get(&peer)
            .is_some_and(|roots| roots.contains(&root))
            .then_some(peer)
    }

    /// Whether the request in flight may go on: its device still has its
    /// collection.
    fn still_allowed(&self, connection: u64, request: u64) -> bool {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let (Some(peer), Some(root)) = (
            state.connections.get(&connection),
            state.requests.get(&(connection, request)),
        ) else {
            return false;
        };
        state
            .allowed
            .get(peer)
            .is_some_and(|roots| roots.contains(root))
    }

    fn count(&self, peer: EndpointId, root: Hash, bytes: u64) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *state.sent.entry((peer, root)).or_default() += bytes;
    }

    async fn serve(self, mut messages: tokio::sync::mpsc::Receiver<ProviderMessage>) {
        while let Some(message) = messages.recv().await {
            match message {
                ProviderMessage::ClientConnected(message) => {
                    let ok = self.admits(message.inner.connection_id, message.inner.endpoint_id);
                    let answer = if ok {
                        Ok(())
                    } else {
                        Err(AbortReason::Permission)
                    };
                    message.tx.send(answer).await.ok();
                }
                ProviderMessage::ConnectionClosed(message) => {
                    let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    let connection = message.inner.connection_id;
                    state.connections.remove(&connection);
                    state.requests.retain(|(c, _), _| *c != connection);
                }
                ProviderMessage::GetRequestReceived(message) => {
                    let root = message.inner.request.hash;
                    let peer = self.grants(message.inner.connection_id, root);
                    if peer.is_some() {
                        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                        state.requests.insert(
                            (message.inner.connection_id, message.inner.request_id),
                            root,
                        );
                    }
                    let answer = if peer.is_some() {
                        Ok(())
                    } else {
                        Err(AbortReason::Permission)
                    };
                    message.tx.send(answer).await.ok();
                    if let Some(peer) = peer {
                        let gate = self.clone();
                        let asked = message.inner.request.ranges.clone();
                        let mut updates = message.rx;
                        tokio::spawn(async move {
                            let (mut wanted, mut current) = (ChunkRanges::empty(), 0);
                            while let Ok(Some(update)) = updates.recv().await {
                                match update {
                                    RequestUpdate::Started(started) => {
                                        wanted = asked[started.index].clone();
                                        current = 0;
                                    }
                                    RequestUpdate::Progress(progress) => {
                                        let sent = wanted_below(&wanted, progress.end_offset);
                                        gate.count(peer, root, sent.saturating_sub(current));
                                        current = sent;
                                    }
                                    RequestUpdate::Completed(_) | RequestUpdate::Aborted(_) => {}
                                }
                            }
                        });
                    }
                }
                // Anything else is refused: a copy only ever fetches.
                ProviderMessage::GetManyRequestReceived(message) => {
                    message.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::PushRequestReceived(message) => {
                    message.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::ObserveRequestReceived(message) => {
                    message.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::Throttle(message) => {
                    let ok =
                        self.still_allowed(message.inner.connection_id, message.inner.request_id);
                    let answer = if ok {
                        Ok(())
                    } else {
                        Err(AbortReason::Permission)
                    };
                    message.tx.send(answer).await.ok();
                }
                _ => {}
            }
        }
    }
}

/// Bytes of `wanted` below offset `end`. Progress gives a place in a file,
/// and a resumed or shared one is only asked for in parts: the rest was
/// never sent by this request, however far along the file it has got.
fn wanted_below(wanted: &ChunkRanges, end: u64) -> u64 {
    wanted
        .boundaries()
        .chunks(2)
        .map(|range| {
            let start = range[0].to_bytes().min(end);
            let stop = range.get(1).map_or(end, |stop| stop.to_bytes().min(end));
            stop - start
        })
        .sum()
}

/// Whether a file starting with `start` runs by itself: a script, or a
/// Linux or macOS program.
#[cfg(unix)]
fn is_program(start: &[u8]) -> bool {
    const STARTS: [&[u8]; 7] = [
        b"#!",
        b"\x7fELF",
        b"\xfe\xed\xfa\xce",
        b"\xfe\xed\xfa\xcf",
        b"\xce\xfa\xed\xfe",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
    ];
    STARTS.iter().any(|program| start.starts_with(program))
}

/// Lets a program at `path` run. A collection holds names and bytes, not
/// modes, and a sender on Windows has none to give, so what a file is says
/// whether it needs it: without this, a Linux game arrives and can't start.
#[cfg(unix)]
fn mark_program(path: &Path) -> std::io::Result<()> {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt as _;
    let mut start = [0; 4];
    let read = std::fs::File::open(path)?.read(&mut start)?;
    if is_program(&start[..read]) {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// An endpoint with this device's key and no relay transport: game files
/// travel over it, so none can pass through a relay.
#[derive(Debug, Clone)]
pub struct DirectDialer {
    endpoint: Endpoint,
}

impl DirectDialer {
    /// Binds one for `identity` (a throwaway key when `None`, for tests), on
    /// loopback only when `loopback`.
    pub async fn bind(identity: Option<&Identity>, loopback: bool) -> Result<Self> {
        let mut builder = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .clear_address_lookup();
        if loopback {
            builder = builder
                .clear_ip_transports()
                .bind_addr(std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::LOCALHOST,
                    0,
                ))
                .expect("127.0.0.1 is a valid bind address");
        }
        if let Some(identity) = identity {
            builder = builder.secret_key(identity.secret_key().clone());
        }
        let endpoint = builder
            .bind()
            .await
            .map_err(|source| Error::Bind { source })?;
        Ok(Self { endpoint })
    }

    /// Binds one for `identity` on `open`'s port, for a transfer through two
    /// routers (`crate::punch`), and what to tell the other device.
    pub async fn bind_on(identity: &Identity, open: &OpenPort) -> Result<(Self, Vec<SocketAddr>)> {
        let endpoint = bind_again(|| on_port(identity, open.port)).await?;
        let addrs = addrs_of(&endpoint, open).await;
        Ok((Self { endpoint }, addrs))
    }

    /// Its endpoint id: this device's, unless it was bound for a test.
    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Bytes it took off the wire, overhead included: what really moved,
    /// which a provider's count can't say (it counts a resumed file from its
    /// start).
    #[cfg(feature = "test-utils")]
    pub fn received(&self) -> u64 {
        self.endpoint.metrics().socket.recv_data_ipv4.get()
    }

    /// Connects to `peer` for iroh-blobs at its direct `addrs` only.
    pub async fn connect(&self, peer: EndpointId, addrs: &[SocketAddr]) -> Result<Connection> {
        if addrs.is_empty() {
            return Err(Error::Copy {
                reason: "no direct address for the other device".into(),
            });
        }
        let addr =
            EndpointAddr::from_parts(peer, addrs.iter().map(|addr| TransportAddr::Ip(*addr)));
        self.endpoint
            .connect(addr, iroh_blobs::ALPN)
            .await
            .map_err(|error| Error::Copy {
                reason: error.to_string(),
            })
    }

    /// Closes it.
    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}
