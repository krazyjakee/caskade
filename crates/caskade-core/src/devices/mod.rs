//! This device and the devices paired with it (#226): identity, the sync
//! endpoint, pairing by code or link, and forgetting.
//!
//! The connection layer is `caskade-sync` (`docs/sync.md`); this service
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

mod gate;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use caskade_sync::pairing::{accept_link, pair_with_link};
use caskade_sync::{
    CodePairing, Config, DeviceInfo, EndpointAddr, EndpointId, Grant, Identity, KeyStore,
    LINK_LIFETIME, Loaded, Lost, Paired, PairingLink, Purpose, Session, Sessions, SyncEndpoint,
    TransportAddr,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::{oneshot, watch};

use crate::accounts::{AccountService, Permission};
use crate::db::{LibraryStore, PairedDeviceRecord};
use crate::settings::SettingsService;
use crate::sync::policy::SyncSettings;
use crate::{Error, Result};

use gate::Gate;

/// The OS keychain the device key lives in, and what a session is opened for.
pub use caskade_sync::{Keychain, Purpose as ConnectionPurpose, Session as DeviceSession};
/// For tests: a key store in memory, and loopback-only endpoints
/// ([`Devices::with_reach`]).
pub use caskade_sync::{MemoryKeyStore, Reach};

/// The settings section holding Never use relays and the listening port
/// (shared with #227's category choices).
pub const NAMESPACE: &str = "core.sync";

/// This device's last endpoint id, name and kind, in the settings table (not
/// a section: the user edits them through [`Devices`]).
pub const DEVICE_KEY: &str = "sync.device";

/// How long a pairing link's address waits for a relay before it's minted
/// with local addresses only.
const LINK_ADDR_WAIT: Duration = Duration::from_secs(2);

/// How often forgotten devices not told yet are tried again.
const FORGET_RETRY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    #[default]
    Desktop,
    Laptop,
    Handheld,
    Tv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceOs {
    Linux,
    Windows,
    Mac,
}

/// How a device is reached right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceLink {
    Lan,
    Direct,
    Relay,
    Offline,
}

/// Where this device's identity stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IdentityStatus {
    /// Being read from the keychain.
    Starting,
    Ready,
    /// The keychain lost this device's key. A new identity needs asking:
    /// paired devices must pair again.
    Missing,
    /// The keychain entry isn't a key; the same choice as `missing`.
    Unreadable,
    /// The keychain didn't answer; sync waits.
    KeychainUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThisDevice {
    /// Its endpoint id, once the identity is ready.
    pub id: Option<String>,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    /// Addresses on this network other devices reach it at.
    pub lan_addresses: Vec<String>,
    pub never_relay: bool,
    pub port: Option<u16>,
    pub identity: IdentityStatus,
    /// The endpoint is running: other devices can reach this one.
    pub listening: bool,
}

/// A device paired with this one. Its grants stay in the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    pub link: DeviceLink,
    /// Where it was last reached.
    pub addresses: Vec<String>,
    pub last_sync_ms: Option<i64>,
    pub paired_at_ms: i64,
}

/// A Caskade device on this network that isn't paired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NearbyDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PairingState {
    /// Codes on screen, waiting for one or both users.
    Waiting,
    Paired,
    /// Cancelled on either device; nothing was stored.
    Cancelled,
    /// The connection failed or timed out; nothing was stored.
    Failed,
}

/// A code pairing: both devices show `code`, and both users confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
    pub code: String,
    /// The other device started it.
    pub incoming: bool,
    /// This device's user confirmed.
    pub confirmed: bool,
    pub state: PairingState,
}

/// A pairing link to show or copy. It works once, until `expires_at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingLinkInfo {
    pub url: String,
    pub expires_at_ms: i64,
}

/// A pairing link opened from outside the app (a `caskade://pair` link),
/// shown for an admin to accept. The link itself stays in the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingLinkTarget {
    /// The device it pairs with.
    pub device_id: String,
    pub expires_at_ms: i64,
    pub expired: bool,
}

/// Reads a pairing link without using it.
pub fn inspect_link(text: &str) -> Result<PairingLinkTarget> {
    let link = PairingLink::parse(text).map_err(pairing_error)?;
    Ok(PairingLinkTarget {
        device_id: link.peer().to_string(),
        expires_at_ms: millis(link.expires_at()),
        expired: link.expired(SystemTime::now()),
    })
}

/// Everything Settings → Devices shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DevicesStatus {
    pub this_device: ThisDevice,
    pub devices: Vec<PairedDevice>,
    pub nearby: Vec<NearbyDevice>,
    pub pairing: Option<Pairing>,
}

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
    /// Tests bind on loopback instead of the network.
    reach: Option<Reach>,
    gate: Arc<Gate>,
    state: Mutex<State>,
    revision: watch::Sender<u64>,
    /// One bind at a time.
    binding: tokio::sync::Mutex<()>,
}

struct State {
    identity: IdentityStatus,
    key: Option<Identity>,
    endpoint: Option<SyncEndpoint>,
    pairing: Option<ActivePairing>,
    /// Forwards the endpoint's link and nearby changes; aborted on rebind.
    forwarders: Vec<Forwarder>,
}

struct ActivePairing {
    peer: EndpointId,
    info: DeviceInfo,
    code: String,
    incoming: bool,
    confirmed: bool,
    state: PairingState,
    decide: Option<oneshot::Sender<bool>>,
}

struct Forwarder(tokio::task::JoinHandle<()>);

impl Drop for Forwarder {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Devices {
    /// The service over `store`, with the device key in `keys` (the OS
    /// keychain in the app). Nothing runs until [`Devices::start`].
    pub fn new(store: LibraryStore, accounts: AccountService, keys: Arc<dyn KeyStore>) -> Self {
        Self::build(store, accounts, keys, None)
    }

    /// Binds on `reach` instead of the network, whatever the settings say:
    /// for tests (`Reach::loopback()`).
    pub fn with_reach(
        store: LibraryStore,
        accounts: AccountService,
        keys: Arc<dyn KeyStore>,
        reach: Reach,
    ) -> Self {
        Self::build(store, accounts, keys, Some(reach))
    }

    fn build(
        store: LibraryStore,
        accounts: AccountService,
        keys: Arc<dyn KeyStore>,
        reach: Option<Reach>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                accounts,
                keys,
                settings: OnceLock::new(),
                reach,
                gate: Arc::default(),
                state: Mutex::new(State {
                    identity: IdentityStatus::Starting,
                    key: None,
                    endpoint: None,
                    pairing: None,
                    forwarders: Vec::new(),
                }),
                revision: watch::channel(0).0,
                binding: tokio::sync::Mutex::new(()),
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
                        if changed.is_some_and(|changed| changed.namespace == NAMESPACE) {
                            inner.bind().await;
                        }
                    }
                    _ = retry.tick() => inner.tell_forgotten().await,
                    _ = inner.gate.knocked() => inner.tell_forgotten().await,
                }
            }
        })
    }

    /// Closes the endpoint. Call at exit.
    pub async fn shutdown(&self) {
        let endpoint = self.inner.lock().endpoint.take();
        if let Some(endpoint) = endpoint {
            let _ = endpoint.shutdown().await;
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

    /// Starts pairing with a nearby device: both show the code.
    pub async fn pair_nearby(&self, device_id: &str) -> Result<Pairing> {
        self.admin()?;
        let peer = parse_id(device_id)?;
        let endpoint = self.inner.endpoint()?;
        if !self.inner.gate.reserve_pairing() {
            return Err(Error::Invalid(crate::tr!("another pairing is in progress")));
        }
        let addrs = endpoint
            .nearby()
            .into_iter()
            .find(|nearby| nearby.id == peer)
            .map(|nearby| nearby.addrs)
            .unwrap_or_default();
        let addr = EndpointAddr::from_parts(peer, addrs.into_iter().map(TransportAddr::Ip));
        let me = self.inner.me().await;
        let begun = async {
            let session = endpoint.connect(addr, Purpose::Pair, None).await?;
            CodePairing::begin(session, &me).await
        }
        .await;
        let pairing = match begun {
            Ok(pairing) => pairing,
            Err(error) => {
                self.inner.gate.release_pairing();
                return Err(pairing_error(error));
            }
        };
        let decision = self.inner.show_pairing(&pairing, false);
        let inner = self.inner.clone();
        tokio::spawn(async move { inner.finish_pairing(pairing, decision).await });
        self.inner
            .pairing()
            .ok_or_else(|| Error::Invalid(crate::tr!("pairing was cancelled")))
    }

    /// This user confirms the codes match.
    pub async fn confirm_pairing(&self, device_id: &str) -> Result<()> {
        self.admin()?;
        self.inner.decide(device_id, true)
    }

    /// This user cancels the pairing, or dismisses a finished one.
    pub async fn cancel_pairing(&self, device_id: &str) -> Result<()> {
        self.admin()?;
        self.inner.decide(device_id, false)
    }

    /// A link that pairs another device with this one from anywhere. It works
    /// once, for ten minutes.
    pub async fn pairing_link(&self) -> Result<PairingLinkInfo> {
        self.admin()?;
        let endpoint = self.inner.endpoint()?;
        let grant = Grant::generate();
        let expires = SystemTime::now() + LINK_LIFETIME;
        self.inner.gate.offer_link(grant, expires);
        let addr = endpoint.shareable_addr(LINK_ADDR_WAIT).await;
        let link = PairingLink::new(addr, grant, expires);
        Ok(PairingLinkInfo {
            url: link.to_url(),
            expires_at_ms: millis(expires),
        })
    }

    /// Pairs with the device a pasted pairing link names.
    pub async fn accept_pairing_link(&self, text: &str) -> Result<PairedDevice> {
        self.admin()?;
        let link = PairingLink::parse(text).map_err(pairing_error)?;
        let endpoint = self.inner.endpoint()?;
        if link.peer() == endpoint.id() {
            return Err(Error::Invalid(crate::tr!(
                "that pairing link is this device's own"
            )));
        }
        let me = self.inner.me().await;
        let paired = pair_with_link(&endpoint, &link, &me, Grant::generate())
            .await
            .map_err(pairing_error)?;
        self.inner.store_paired(&paired).await?;
        self.inner
            .paired_device(&paired.peer.to_string())
            .await
            .ok_or_else(|| Error::Invalid(crate::tr!("pairing was cancelled")))
    }

    /// Forgets a paired device: it's refused from now on, and told when it
    /// can be reached. Nothing synced is deleted.
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

    fn admin(&self) -> Result<()> {
        self.inner.accounts.require(Permission::Admin)
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
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
            _ => DeviceRecord::default(),
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
        match self.settings.get() {
            Some(settings) => settings.read_or_default(NAMESPACE).await,
            None => SyncSettings::default(),
        }
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

    async fn load_identity(self: &Arc<Self>) {
        let record = self.record().await;
        let known = record.id.as_deref().and_then(|id| id.parse().ok());
        let status = match Identity::load(self.keys.clone(), known).await {
            Loaded::Ready { identity, .. } => {
                self.identity_ready(identity).await;
                return;
            }
            Loaded::NeedsConfirmation {
                reason: Lost::Missing,
            } => IdentityStatus::Missing,
            Loaded::NeedsConfirmation {
                reason: Lost::Unreadable,
            } => IdentityStatus::Unreadable,
            Loaded::Unavailable => IdentityStatus::KeychainUnavailable,
        };
        tracing::warn!(?status, "sync waits for the device key");
        self.lock().identity = status;
        self.changed();
    }

    async fn identity_ready(self: &Arc<Self>, identity: Identity) {
        let mut record = self.record().await;
        let id = identity.id().to_string();
        if record.id.as_deref() != Some(&id) {
            record.id = Some(id);
            if let Err(error) = self.save_record(&record).await {
                tracing::warn!(%error, "this device's id not saved");
            }
        }
        {
            let mut state = self.lock();
            state.identity = IdentityStatus::Ready;
            state.key = Some(identity);
        }
        self.bind().await;
    }

    /// (Re)binds the endpoint with the current settings.
    async fn bind(self: &Arc<Self>) {
        let _binding = self.binding.lock().await;
        let (old, key) = {
            let mut state = self.lock();
            state.forwarders.clear();
            (state.endpoint.take(), state.key.clone())
        };
        if let Some(old) = old {
            let _ = old.shutdown().await;
        }
        let Some(identity) = key else {
            self.changed();
            return;
        };
        let settings = self.settings().await;
        let reach = self
            .reach
            .clone()
            .unwrap_or_else(|| Reach::network(settings.never_relay, settings.port));
        let config = Config {
            identity: Some(identity),
            reach,
            device: self.me().await,
            ..Config::default()
        };
        let handler = Arc::new(Handler(Arc::downgrade(self)));
        match SyncEndpoint::bind(config, self.gate.clone(), handler).await {
            Ok(endpoint) => {
                let forwarders = self.forward(&endpoint);
                let mut state = self.lock();
                state.endpoint = Some(endpoint);
                state.forwarders = forwarders;
            }
            Err(error) => tracing::warn!(%error, "sync endpoint not started"),
        }
        self.changed();
        let this = self.clone();
        tokio::spawn(async move { this.tell_forgotten().await });
    }

    /// Link and nearby changes become revisions.
    fn forward(self: &Arc<Self>, endpoint: &SyncEndpoint) -> Vec<Forwarder> {
        let mut forwarders = Vec::new();
        let mut links = endpoint.links().subscribe();
        let weak = Arc::downgrade(self);
        forwarders.push(Forwarder(tokio::spawn(async move {
            while let Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                links.recv().await
            {
                let Some(inner) = weak.upgrade() else { break };
                inner.changed();
            }
        })));
        if let Some(lan) = endpoint.lan() {
            let mut nearby = lan.subscribe();
            let weak = Arc::downgrade(self);
            forwarders.push(Forwarder(tokio::spawn(async move {
                while let Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                    nearby.recv().await
                {
                    let Some(inner) = weak.upgrade() else { break };
                    inner.changed();
                    inner.tell_forgotten().await;
                }
            })));
        }
        forwarders
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
        let devices: Vec<PairedDevice> = paired
            .iter()
            .filter(|device| device.forgotten_at_ms.is_none())
            .map(|device| view(device, endpoint.as_ref()))
            .collect();
        let nearby = endpoint
            .as_ref()
            .map(SyncEndpoint::nearby)
            .unwrap_or_default()
            .into_iter()
            .filter(|nearby| !devices.iter().any(|d| d.id == nearby.id.to_string()))
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
            .map(|device| view(device, endpoint.as_ref()))
    }

    fn pairing(&self) -> Option<Pairing> {
        self.lock().pairing.as_ref().map(ActivePairing::view)
    }

    /// Puts a pairing's codes on screen; the receiver gets this user's
    /// decision.
    fn show_pairing(&self, pairing: &CodePairing, incoming: bool) -> oneshot::Receiver<bool> {
        let (decide, decision) = oneshot::channel();
        self.lock().pairing = Some(ActivePairing {
            peer: pairing.peer(),
            info: pairing.peer_info().clone(),
            code: pairing.code().to_owned(),
            incoming,
            confirmed: false,
            state: PairingState::Waiting,
            decide: Some(decide),
        });
        self.changed();
        decision
    }

    async fn finish_pairing(&self, pairing: CodePairing, decision: oneshot::Receiver<bool>) {
        let peer = pairing.peer();
        let result = pairing
            .finish(async { decision.await.unwrap_or(false) }, Grant::generate())
            .await;
        let outcome = match result {
            Ok(paired) => match self.store_paired(&paired).await {
                Ok(()) => PairingState::Paired,
                Err(error) => {
                    tracing::warn!(%error, "pairing not stored");
                    PairingState::Failed
                }
            },
            Err(caskade_sync::Error::Cancelled) => PairingState::Cancelled,
            Err(error) => {
                tracing::info!(%error, "pairing ended");
                PairingState::Failed
            }
        };
        self.gate.release_pairing();
        if let Some(active) = self.lock().pairing.as_mut().filter(|p| p.peer == peer) {
            active.state = outcome;
            active.decide = None;
        }
        self.changed();
    }

    fn decide(&self, device_id: &str, confirmed: bool) -> Result<()> {
        let mut state = self.lock();
        let Some(active) = state
            .pairing
            .as_mut()
            .filter(|p| p.peer.to_string() == device_id)
        else {
            return Err(Error::Invalid(crate::tr!("no pairing with that device")));
        };
        match active.decide.take() {
            Some(decide) => {
                let _ = decide.send(confirmed);
                active.confirmed = confirmed;
                if !confirmed {
                    active.state = PairingState::Cancelled;
                }
            }
            // Finished: cancelling dismisses it.
            None if !confirmed => state.pairing = None,
            None => {}
        }
        drop(state);
        self.changed();
        Ok(())
    }

    async fn store_paired(&self, paired: &Paired) -> Result<()> {
        let record = PairedDeviceRecord {
            endpoint_id: paired.peer.to_string(),
            name: paired.info.name.clone(),
            kind: name_of(&DeviceKind::from(paired.info.kind)),
            os: name_of(&DeviceOs::from(paired.info.os)),
            addresses: paired.addr.addrs.iter().map(addr_text).collect(),
            wol_macs: Vec::new(),
            last_sync_ms: None,
            grant_in: paired.grant_in.to_hex(),
            grant_out: paired.grant_out.to_hex(),
            paired_at_ms: now_ms(),
            forgotten_at_ms: None,
        };
        self.store.save_paired_device(&record).await?;
        self.gate.pair(paired.peer, paired.grant_in);
        self.changed();
        Ok(())
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
        self.changed();
    }
}

impl ActivePairing {
    fn view(&self) -> Pairing {
        Pairing {
            device_id: self.peer.to_string(),
            name: self.info.name.clone(),
            kind: self.info.kind.into(),
            os: self.info.os.into(),
            code: self.code.clone(),
            incoming: self.incoming,
            confirmed: self.confirmed,
            state: self.state,
        }
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
            // Library sync comes with #230; copies with #233.
            _ => session.end().await,
        }
    }
}

fn view(device: &PairedDeviceRecord, endpoint: Option<&SyncEndpoint>) -> PairedDevice {
    let link = device
        .endpoint_id
        .parse::<EndpointId>()
        .ok()
        .zip(endpoint)
        .map_or(DeviceLink::Offline, |(peer, endpoint)| {
            endpoint.link(peer).into()
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
    }
}

/// The user-facing error for a pairing failure.
fn pairing_error(error: caskade_sync::Error) -> Error {
    use caskade_sync::Error as E;
    Error::Invalid(match error {
        E::NotALink => crate::tr!("that isn't a Caskade pairing link"),
        E::LinkExpired => {
            crate::tr!("this pairing link has expired; make a new one on the other device")
        }
        E::LinkRefused => crate::tr!(
            "this pairing link was already used or has expired; make a new one on the other device"
        ),
        E::Refused => crate::tr!("the other device isn't accepting a pairing right now"),
        E::Cancelled => crate::tr!("pairing was cancelled"),
        _ => crate::tr!("couldn't reach the other device"),
    })
}

fn parse_id(id: &str) -> Result<EndpointId> {
    id.parse()
        .map_err(|_| Error::Invalid(crate::tr!("that isn't a device id")))
}

fn addr_text(addr: &TransportAddr) -> String {
    match addr {
        TransportAddr::Ip(addr) => addr.to_string(),
        TransportAddr::Relay(url) => url.to_string(),
        _ => String::new(),
    }
}

fn parse_addr(text: &str) -> Option<TransportAddr> {
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Some(TransportAddr::Ip(addr));
    }
    text.parse().ok().map(TransportAddr::Relay)
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

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
    })
}

fn now_ms() -> i64 {
    millis(SystemTime::now())
}

impl From<caskade_sync::DeviceKind> for DeviceKind {
    fn from(kind: caskade_sync::DeviceKind) -> Self {
        use caskade_sync::DeviceKind as K;
        match kind {
            K::Desktop => Self::Desktop,
            K::Laptop => Self::Laptop,
            K::Handheld => Self::Handheld,
            K::Tv => Self::Tv,
        }
    }
}

impl From<DeviceKind> for caskade_sync::DeviceKind {
    fn from(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::Desktop => Self::Desktop,
            DeviceKind::Laptop => Self::Laptop,
            DeviceKind::Handheld => Self::Handheld,
            DeviceKind::Tv => Self::Tv,
        }
    }
}

impl From<caskade_sync::DeviceOs> for DeviceOs {
    fn from(os: caskade_sync::DeviceOs) -> Self {
        use caskade_sync::DeviceOs as O;
        match os {
            O::Linux => Self::Linux,
            O::Windows => Self::Windows,
            O::Mac => Self::Mac,
        }
    }
}

impl From<caskade_sync::Link> for DeviceLink {
    fn from(link: caskade_sync::Link) -> Self {
        use caskade_sync::Link as L;
        match link {
            L::Lan => Self::Lan,
            L::Direct => Self::Direct,
            L::Relay => Self::Relay,
            L::Offline => Self::Offline,
        }
    }
}
