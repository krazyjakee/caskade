//! This device and the devices paired with it (#226): identity, the sync
//! endpoint, pairing by code or link, and forgetting.
//!
//! The connection layer is `caskade-sync`; this service
//! keeps what it needs on disk and decides who is admitted:
//!
//! - **Identity.** Loaded from the keychain off the caller's thread by
//!   [`Devices::start`]. The last endpoint id is kept in the settings table
//!   (`sync.device`, with this device's name and kind), so a missing key after
//!   first use asks before a new identity is made ([`Devices::create_identity`]).
//! - **Paired devices** live in `paired_devices` (migration 0031) with the
//!   grants each side presents. Grants are credentials: the table is local
//!   (never synced), left out of backups, and never sent to the webview.
//! - **Admission** ([`gate`]) is in memory: a paired device with its grant, a
//!   pairing link once before it expires, and one code pairing at a time.
//! - **Only admins** pair, confirm, mint or accept links, forget, rename this
//!   device or make a new identity: pairing hands another device every
//!   account and the library. Each method checks it here, not only the UI.
//!
//! Pairing links live in memory: a restart invalidates links not used yet.

mod addresses;
mod endpoint;
mod gate;
mod pairing;
mod types;
pub use gate::ShareAdmission;
pub use pairing::inspect_link;
pub use types::{
    DeviceLink, DeviceOs, DevicesStatus, IdentityStatus, NearbyDevice, PairedDevice, Pairing,
    PairingLinkInfo, PairingLinkTarget, PairingState, ThisDevice,
};

pub use crate::device_type::DeviceKind;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use caskade_sync::pairing::accept_link;
use caskade_sync::{
    CodePairing, DeviceInfo, EndpointAddr, EndpointId, Grant, Identity, KeyStore, Link, Purpose,
    Session, Sessions, SyncEndpoint,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::accounts::{AccountService, Permission};
use crate::db::{LibraryStore, PairedDeviceRecord};
use crate::settings::SettingsService;
use crate::sync::policy::SyncSettings;
use crate::{Error, Result, now_ms};

use endpoint::{Forwarder, addr_text, parse_addr};
use gate::Gate;
use pairing::ActivePairing;

/// The OS keychain the device key lives in, and what a session is opened for.
pub use caskade_sync::{Keychain, Purpose as ConnectionPurpose, Session as DeviceSession};
/// For tests: a key store in memory, and loopback-only endpoints
/// ([`Devices::with_reach`]).
pub use caskade_sync::{MemoryKeyStore, Reach};

/// This device's last endpoint id, name and kind, in the settings table (not
/// a section: the user edits them through [`Devices`]).
pub const DEVICE_KEY: &str = "sync.device";

/// How often forgotten devices not told yet are tried again.
const FORGET_RETRY: Duration = Duration::from_secs(5 * 60);
/// How long a paired device with no open connection keeps the link of its
/// last exchange: library sync reaches each one every minute, so three
/// minutes without a failure means it's still in reach.
const LAST_LINK_FOR_MS: i64 = 3 * 60 * 1000;

/// What `sync.device` holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct DeviceRecord {
    id: Option<String>,
    name: Option<String>,
    kind: DeviceKind,
}

/// This device and its paired devices. Cheap to clone.
#[derive(Clone)]
pub struct Devices {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    accounts: AccountService,
    keys: Arc<dyn KeyStore>,
    settings: OnceLock<SettingsService>,
    /// Turns the reach the settings ask for into the one the endpoint binds:
    /// unchanged in the app; tests stay off the network with it.
    reach: Box<dyn Fn(Reach) -> Reach + Send + Sync>,
    /// The endpoint is on 127.0.0.1, as a test's is: direct dialers stay
    /// there too.
    loopback: AtomicBool,
    gate: Arc<Gate>,
    state: Mutex<State>,
    revision: watch::Sender<u64>,
    /// One bind at a time.
    binding: tokio::sync::Mutex<()>,
    /// Where library sync (#230) takes `sync` sessions.
    sync_handler: OnceLock<Arc<dyn Sessions>>,
    /// Where copies of games (#233) take `copy` sessions.
    copy_handler: OnceLock<Arc<dyn Sessions>>,
    /// Where streaming (#267) takes `stream` sessions.
    stream_handler: OnceLock<Arc<dyn Sessions>>,
    /// Where one-off shares (#246) take `share` sessions.
    share_handler: OnceLock<Arc<dyn Sessions>>,
    /// Where play requests (#234) take `play` sessions.
    play_handler: OnceLock<Arc<dyn Sessions>>,
    /// Serves the folders offered to other devices (#233), bound with the
    /// endpoint.
    copy_provider: OnceLock<caskade_sync::copy::CopyProtocol>,
    /// Pulls game files with no relay transport (#233), for the current
    /// identity.
    dialer: tokio::sync::Mutex<Option<(EndpointId, caskade_sync::copy::DirectDialer)>>,
    /// Fetches shares from several of their holders (#249), for the current
    /// identity.
    swarm: tokio::sync::Mutex<Option<(EndpointId, caskade_sync::swarm::SwarmDialer)>>,
    /// Each device's sync, as library sync reports it.
    sync: Mutex<HashMap<String, DeviceSync>>,
    /// Paired devices that came into reach (found on this network).
    reachable: tokio::sync::broadcast::Sender<String>,
}

/// How library sync with a device is going (#230).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceSync {
    pub syncing: bool,
    /// Why the last exchange failed; never a toast: a device out of reach is
    /// a normal state.
    pub error: Option<String>,
}

struct State {
    identity: IdentityStatus,
    key: Option<Identity>,
    endpoint: Option<SyncEndpoint>,
    pairing: Option<ActivePairing>,
    /// Forwards the endpoint's link and nearby changes; aborted on rebind.
    forwarders: Vec<Forwarder>,
    /// Never use relays and the port the endpoint was bound with.
    bound_with: Option<(bool, Option<u16>)>,
    /// The relays the endpoint was bound with.
    relays: Option<caskade_sync::Relays>,
}

impl Devices {
    /// The service over `store`, with the device key in `keys` (the OS
    /// keychain in the app). Nothing runs until [`Devices::start`].
    pub fn new(store: LibraryStore, accounts: AccountService, keys: Arc<dyn KeyStore>) -> Self {
        Self::build(store, accounts, keys, |network| network)
    }

    /// Binds on `reach` instead of the network, whatever the settings say:
    /// for tests (`Reach::loopback()`).
    pub fn with_reach(
        store: LibraryStore,
        accounts: AccountService,
        keys: Arc<dyn KeyStore>,
        reach: Reach,
    ) -> Self {
        Self::build(store, accounts, keys, move |_| reach.clone())
    }

    /// Binds on what `reach` makes of the reach the settings ask for, at
    /// every bind: for tests following a setting to the endpoint.
    pub fn with_reach_from(
        store: LibraryStore,
        accounts: AccountService,
        keys: Arc<dyn KeyStore>,
        reach: impl Fn(Reach) -> Reach + Send + Sync + 'static,
    ) -> Self {
        Self::build(store, accounts, keys, reach)
    }

    fn build(
        store: LibraryStore,
        accounts: AccountService,
        keys: Arc<dyn KeyStore>,
        reach: impl Fn(Reach) -> Reach + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                accounts,
                keys,
                settings: OnceLock::new(),
                reach: Box::new(reach),
                loopback: AtomicBool::new(false),
                gate: Arc::default(),
                state: Mutex::new(State {
                    identity: IdentityStatus::Starting,
                    key: None,
                    endpoint: None,
                    pairing: None,
                    forwarders: Vec::new(),
                    bound_with: None,
                    relays: None,
                }),
                revision: watch::channel(0).0,
                binding: tokio::sync::Mutex::new(()),
                sync_handler: OnceLock::new(),
                copy_handler: OnceLock::new(),
                stream_handler: OnceLock::new(),
                share_handler: OnceLock::new(),
                play_handler: OnceLock::new(),
                copy_provider: OnceLock::new(),
                dialer: tokio::sync::Mutex::new(None),
                swarm: tokio::sync::Mutex::new(None),
                sync: Mutex::default(),
                reachable: tokio::sync::broadcast::channel(32).0,
            }),
        }
    }

    /// Reads `core.sync` (registered with the built-in sections) from
    /// `settings`.
    pub fn set_settings(&self, settings: SettingsService) {
        let _ = self.inner.settings.set(settings);
    }

    /// Changes as they happen: a revision to refetch [`Devices::status`] on.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.revision.subscribe()
    }

    /// Loads the identity, binds the endpoint, and keeps it current: a
    /// settings change rebinds it, and forgotten devices are told when they
    /// can be reached. Runs until the runtime stops.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            inner.load_paired().await;
            inner.load_identity().await;
            let mut changes = inner.settings.get().map(SettingsService::subscribe);
            let mut retry = tokio::time::interval(FORGET_RETRY);
            loop {
                tokio::select! {
                    changed = async {
                        match changes.as_mut() {
                            Some(changes) => changes.recv().await.ok(),
                            None => std::future::pending().await,
                        }
                    } => {
                        // Only the connection options rebind: category
                        // choices share the section.
                        if changed.is_some_and(|changed| changed.namespace == SyncSettings::NAMESPACE) {
                            let settings = inner.settings().await;
                            let wanted = Some((settings.never_relay, settings.port));
                            if inner.lock().bound_with != wanted {
                                inner.bind().await;
                            }
                        }
                    }
                    _ = retry.tick() => inner.tell_forgotten().await,
                    _ = inner.gate.knocked() => inner.tell_forgotten().await,
                }
            }
        })
    }

    /// Closes the endpoint and the store it served. Call at exit.
    pub async fn shutdown(&self) {
        let endpoint = self.inner.lock().endpoint.take();
        if let Some(endpoint) = endpoint {
            let _ = endpoint.shutdown().await;
        }
        if let Some(provider) = self.inner.copy_provider.get() {
            provider.store().shutdown().await;
        }
    }

    /// This device, its paired devices, nearby ones and the pairing on
    /// screen. No grant or key is in it.
    pub async fn status(&self) -> DevicesStatus {
        self.inner.status().await
    }

    /// Renames this device (empty: back to the host name).
    pub async fn rename(&self, name: &str) -> Result<()> {
        self.admin()?;
        let name = caskade_sync::device::clean_name(name);
        let mut record = self.inner.record().await;
        record.name = (!name.is_empty()).then_some(name);
        self.inner.save_record(&record).await?;
        self.inner.announce().await;
        Ok(())
    }

    pub async fn set_kind(&self, kind: DeviceKind) -> Result<()> {
        self.admin()?;
        let mut record = self.inner.record().await;
        record.kind = kind;
        self.inner.save_record(&record).await?;
        self.inner.announce().await;
        Ok(())
    }

    /// Forgets a paired device: it's refused from now on, and told when it
    /// can be reached. Nothing synced is deleted, except what that device
    /// said it has installed (#232).
    pub async fn forget(&self, device_id: &str) -> Result<()> {
        self.admin()?;
        let peer = parse_id(device_id)?;
        self.inner.gate.forget(&peer, true);
        let forgotten = self
            .inner
            .store
            .forget_paired_device(device_id, now_ms())
            .await?;
        if !forgotten {
            return Err(crate::db::missing("paired device", device_id));
        }
        self.inner.store.delete_sync_peer(device_id).await?;
        self.inner.store.sync_forget_copies(device_id).await?;
        self.inner.changed();
        let inner = self.inner.clone();
        tokio::spawn(async move { inner.tell_forgotten().await });
        Ok(())
    }

    /// Makes a new identity after the keychain lost the old one, once the
    /// user agreed: paired devices must pair again.
    pub async fn create_identity(&self) -> Result<()> {
        self.admin()?;
        let identity = Identity::create(self.inner.keys.clone())
            .await
            .map_err(|_| Error::Invalid(crate::tr!("the system keychain is unavailable")))?;
        self.inner.identity_ready(identity).await;
        Ok(())
    }

    /// Reads the keychain again after it didn't answer.
    pub async fn retry_identity(&self) -> Result<()> {
        self.admin()?;
        self.inner.load_identity().await;
        Ok(())
    }

    /// Opens a connection to a paired device for `purpose`, presenting the
    /// grant it gave this one: what library sync (#230) and copies (#233)
    /// start from. Fails for a device that isn't paired, or was forgotten.
    pub async fn connect(&self, device_id: &str, purpose: Purpose) -> Result<Session> {
        let peer = parse_id(device_id)?;
        let endpoint = self.inner.endpoint()?;
        let device = self
            .inner
            .store
            .paired_devices()
            .await?
            .into_iter()
            .find(|d| d.endpoint_id == device_id && d.forgotten_at_ms.is_none())
            .ok_or_else(|| crate::db::missing("paired device", device_id))?;
        let grant: Grant = device
            .grant_out
            .parse()
            .map_err(|_| crate::db::missing("paired device", device_id))?;
        let addr = EndpointAddr::from_parts(
            peer,
            device.addresses.iter().filter_map(|addr| parse_addr(addr)),
        );
        let session = endpoint
            .connect(addr, purpose, Some(grant))
            .await
            .map_err(|error| match error {
                caskade_sync::Error::Refused => {
                    Error::Invalid(crate::tr!("the other device refused the connection"))
                }
                _ => Error::Invalid(crate::tr!("couldn't reach the other device")),
            })?;
        let addresses: Vec<String> = session.addr().addrs.iter().map(addr_text).collect();
        if !addresses.is_empty() {
            let _ = self
                .inner
                .store
                .set_paired_device_addresses(device_id, &addresses)
                .await;
        }
        Ok(session)
    }

    /// Where `sync` sessions from paired devices go: library sync (#230).
    pub fn set_sync_handler(&self, handler: Arc<dyn Sessions>) {
        let _ = self.inner.sync_handler.set(handler);
    }

    /// Where `copy` sessions from paired devices go (#233).
    pub fn set_copy_handler(&self, handler: Arc<dyn Sessions>) {
        let _ = self.inner.copy_handler.set(handler);
    }

    /// Where `stream` sessions from paired devices go (#267): a client
    /// handing its pairing code to the host.
    pub fn set_stream_handler(&self, handler: Arc<dyn Sessions>) {
        let _ = self.inner.stream_handler.set(handler);
    }

    /// Who a `share` session admits (#246): a share's grant belongs to a
    /// person, so whoever keeps the shares decides; the paired devices'
    /// grants never open one.
    pub fn set_share_admission(&self, admission: ShareAdmission) {
        self.inner.gate.set_share_admission(admission);
    }

    /// Where `play` sessions from paired devices go (#234).
    pub fn set_play_handler(&self, handler: Arc<dyn Sessions>) {
        let _ = self.inner.play_handler.set(handler);
    }

    /// Where `share` sessions go (#246).
    pub fn set_share_handler(&self, handler: Arc<dyn Sessions>) {
        let _ = self.inner.share_handler.set(handler);
    }

    /// Opens a connection for `purpose` to a device that isn't paired (a
    /// share link's), at `addr`, presenting `grant` (#246). The device
    /// turning it down is [`Error::Forbidden`] (a stopped or expired share:
    /// the one refusal); anything else, reaching it included, is
    /// [`Error::Invalid`], worth trying again.
    pub async fn connect_to(
        &self,
        addr: EndpointAddr,
        purpose: Purpose,
        grant: Option<Grant>,
    ) -> Result<Session> {
        let endpoint = self.inner.endpoint()?;
        endpoint
            .connect(addr, purpose, grant)
            .await
            .map_err(|error| match error {
                caskade_sync::Error::Refused => {
                    Error::Forbidden(crate::tr!("the other device turned the connection down"))
                }
                error => Error::Invalid(error.to_string()),
            })
    }

    /// Serves folders offered to other devices (#233). Set before the
    /// endpoint starts: its protocols are fixed when it binds.
    pub fn set_copy_provider(&self, provider: caskade_sync::copy::CopyProtocol) {
        let _ = self.inner.copy_provider.set(provider);
    }

    /// Records how sync with a device is going, for the status.
    pub fn set_sync_state(&self, device_id: &str, sync: DeviceSync) {
        let changed = {
            let mut states = crate::lock(&self.inner.sync);
            states.insert(device_id.to_owned(), sync.clone()) != Some(sync)
        };
        if changed {
            self.inner.changed();
        }
    }

    /// Records a finished sync with a device.
    pub async fn record_sync(&self, device_id: &str, at_ms: i64) -> Result<()> {
        self.inner
            .store
            .set_paired_device_synced(device_id, at_ms)
            .await?;
        self.inner.changed();
        Ok(())
    }

    /// Paired devices that came into reach on this network, by id.
    pub fn reachable(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.inner.reachable.subscribe()
    }

    /// This device's endpoint id while the endpoint runs.
    pub fn listening_id(&self) -> Option<String> {
        self.inner
            .lock()
            .endpoint
            .as_ref()
            .map(|endpoint| endpoint.id().to_string())
    }

    /// The paired devices (not forgotten), by id.
    pub async fn paired_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .inner
            .store
            .paired_devices()
            .await?
            .into_iter()
            .filter(|device| device.forgotten_at_ms.is_none())
            .map(|device| device.endpoint_id)
            .collect())
    }

    fn admin(&self) -> Result<()> {
        self.inner.accounts.require(Permission::Admin)
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        crate::lock(&self.state)
    }

    fn on_loopback(&self) -> bool {
        self.loopback.load(Ordering::Relaxed)
    }

    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
    }

    fn sync_states(&self) -> HashMap<String, DeviceSync> {
        crate::lock(&self.sync).clone()
    }

    fn endpoint(&self) -> Result<SyncEndpoint> {
        self.lock()
            .endpoint
            .clone()
            .ok_or_else(|| Error::Invalid(crate::tr!("sync isn't ready on this device yet")))
    }

    async fn record(&self) -> DeviceRecord {
        match self.store.get_setting(DEVICE_KEY).await {
            Ok(Some(value)) => serde_json::from_value(value).unwrap_or_default(),
            // Until the user names its kind, what it looks like (#244).
            _ => DeviceRecord {
                kind: crate::device_type::detected(),
                ..DeviceRecord::default()
            },
        }
    }

    async fn save_record(&self, record: &DeviceRecord) -> Result<()> {
        self.store
            .set_setting(DEVICE_KEY, &serde_json::to_value(record)?)
            .await?;
        self.changed();
        Ok(())
    }

    async fn settings(&self) -> SyncSettings {
        SyncSettings::read(self.settings.get()).await
    }

    /// This device as it introduces itself.
    async fn me(&self) -> DeviceInfo {
        let record = self.record().await;
        let name = record.name.unwrap_or_else(host_name);
        DeviceInfo::new(&name, record.kind.into())
    }

    async fn announce(&self) {
        let me = self.me().await;
        let endpoint = self.lock().endpoint.clone();
        if let Some(lan) = endpoint.as_ref().and_then(SyncEndpoint::lan) {
            lan.announce(me);
        }
    }

    async fn load_paired(&self) {
        let paired = match self.store.paired_devices().await {
            Ok(paired) => paired,
            Err(error) => {
                tracing::warn!(%error, "paired devices not read");
                return;
            }
        };
        let forgotten = paired
            .iter()
            .filter(|device| device.forgotten_at_ms.is_some())
            .filter_map(|device| device.endpoint_id.parse().ok())
            .collect();
        let admitted = paired
            .into_iter()
            .filter(|device| device.forgotten_at_ms.is_none())
            .filter_map(|device| {
                Some((
                    device.endpoint_id.parse().ok()?,
                    device.grant_in.parse().ok()?,
                ))
            })
            .collect::<HashMap<EndpointId, Grant>>();
        self.gate.set_paired(admitted, forgotten);
    }

    async fn status(&self) -> DevicesStatus {
        let record = self.record().await;
        let settings = self.settings().await;
        let (identity, endpoint, pairing) = {
            let state = self.lock();
            (
                state.identity,
                state.endpoint.clone(),
                state.pairing.as_ref().map(ActivePairing::view),
            )
        };
        let paired = self.store.paired_devices().await.unwrap_or_default();
        let sync = self.sync_states();
        let devices: Vec<PairedDevice> = paired
            .iter()
            .filter(|device| device.forgotten_at_ms.is_none())
            .map(|device| view(device, endpoint.as_ref(), &sync))
            .collect();
        let mut nearby: Vec<_> = endpoint
            .as_ref()
            .map(SyncEndpoint::nearby)
            .unwrap_or_default()
            .into_iter()
            .filter(|nearby| !devices.iter().any(|d| d.id == nearby.id.to_string()))
            .collect();
        if !nearby.is_empty() {
            let networks = addresses::networks();
            for device in &mut nearby {
                addresses::rank(&mut device.addrs, &networks);
            }
        }
        let nearby = nearby
            .into_iter()
            .map(|nearby| NearbyDevice {
                id: nearby.id.to_string(),
                name: nearby.info.name,
                kind: nearby.info.kind.into(),
                os: nearby.info.os.into(),
                addresses: nearby.addrs.iter().map(SocketAddr::to_string).collect(),
            })
            .collect();
        let lan_addresses = endpoint
            .as_ref()
            .map(|endpoint| {
                endpoint
                    .addr()
                    .ip_addrs()
                    .filter(|addr| caskade_sync::link::is_local(addr.ip()))
                    .filter(|addr| !addr.ip().is_loopback())
                    .map(SocketAddr::to_string)
                    .collect()
            })
            .unwrap_or_default();
        DevicesStatus {
            this_device: ThisDevice {
                id: endpoint.as_ref().map(|e| e.id().to_string()).or(record.id),
                name: record.name.unwrap_or_else(host_name),
                kind: record.kind,
                os: caskade_sync::DeviceOs::current().into(),
                lan_addresses,
                internet: endpoint.as_ref().and_then(SyncEndpoint::internet),
                never_relay: settings.never_relay,
                port: settings.port,
                identity,
                listening: endpoint.is_some(),
            },
            devices,
            nearby,
            pairing,
        }
    }

    async fn paired_device(&self, id: &str) -> Option<PairedDevice> {
        let endpoint = self.lock().endpoint.clone();
        self.store
            .paired_devices()
            .await
            .ok()?
            .iter()
            .find(|device| device.endpoint_id == id && device.forgotten_at_ms.is_none())
            .map(|device| view(device, endpoint.as_ref(), &self.sync_states()))
    }

    /// Tells forgotten devices they were forgotten, when they can be reached;
    /// each is deleted once told (or once it refuses: it forgot this one too).
    async fn tell_forgotten(&self) {
        let Ok(endpoint) = self.endpoint() else {
            return;
        };
        let Ok(devices) = self.store.paired_devices().await else {
            return;
        };
        for device in devices.iter().filter(|d| d.forgotten_at_ms.is_some()) {
            let (Ok(peer), Ok(grant)) = (
                device.endpoint_id.parse::<EndpointId>(),
                device.grant_out.parse::<Grant>(),
            ) else {
                let _ = self.store.delete_paired_device(&device.endpoint_id).await;
                continue;
            };
            let addr = EndpointAddr::from_parts(
                peer,
                device.addresses.iter().filter_map(|a| parse_addr(a)),
            );
            let told = match endpoint.connect(addr, Purpose::Forget, Some(grant)).await {
                Ok(session) => {
                    session.end().await;
                    true
                }
                Err(caskade_sync::Error::Refused) => true,
                Err(_) => false,
            };
            if told {
                let _ = self.store.delete_paired_device(&device.endpoint_id).await;
                self.gate.forget(&peer, false);
                self.changed();
            }
        }
    }

    /// A paired device said it forgot this one.
    async fn forgotten_by(&self, peer: EndpointId) {
        self.gate.forget(&peer, false);
        let _ = self.store.delete_paired_device(&peer.to_string()).await;
        let _ = self.store.sync_forget_copies(&peer.to_string()).await;
        self.changed();
    }
}

/// Accepted connections.
struct Handler(Weak<Inner>);

#[async_trait]
impl Sessions for Handler {
    async fn opened(&self, session: Session) {
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        match session.purpose() {
            Purpose::Pair if session.grant().is_some() => {
                let me = inner.me().await;
                match accept_link(session, &me, Grant::generate()).await {
                    Ok(paired) => {
                        if let Err(error) = inner.store_paired(&paired).await {
                            tracing::warn!(%error, "pairing not stored");
                        }
                    }
                    Err(error) => tracing::info!(%error, "link pairing ended"),
                }
            }
            Purpose::Pair => {
                let me = inner.me().await;
                match CodePairing::begin(session, &me).await {
                    Ok(pairing) => {
                        let decision = inner.show_pairing(&pairing, true);
                        // The connection lives as long as this future.
                        inner.finish_pairing(pairing, decision).await;
                    }
                    Err(error) => {
                        tracing::info!(%error, "pairing ended");
                        inner.gate.release_pairing();
                    }
                }
            }
            Purpose::Forget => {
                let peer = session.peer();
                session.end().await;
                inner.forgotten_by(peer).await;
            }
            Purpose::Sync => match inner.sync_handler.get() {
                Some(handler) => handler.opened(session).await,
                None => session.end().await,
            },
            Purpose::Copy => match inner.copy_handler.get() {
                Some(handler) => handler.opened(session).await,
                None => session.end().await,
            },
            Purpose::Stream => match inner.stream_handler.get() {
                Some(handler) => handler.opened(session).await,
                None => session.end().await,
            },
            Purpose::Play => match inner.play_handler.get() {
                Some(handler) => handler.opened(session).await,
                None => session.end().await,
            },
            Purpose::Share => match inner.share_handler.get() {
                Some(handler) => handler.opened(session).await,
                None => session.end().await,
            },
        }
    }
}

fn view(
    device: &PairedDeviceRecord,
    endpoint: Option<&SyncEndpoint>,
    sync: &HashMap<String, DeviceSync>,
) -> PairedDevice {
    let state = sync.get(&device.endpoint_id).cloned().unwrap_or_default();
    let link = device
        .endpoint_id
        .parse::<EndpointId>()
        .ok()
        .zip(endpoint)
        .map_or(DeviceLink::Offline, |(peer, endpoint)| {
            link_of(peer, endpoint, device.last_sync_ms, &state)
        });
    PairedDevice {
        id: device.endpoint_id.clone(),
        name: device.name.clone(),
        kind: parse_name(&device.kind).unwrap_or_default(),
        os: parse_name(&device.os).unwrap_or(DeviceOs::Linux),
        link,
        addresses: device
            .addresses
            .iter()
            .filter(|addr| addr.parse::<SocketAddr>().is_ok())
            .cloned()
            .collect(),
        last_sync_ms: device.last_sync_ms,
        paired_at_ms: device.paired_at_ms,
        syncing: state.syncing,
        sync_error: state.error,
    }
}

/// How a paired device is reached: its open connection's link; with none
/// open, `lan` while it announces itself on this network, or the link of
/// its last exchange while that went through less than
/// [`LAST_LINK_FOR_MS`] ago and nothing has failed since. Connections
/// close between syncs, so an open one alone would call every device
/// offline most of the time.
fn link_of(
    peer: EndpointId,
    endpoint: &SyncEndpoint,
    last_sync_ms: Option<i64>,
    state: &DeviceSync,
) -> DeviceLink {
    let open = endpoint.link(peer);
    if open != Link::Offline {
        return open.into();
    }
    if endpoint.nearby().iter().any(|nearby| nearby.id == peer) {
        return DeviceLink::Lan;
    }
    let recent = last_sync_ms.is_some_and(|at| now_ms().saturating_sub(at) < LAST_LINK_FOR_MS);
    match endpoint.last_link(peer) {
        Some(link) if recent && state.error.is_none() => link.into(),
        _ => DeviceLink::Offline,
    }
}

fn parse_id(id: &str) -> Result<EndpointId> {
    id.parse()
        .map_err(|_| Error::Invalid(crate::tr!("that isn't a device id")))
}

fn name_of<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn parse_name<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

/// This machine's host name, the default device name.
fn host_name() -> String {
    sysinfo::System::host_name()
        .map(|name| caskade_sync::device::clean_name(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Caskade".to_owned())
}
