//! Streaming hosts this device can stream from
//! (#262): found on the local network (mDNS), typed by address
//! (`core.streaming.addresses`), or paired before (Caskade's own client
//! state). Any Sunshine host is welcome, not only Caskade's.
//!
//! Each is asked `/serverinfo` (no pairing needed) for its name, id and
//! whether it is busy; one that doesn't answer is asleep when its MAC is
//! known (Moonlight wakes it first), else unreachable. A paired host's apps
//! become releases of library games ([`LibraryStore::sync_stream_apps`]):
//! the host is asked for them directly, and when it doesn't answer they are
//! read from the client's state after `moonlight list` refreshed them.

use super::{
    client::{SETTINGS_DIR, SETTINGS_FILE, StreamingClient},
    host::{HTTP_PORT, SETTINGS},
    mdns,
    pairing::Identity,
    util::{tag, url_host},
};
use caskade_core::{
    Error, Result,
    bus::{Bus, Event},
    db::{LibraryStore, StreamApp},
    settings::{SettingsService, StreamingSettings},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Where the id of this device's own host stays across restarts. Never in a
/// backup: restored on another PC, it would hide this one from it.
pub const OWN_HOST_KEY: &str = "streaming.ownHost";

/// Whether a host can be streamed from now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HostReach {
    Ready,
    /// Playing another game.
    Busy,
    /// Not answering, but it can be woken (its MAC is known).
    Asleep,
    Unreachable,
}

/// A host as the client sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamHost {
    /// Its unique id.
    pub id: String,
    /// Its device name.
    pub name: String,
    pub address: Option<String>,
    pub reach: HostReach,
    /// This device paired with it.
    pub paired: bool,
    /// The games it offers this device.
    pub games: u32,
}

/// What `/serverinfo` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub name: String,
    pub id: String,
    pub busy: bool,
}

/// Reads `/serverinfo`'s answer.
pub fn server_info(xml: &str) -> Option<ServerInfo> {
    let name = tag(xml, "hostname")?;
    let id = tag(xml, "uniqueid")?;
    if id.is_empty() {
        return None;
    }
    let busy = tag(xml, "state").is_some_and(|state| state.contains("BUSY"))
        || tag(xml, "currentgame").is_some_and(|game| game != "0" && !game.is_empty());
    Some(ServerInfo {
        name: name.to_owned(),
        id: id.to_owned(),
        busy,
    })
}

/// Asks the host at `address` its `/serverinfo`, with a short timeout.
pub async fn ask_server_info(address: &str, port: u16) -> Option<ServerInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
        .ok()?;
    ask_server_info_with(&client, address, port).await
}

/// [`ask_server_info`] through `client`.
async fn ask_server_info_with(
    client: &reqwest::Client,
    address: &str,
    port: u16,
) -> Option<ServerInfo> {
    let url = format!(
        "http://{}:{port}/serverinfo?uniqueid=0123456789ABCDEF&uuid={}",
        url_host(address),
        uuid::Uuid::new_v4().simple()
    );
    let body = client.get(url).send().await.ok()?.text().await.ok()?;
    server_info(&body)
}

/// A host the client paired with, from its state file's `[hosts]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PairedHost {
    pub id: String,
    pub name: String,
    pub address: Option<String>,
    /// Every address it was reached at: typed, then on the local network.
    pub addresses: Vec<String>,
    /// Its MAC is known: it can be woken.
    pub wakeable: bool,
    /// Its MAC, for Wake-on-LAN.
    pub mac: Option<[u8; 6]>,
    /// Its certificate, as pairing recorded it.
    pub server_cert_pem: Option<String>,
    pub apps: Vec<StreamApp>,
}

/// A Qt settings `@ByteArray(...)` value as bytes: `\xNN` escapes (as
/// many hex digits as follow) and the usual letters undone.
pub fn qt_raw_bytes(value: &str) -> Option<Vec<u8>> {
    let inner = value
        .trim_matches('"')
        .strip_prefix("@ByteArray(")?
        .strip_suffix(')')?;
    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buffer = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        match chars.next() {
            Some('x') => {
                let mut value: u32 = 0;
                while let Some(digit) = chars.peek().and_then(|c| c.to_digit(16)) {
                    value = value.saturating_mul(16).saturating_add(digit);
                    chars.next();
                }
                out.push(u8::try_from(value).ok()?);
            }
            Some('n') => out.push(b'\n'),
            Some('r') => out.push(b'\r'),
            Some('t') => out.push(b'\t'),
            Some('0') => out.push(0),
            Some('a') => out.push(7),
            Some('b') => out.push(8),
            Some('f') => out.push(12),
            Some('v') => out.push(11),
            Some(other) => {
                let mut buffer = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
            None => out.push(b'\\'),
        }
    }
    Some(out)
}

/// A Qt settings `@ByteArray(...)` value holding text.
pub fn qt_text(value: &str) -> Option<String> {
    String::from_utf8(qt_raw_bytes(value)?).ok()
}

/// The paired hosts in a client state file (Qt settings: `N\key=value`).
pub fn paired_hosts(settings: &str) -> Vec<PairedHost> {
    let mut in_hosts = false;
    let mut hosts: BTreeMap<u32, PairedHost> = BTreeMap::new();
    let mut apps: BTreeMap<(u32, u32), StreamApp> = BTreeMap::new();
    for line in settings.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_hosts = line == "[hosts]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_hosts) else {
            continue;
        };
        let parts: Vec<&str> = key.split('\\').collect();
        let Some(Ok(host)) = parts.first().map(|n| n.parse::<u32>()) else {
            continue;
        };
        let entry = hosts.entry(host).or_default();
        match parts[1..] {
            ["uuid"] => entry.id = value.to_owned(),
            ["hostname"] => entry.name = value.to_owned(),
            ["manualaddress" | "localaddress"] if !value.is_empty() => {
                if entry.address.is_none() {
                    entry.address = Some(value.to_owned());
                }
                if !entry.addresses.iter().any(|known| known == value) {
                    entry.addresses.push(value.to_owned());
                }
            }
            ["mac"] => {
                entry.mac = qt_raw_bytes(value).and_then(|bytes| bytes.try_into().ok());
                entry.wakeable = entry.mac.is_some();
            }
            ["srvcert"] => entry.server_cert_pem = qt_text(value),
            ["apps", app, field] => {
                let Ok(app) = app.parse::<u32>() else {
                    continue;
                };
                let slot = apps.entry((host, app)).or_insert(StreamApp {
                    id: String::new(),
                    name: String::new(),
                });
                match field {
                    "id" => slot.id = value.to_owned(),
                    "name" => slot.name = value.to_owned(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for ((host, _), app) in apps {
        if let Some(entry) = hosts.get_mut(&host)
            && !app.id.is_empty()
            && !app.name.is_empty()
        {
            entry.apps.push(app);
        }
    }
    hosts
        .into_values()
        .filter(|host| !host.id.is_empty())
        .collect()
}

/// How often things are done (shorter in tests).
#[derive(Debug, Clone, Copy)]
pub struct HostTiming {
    /// How long mDNS answers are waited for.
    pub listen: Duration,
    /// How long a host gets to answer `/serverinfo`.
    pub answer: Duration,
    /// How often hosts are looked for.
    pub every: Duration,
    /// How often a paired host's app list is fetched again.
    pub apps_every: Duration,
}

impl Default for HostTiming {
    fn default() -> Self {
        Self {
            listen: Duration::from_millis(1500),
            answer: Duration::from_secs(2),
            every: Duration::from_secs(5),
            apps_every: Duration::from_secs(10 * 60),
        }
    }
}

/// A host's apps, and the device it is on.
type MadeFrom = (Vec<StreamApp>, Option<String>);

struct Inner {
    store: LibraryStore,
    bus: Bus,
    settings: SettingsService,
    client: StreamingClient,
    mdns: std::net::SocketAddr,
    timing: HostTiming,
    http: reqwest::Client,
    hosts: Mutex<BTreeMap<String, StreamHost>>,
    /// When each host's apps were last fetched.
    apps_at: Mutex<HashMap<String, Instant>>,
    /// What each host last said it offers, asked directly.
    apps: Mutex<HashMap<String, Vec<StreamApp>>>,
    /// What each host's releases were last made from (its apps, and the
    /// device it is on): the library is written again only when it changes.
    synced: Mutex<HashMap<String, MadeFrom>>,
    /// The client's state folder, when a test gives one.
    state: Option<std::path::PathBuf>,
    refreshing: tokio::sync::Mutex<()>,
    /// The devices paired through sync (#267): a host on one of them names
    /// its games by that device's copies.
    devices: std::sync::OnceLock<caskade_core::devices::Devices>,
    /// The port this device's own host would answer on, over loopback.
    own_port: u16,
    /// The id of this device's own host, once it has answered, here or
    /// before a restart ([`OWN_HOST_KEY`]).
    own: Mutex<Option<String>>,
}

/// The hosts this device can stream from; cheap to clone.
#[derive(Clone)]
pub struct StreamHosts {
    inner: Arc<Inner>,
}

impl StreamHosts {
    pub fn new(
        store: LibraryStore,
        bus: Bus,
        settings: SettingsService,
        client: StreamingClient,
    ) -> Result<Self> {
        Self::with(
            store,
            bus,
            settings,
            client,
            mdns::MULTICAST,
            HostTiming::default(),
        )
    }

    /// Another mDNS responder and timing (tests).
    pub fn with(
        store: LibraryStore,
        bus: Bus,
        settings: SettingsService,
        client: StreamingClient,
        mdns: std::net::SocketAddr,
        timing: HostTiming,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(timing.answer)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(caskade_core::error::net)?;
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                bus,
                settings,
                client,
                mdns,
                timing,
                http,
                hosts: Mutex::default(),
                apps_at: Mutex::default(),
                apps: Mutex::default(),
                synced: Mutex::default(),
                state: None,
                refreshing: tokio::sync::Mutex::new(()),
                devices: std::sync::OnceLock::new(),
                own_port: HTTP_PORT,
                own: Mutex::default(),
            }),
        })
    }

    /// Looks for this device's own host on `port` (tests).
    pub fn with_own_port(self, port: u16) -> Self {
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_own_port before the service is shared"));
        inner.own_port = port;
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Reads paired hosts from `state` instead of the installed client's
    /// (tests).
    pub fn with_client_state(self, state: std::path::PathBuf) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_client_state before the service is shared"));
        Self {
            inner: Arc::new(Inner {
                state: Some(state),
                ..inner
            }),
        }
    }

    /// Names a host's games by its device's own copies when the host is a
    /// device paired through sync (#267). Set once, when devices exist.
    pub fn set_devices(&self, devices: caskade_core::devices::Devices) {
        let _ = self.inner.devices.set(devices);
    }

    /// The sync-paired device at one of `addresses`, by its id.
    async fn device_at(&self, addresses: &[String]) -> Option<String> {
        let devices = self.inner.devices.get()?;
        let ip = |text: &str| {
            text.parse::<std::net::SocketAddr>()
                .map(|addr| addr.ip())
                .or_else(|_| text.parse::<IpAddr>())
                .ok()
        };
        let wanted: Vec<IpAddr> = addresses.iter().filter_map(|a| ip(a)).collect();
        devices
            .status()
            .await
            .devices
            .into_iter()
            .find(|device| {
                device
                    .addresses
                    .iter()
                    .filter_map(|a| ip(a))
                    .any(|at| wanted.contains(&at))
            })
            .map(|device| device.id)
    }

    /// Every host seen since Caskade started, by name.
    pub fn hosts(&self) -> Vec<StreamHost> {
        let mut hosts: Vec<StreamHost> = caskade_core::lock(&self.inner.hosts)
            .values()
            .cloned()
            .collect();
        hosts.sort_by_key(|host| host.name.to_lowercase());
        hosts
    }

    /// Remembers a host typed by address (a name or an IP), for networks
    /// where mDNS doesn't reach.
    pub async fn add_address(&self, address: &str) -> Result<()> {
        let address = address.trim();
        let valid = address.parse::<IpAddr>().is_ok()
            || (!address.is_empty()
                && address.len() <= 253
                && address
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'));
        if !valid {
            return Err(Error::Invalid(caskade_core::tr!(
                "{address} isn't a device's address",
                address = address
            )));
        }
        let mut settings: StreamingSettings = self.inner.settings.read_or_default(SETTINGS).await;
        if !settings.addresses.iter().any(|known| known == address) {
            settings.addresses.push(address.to_owned());
            self.inner
                .settings
                .set(SETTINGS, serde_json::to_value(settings)?, None)
                .await
                .map_err(|error| Error::Invalid(error.to_string()))?;
        }
        Ok(())
    }

    async fn ask(&self, address: &str, port: u16) -> Option<ServerInfo> {
        ask_server_info_with(&self.inner.http, address, port).await
    }

    /// The client's state file, when it is installed.
    async fn client_settings(&self) -> Option<String> {
        let state = match &self.inner.state {
            Some(state) => state.clone(),
            None => self.inner.client.state().await.ok()??,
        };
        std::fs::read_to_string(state.join(SETTINGS_DIR).join(SETTINGS_FILE)).ok()
    }

    /// The paired hosts in the client's state file `settings`.
    fn paired(&self, settings: Option<&str>) -> Vec<PairedHost> {
        let mut hosts = settings.map(paired_hosts).unwrap_or_default();
        hosts.retain(|host| !self.is_own(&host.id));
        hosts
    }

    /// Whether `id` is this device's own host, as far as it has answered,
    /// now or before a restart. By id: another device can have this device's
    /// name.
    fn is_own(&self, id: &str) -> bool {
        caskade_core::lock(&self.inner.own)
            .as_ref()
            .is_some_and(|own| own.eq_ignore_ascii_case(id))
    }

    /// Learns this device's own host: asked on loopback, else as it last
    /// answered, which a host switched off can't say itself.
    async fn find_own(&self) {
        let store = &self.inner.store;
        if let Some(own) = self.ask("127.0.0.1", self.inner.own_port).await {
            if !self.is_own(&own.id)
                && let Err(error) = store
                    .set_setting(OWN_HOST_KEY, &serde_json::json!(own.id))
                    .await
            {
                tracing::debug!(%error, "this device's host id not kept");
            }
            *caskade_core::lock(&self.inner.own) = Some(own.id);
        } else if caskade_core::lock(&self.inner.own).is_none()
            && let Ok(Some(serde_json::Value::String(own))) = store.get_setting(OWN_HOST_KEY).await
        {
            *caskade_core::lock(&self.inner.own) = Some(own);
        }
    }

    /// [`refresh`](Self::refresh), asking each paired host for its games now
    /// and not at the next [`HostTiming::apps_every`]: a device just paired,
    /// or someone pressed Look again.
    pub async fn look_again(&self) -> Result<()> {
        caskade_core::lock(&self.inner.apps_at).clear();
        caskade_core::lock(&self.inner.synced).clear();
        self.refresh().await
    }

    /// Looks for hosts once: mDNS, typed addresses and paired hosts, each
    /// asked whether it is there; a paired host's apps go into the library.
    pub async fn refresh(&self) -> Result<()> {
        let _refreshing = self.inner.refreshing.lock().await;
        // A host on this device answers on loopback, and is remembered once
        // it stops: it's found on the network like any other.
        self.find_own().await;
        let mut candidates: Vec<(String, u16)> = Vec::new();
        match mdns::browse(self.inner.mdns, self.inner.timing.listen).await {
            Ok(found) => candidates.extend(
                found
                    .into_iter()
                    .map(|host| (host.address.to_string(), host.port)),
            ),
            Err(error) => tracing::debug!(%error, "no mDNS answers"),
        }
        let settings: StreamingSettings = self.inner.settings.read_or_default(SETTINGS).await;
        candidates.extend(
            settings
                .addresses
                .into_iter()
                .map(|address| (address, HTTP_PORT)),
        );
        let client_settings = self.client_settings().await;
        let paired = self.paired(client_settings.as_deref());
        candidates.extend(
            paired
                .iter()
                .filter_map(|host| Some((host.address.clone()?, HTTP_PORT))),
        );
        candidates.sort();
        candidates.dedup();
        let mut asking = tokio::task::JoinSet::new();
        for (address, port) in candidates {
            let this = self.clone();
            asking.spawn(async move {
                let info = this.ask(&address, port).await;
                (address, info)
            });
        }
        let mut answers = Vec::new();
        while let Some(answer) = asking.join_next().await {
            if let Ok(answer) = answer {
                answers.push(answer);
            }
        }
        answers.sort_by_key(|(address, _)| address.clone());
        let mut seen: BTreeMap<String, StreamHost> = BTreeMap::new();
        for (address, info) in answers {
            let Some(info) = info else { continue };
            let paired = paired
                .iter()
                .find(|host| host.id.eq_ignore_ascii_case(&info.id));
            seen.entry(info.id.clone()).or_insert(StreamHost {
                name: info.name,
                address: Some(address),
                reach: if info.busy {
                    HostReach::Busy
                } else {
                    HostReach::Ready
                },
                paired: paired.is_some(),
                games: paired.map_or(0, |host| self.apps_of(host).len() as u32),
                id: info.id,
            });
        }
        let changed = {
            let mut hosts = caskade_core::lock(&self.inner.hosts);
            // Known hosts that didn't answer now: asleep or unreachable.
            let mut next: BTreeMap<String, StreamHost> = hosts
                .values()
                .filter(|host| !seen.contains_key(&host.id))
                .map(|host| {
                    let wakeable = paired
                        .iter()
                        .any(|paired| paired.id.eq_ignore_ascii_case(&host.id) && paired.wakeable);
                    StreamHost {
                        reach: if wakeable {
                            HostReach::Asleep
                        } else {
                            HostReach::Unreachable
                        },
                        ..host.clone()
                    }
                })
                .map(|host| (host.id.clone(), host))
                .collect();
            for host in &paired {
                if !seen.contains_key(&host.id) && !next.contains_key(&host.id) {
                    next.insert(
                        host.id.clone(),
                        StreamHost {
                            id: host.id.clone(),
                            name: host.name.clone(),
                            address: host.address.clone(),
                            reach: if host.wakeable {
                                HostReach::Asleep
                            } else {
                                HostReach::Unreachable
                            },
                            paired: true,
                            games: self.apps_of(host).len() as u32,
                        },
                    );
                }
            }
            next.extend(seen);
            next.retain(|id, _| !self.is_own(id));
            let changed = *hosts != next;
            *hosts = next;
            changed
        };
        if changed {
            self.inner.bus.emit(Event::StreamHostsChanged);
        }
        self.library(&paired, client_settings.as_deref()).await
    }

    /// What `host` offers, from the host itself with the client's identity.
    /// `moonlight list` can wait on box art for good, and only a clean quit
    /// saves what it found, which Windows gives no way to ask of it.
    async fn ask_for_apps(
        &self,
        host: &PairedHost,
        identity: Option<&Identity>,
        address: &str,
    ) -> Option<Vec<StreamApp>> {
        let identity = identity?;
        let endpoint = super::pairing::Endpoint::new(address).ok()?;
        let link =
            super::gamestream::HostLink::new(endpoint, identity, host.server_cert_pem.as_deref()?)
                .ok()?;
        match link.apps().await {
            Ok(apps) => Some(apps),
            Err(error) => {
                tracing::debug!(%error, host = host.name, "the host didn't list its games");
                None
            }
        }
    }

    /// `host`'s games: what it last said itself, else the client's record.
    fn apps_of(&self, host: &PairedHost) -> Vec<StreamApp> {
        caskade_core::lock(&self.inner.apps)
            .get(&host.id)
            .cloned()
            .unwrap_or_else(|| host.apps.clone())
    }

    /// Paired hosts' apps as library releases; a reachable host's list is
    /// fetched again now and then. `settings` is the client's state file.
    async fn library(&self, paired: &[PairedHost], settings: Option<&str>) -> Result<()> {
        let identity = settings.and_then(Identity::from_settings);
        let reachable: Vec<&PairedHost> = paired
            .iter()
            .filter(|host| {
                caskade_core::lock(&self.inner.hosts)
                    .get(&host.id)
                    .is_some_and(|known| matches!(known.reach, HostReach::Ready | HostReach::Busy))
            })
            .collect();
        // `moonlight list` rewrote the client's state file.
        let mut listed = false;
        for host in reachable {
            let due = caskade_core::lock(&self.inner.apps_at)
                .get(&host.id)
                .is_none_or(|at| at.elapsed() >= self.inner.timing.apps_every);
            if let (true, Some(address)) = (due, &host.address) {
                caskade_core::lock(&self.inner.apps_at).insert(host.id.clone(), Instant::now());
                match self.ask_for_apps(host, identity.as_ref(), address).await {
                    Some(apps) => {
                        caskade_core::lock(&self.inner.apps).insert(host.id.clone(), apps);
                    }
                    None => {
                        if let Err(error) = self.inner.client.list(address).await {
                            tracing::debug!(%error, host = host.name, "app list not refreshed");
                        }
                        listed = true;
                    }
                }
            }
        }
        let paired = if listed {
            self.paired(self.client_settings().await.as_deref())
        } else {
            paired.to_vec()
        };
        for host in &paired {
            let mut addresses = host.addresses.clone();
            if let Some(found) = caskade_core::lock(&self.inner.hosts)
                .get(&host.id)
                .and_then(|known| known.address.clone())
            {
                addresses.push(found);
            }
            let device = self.device_at(&addresses).await;
            let made_from = (self.apps_of(host), device);
            if caskade_core::lock(&self.inner.synced).get(&host.id) == Some(&made_from)
                && self.releases_kept(&host.id, &made_from.0).await
            {
                continue;
            }
            let (apps, device) = &made_from;
            let sync = self
                .inner
                .store
                .sync_stream_apps_from(&host.id, apps, device.as_deref())
                .await?;
            caskade_core::lock(&self.inner.synced).insert(host.id.clone(), made_from);
            for game_id in sync.added {
                self.inner.bus.emit(Event::LibraryGameAdded { game_id });
            }
            for game_id in sync.updated {
                self.inner.bus.emit(Event::LibraryGameUpdated { game_id });
            }
            for game_id in sync.removed {
                self.inner.bus.emit(Event::LibraryGameRemoved { game_id });
            }
        }
        Ok(())
    }

    /// Whether the library still has a release for each of `apps` from
    /// `host`, and no other: someone may have deleted one since.
    async fn releases_kept(&self, host: &str, apps: &[StreamApp]) -> bool {
        let Ok(entries) = self
            .inner
            .store
            .entries_for_source(&format!("stream:{host}"))
            .await
        else {
            return false;
        };
        let mut kept: Vec<&str> = entries.iter().map(|e| e.external_id.as_str()).collect();
        let mut offered: Vec<&str> = apps.iter().map(|app| app.id.as_str()).collect();
        kept.sort_unstable();
        offered.sort_unstable();
        kept == offered
    }

    /// Looks for hosts now and then, while Caskade runs.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(this.inner.timing.every);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                if let Err(error) = this.refresh().await {
                    tracing::warn!(%error, "streaming hosts not refreshed");
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
