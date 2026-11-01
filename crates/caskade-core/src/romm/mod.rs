//! RomM servers as a library source (#1). Caskade is a client of the RomM
//! REST API (<https://github.com/rommapp/romm>): it signs in, lists the
//! platforms, collections and ROMs a server has and imports each ROM as a
//! normal, installed-looking library game (`romm:<server>` source, the RomM
//! ROM id as its key) with the server's metadata and cover. Nothing is
//! downloaded up front.
//!
//! Launching one goes through the normal launch pipeline: the
//! [`LaunchPreparer`] step downloads the ROM into the cache first (with
//! progress, cancellation and resume), verifies it against the server's
//! hashes, unpacks multi-file ROMs and archives the platform can't open
//! (with `caskade-installer`'s extractor), commits it with a rename and
//! points the entry's [`LaunchSpec::Emulator`] at the files. The second
//! launch starts from the cache without the network.
//!
//! The cache (`romm-cache/` on the default storage location unless set)
//! keeps to its size limit by evicting the least recently played games;
//! pinned games and, by default, favourites are never evicted. A ROM the
//! server deleted leaves the library unless it is cached; then it stays,
//! orphaned, until it is evicted. While a server can't be reached, cached
//! games keep launching and the others show as unavailable (an entry
//! state of `broken` with the reason).
//!
//! Credentials are kept in the OS keychain ([`secrets`]).
//!
//! [`LaunchPreparer`]: crate::launch::LaunchPreparer
//! [`LaunchSpec::Emulator`]: crate::model::LaunchSpec::Emulator

pub mod api;
mod artwork;
mod bios;
mod cache;
#[cfg(feature = "net")]
mod client;
pub mod secrets;
pub mod settings;
mod sync;
mod watch;

pub use api::{RemoteCollection, RemoteFile, RemotePlatform, RemoteRom};
pub use secrets::{KeychainSecrets, MemorySecrets, SecretStore};
pub use settings::{
    NAMESPACE, RommAuth, RommCacheSettings, RommPlatformMapping, RommServerConfig, RommSettings,
    register_settings,
};

use crate::{
    artwork::ArtworkCache,
    bus::Bus,
    db::LibraryStore,
    emulation::{EmulationService, EmulatorTarget},
    lock,
    model::{GameEntry, SourceId},
    now_ms,
    settings::SettingsService,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Library sources of RomM servers are `romm:<server id>`.
pub const SOURCE_PREFIX: &str = "romm:";

pub fn source_id(server: &str) -> SourceId {
    SourceId::from(format!("{SOURCE_PREFIX}{server}"))
}

/// The server of a RomM library entry.
pub fn server_of(entry: &GameEntry) -> Option<&str> {
    entry.source_id.as_str().strip_prefix(SOURCE_PREFIX)
}

/// Whether the entry's ROM comes from a RomM server and downloads at launch.
pub fn is_romm(entry: &GameEntry) -> bool {
    server_of(entry).is_some()
}

#[derive(Debug, thiserror::Error)]
pub enum RommError {
    /// Connection refused, DNS, TLS handshake or connect timeout.
    #[error("{}", text::unreachable(.0))]
    Unreachable(String),
    /// The server answered, then stalled or broke off: this request failed,
    /// the server isn't offline.
    #[error("{}", text::interrupted(.0))]
    Interrupted(String),
    #[error("{}", text::auth(.0))]
    Auth(String),
    #[error("{0}")]
    Server(String),
    #[error("{0}")]
    Config(String),
    #[error("{}", text::cancelled())]
    Cancelled,
    #[error("{}", text::verify(.0))]
    Verify(String),
    #[error("{}", text::extract(.0))]
    Extract(String),
    #[error(transparent)]
    Core(#[from] crate::Error),
}

/// [`RommError`]'s texts, in the core's language (#214).
mod text {
    pub(super) fn unreachable(why: &str) -> String {
        crate::tr!("can't reach the server: {error}", error = why)
    }

    pub(super) fn interrupted(why: &str) -> String {
        crate::tr!(
            "the connection to the server broke off: {error}",
            error = why
        )
    }

    pub(super) fn auth(why: &str) -> String {
        crate::tr!("sign-in failed: {error}", error = why)
    }

    pub(super) fn cancelled() -> String {
        crate::tr!("the download was cancelled")
    }

    /// `what`: the checksum or file list that didn't match.
    pub(super) fn verify(what: &str) -> String {
        crate::tr!(
            "the download doesn't match the server's {what}",
            what = what
        )
    }

    pub(super) fn extract(why: &str) -> String {
        crate::tr!("can't unpack the download: {error}", error = why)
    }
}

impl From<std::io::Error> for RommError {
    fn from(error: std::io::Error) -> Self {
        Self::Core(error.into())
    }
}

impl From<serde_json::Error> for RommError {
    fn from(error: serde_json::Error) -> Self {
        Self::Core(error.into())
    }
}

pub type Result<T, E = RommError> = std::result::Result<T, E>;

/// How a stored secret starts: then the origin it was saved for, a newline
/// and the secret itself.
const SEALED: &str = "caskade-romm-v1\n";

/// `url`'s origin (scheme, host and port), for HTTP(S) addresses only.
fn origin(url: &str) -> Option<String> {
    let url = url::Url::parse(url.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.origin().ascii_serialization())
}

/// `secret` as stored: bound to the origin of the address it was saved for.
fn seal(url: &str, secret: &str) -> Option<String> {
    Some(format!("{SEALED}{}\n{secret}", origin(url)?))
}

/// The secret in `stored`, if it was saved for `url`'s origin.
fn unseal(stored: &str, url: &str) -> Option<String> {
    let (bound, secret) = stored.strip_prefix(SEALED)?.split_once('\n')?;
    (origin(url)? == bound).then(|| secret.to_owned())
}

/// A ROM downloaded into the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct CachedRom {
    /// The ROM's folder in the cache.
    pub dir: PathBuf,
    pub size: u64,
    /// What the emulator opens: one file, an `.m3u` or each disc.
    pub roms: Vec<PathBuf>,
    /// Unix milliseconds.
    pub cached_at_ms: i64,
    pub last_used_ms: i64,
}

/// A remote ROM in the library (the `romm_roms` table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RommRom {
    pub server_id: String,
    pub rom_id: u32,
    pub entry_id: String,
    pub game_id: String,
    /// The Caskade platform it launches as.
    pub platform_id: String,
    /// The server's record as last synced.
    pub rom: RemoteRom,
    /// The emulator target the last sync set. An entry whose target differs
    /// was changed on the game page and keeps its own.
    pub synced_target: EmulatorTarget,
    /// "Keep downloaded": never evicted.
    pub pinned: bool,
    /// Deleted on the server but still cached.
    pub orphaned: bool,
    pub cache: Option<CachedRom>,
}

/// How a server answered last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Untested,
    Connected,
    /// Not reachable: cached games still play.
    Offline,
    /// Reachable, but sign-in or the API failed.
    Error,
}

/// A server and what Caskade knows about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub id: String,
    pub state: ConnectionState,
    pub version: Option<String>,
    pub error: Option<String>,
    /// Unix milliseconds of the last finished sync this session.
    pub last_sync_ms: Option<i64>,
    pub syncing: bool,
    /// Library games from this server.
    pub games: u32,
    pub has_secret: bool,
}

/// A RomM platform and the Caskade platform it would map to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformMatch {
    pub platform: RemotePlatform,
    /// From the platform definitions (IGDB id, slug or name).
    pub suggested: Option<String>,
}

/// What "Test connection" found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionReport {
    pub version: Option<String>,
    pub platforms: Vec<PlatformMatch>,
    pub collections: Vec<RemoteCollection>,
}

/// What one sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RommSyncReport {
    pub server_id: String,
    pub added: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub removed: u32,
    /// Deleted on the server, kept because they are cached.
    pub orphaned: u32,
    /// RomM platforms with no Caskade platform to launch them as.
    pub unmatched_platforms: Vec<String>,
    pub warnings: Vec<String>,
}

/// One cached game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub server_id: String,
    pub rom_id: u32,
    pub entry_id: String,
    pub game_id: String,
    pub title: String,
    pub size: u64,
    pub last_used_ms: i64,
    pub pinned: bool,
    pub favorite: bool,
    pub orphaned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheStatus {
    pub dir: PathBuf,
    pub used: u64,
    /// `None` is unlimited.
    pub limit: Option<u64>,
    pub games: Vec<CacheEntry>,
}

/// The whole RomM panel: settings, server states and the cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RommOverview {
    pub settings: RommSettings,
    pub servers: Vec<ServerStatus>,
    pub cache: CacheStatus,
    pub downloads: Vec<DownloadProgress>,
}

/// A RomM game's download state, for the library and the game page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RommGameStatus {
    pub entry_id: String,
    pub server_id: String,
    /// `cached`, `remote` (downloads at launch), `unavailable` (not cached
    /// and the server can't be reached) or `orphaned`. Themes style it
    /// through `data-status`.
    pub status: String,
    pub pinned: bool,
    pub size: Option<u64>,
    pub message: Option<String>,
    pub download: Option<DownloadProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    Downloading,
    Verifying,
    Unpacking,
    Done,
    Failed,
    Cancelled,
}

/// A download's progress; every change is sent to [`RommService::subscribe`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub entry_id: String,
    pub game_id: String,
    pub title: String,
    pub server_id: String,
    pub phase: DownloadPhase,
    pub received: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ServerState {
    state: Option<ConnectionState>,
    version: Option<String>,
    error: Option<String>,
    last_sync_ms: Option<i64>,
    syncing: bool,
    /// ROMs the last sync found gone from the server but kept, because so
    /// many went at once that the listing looked wrong. A sync the user
    /// starts that finds them gone again removes them.
    held: BTreeSet<u32>,
}

/// How long RomM requests may take.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub(crate) struct Timeouts {
    pub connect: Duration,
    /// Silence on a connection, downloads included.
    pub read: Duration,
    /// The whole of a listing, image or sign-in request (not a download).
    pub request: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            read: Duration::from_secs(60),
            request: Duration::from_secs(120),
        }
    }
}

struct Download {
    progress: DownloadProgress,
    cancel: CancellationToken,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    settings: SettingsService,
    emulation: EmulationService,
    artwork: Option<ArtworkCache>,
    secrets: Arc<dyn SecretStore>,
    #[cfg_attr(not(feature = "net"), allow(dead_code))]
    timeouts: Timeouts,
    /// The cache folder when no storage location exists.
    fallback_cache: PathBuf,
    servers: Mutex<BTreeMap<String, ServerState>>,
    downloads: Mutex<BTreeMap<String, Download>>,
    /// One preparation per entry at a time.
    preparing: Locks,
    /// One sync per server at a time.
    syncing: Locks,
    /// One screenshot download per entry at a time.
    #[cfg_attr(not(feature = "net"), allow(dead_code))]
    page_images: Locks,
    /// One eviction pass at a time.
    evicting: tokio::sync::Mutex<()>,
    updates: broadcast::Sender<DownloadProgress>,
}

/// RomM servers, their sync and the download cache; cheap to clone.
#[derive(Clone)]
pub struct RommService {
    inner: Arc<Inner>,
}

type Locks = Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>;

fn keyed(map: &Locks, key: &str) -> Arc<tokio::sync::Mutex<()>> {
    lock(map).entry(key.to_owned()).or_default().clone()
}

impl RommService {
    /// Credentials go to the OS keychain; `fallback_cache` holds downloads
    /// while there is no storage location.
    pub fn new(
        store: LibraryStore,
        bus: Bus,
        settings: SettingsService,
        emulation: EmulationService,
        fallback_cache: impl Into<PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                settings,
                emulation,
                artwork: None,
                secrets: Arc::new(KeychainSecrets),
                timeouts: Timeouts::default(),
                fallback_cache: fallback_cache.into(),
                servers: Mutex::default(),
                downloads: Mutex::default(),
                preparing: Mutex::default(),
                syncing: Mutex::default(),
                page_images: Mutex::default(),
                evicting: tokio::sync::Mutex::new(()),
                updates: broadcast::channel(256).0,
            }),
        }
    }

    /// Imports covers into the artwork cache. Configure before cloning.
    pub fn with_artwork(mut self, artwork: ArtworkCache) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.artwork = Some(artwork);
        }
        self
    }

    /// Another credential store (tests use [`MemorySecrets`]). Configure
    /// before cloning.
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.secrets = secrets;
        }
        self
    }

    /// Shorter stall (`read`) and request (`request`) timeouts, for tests.
    /// Configure before cloning.
    #[doc(hidden)]
    pub fn with_timeouts(mut self, read: Duration, request: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.timeouts.read = read;
            inner.timeouts.request = request;
        }
        self
    }

    /// Every download change.
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadProgress> {
        self.inner.updates.subscribe()
    }

    pub async fn settings(&self) -> Result<RommSettings> {
        Ok(settings::load(&self.inner.settings).await?)
    }

    async fn server_config(&self, id: &str) -> Result<RommServerConfig> {
        self.settings().await?.server(id).cloned().ok_or_else(|| {
            RommError::Config(crate::tr!("there is no RomM server {server}", server = id))
        })
    }

    fn validate(&self, settings: &RommSettings) -> Result<()> {
        let catalog = self.inner.emulation.catalog();
        let mut ids = std::collections::BTreeSet::new();
        for server in &settings.servers {
            if !ids.insert(server.id.as_str()) {
                return Err(RommError::Config(crate::tr!(
                    "two servers have the id {server}",
                    server = server.id
                )));
            }
            #[cfg(feature = "net")]
            client::base_url(&server.url)?;
            for mapping in &server.platforms {
                if let Some(platform) = &mapping.platform
                    && catalog.platform(platform).is_none()
                {
                    return Err(RommError::Config(crate::tr!(
                        "unknown platform {platform}",
                        platform = platform
                    )));
                }
                if mapping.emulator_id.is_some() != mapping.profile_id.is_some() {
                    return Err(RommError::Config(crate::tr!(
                        "choose both an emulator and a profile, or neither"
                    )));
                }
            }
        }
        if !settings.cache.dir.trim().is_empty()
            && !std::path::Path::new(settings.cache.dir.trim()).is_absolute()
        {
            return Err(RommError::Config(crate::tr!(
                "the cache folder must be an absolute path"
            )));
        }
        Ok(())
    }

    /// Replaces `core.romm`. Removed servers go with their games and cache.
    pub async fn save_settings(&self, next: RommSettings) -> Result<RommOverview> {
        self.validate(&next)?;
        let previous = self.settings().await?;
        settings::save(&self.inner.settings, &next).await?;
        for server in previous
            .servers
            .iter()
            .filter(|server| next.server(&server.id).is_none())
        {
            self.forget_server(&server.id).await?;
        }
        if next.cache != previous.cache {
            self.enforce_limit(None, 0).await?;
        }
        self.overview().await
    }

    /// Adds or updates one server; a `secret` (password or API token) goes
    /// to the keychain, `None` keeps the stored one.
    pub async fn save_server(
        &self,
        server: RommServerConfig,
        secret: Option<String>,
    ) -> Result<RommOverview> {
        let mut next = self.settings().await?;
        let previous = next.server(&server.id).cloned();
        let auth_changed = previous
            .as_ref()
            .is_some_and(|old| old.auth != server.auth || old.username != server.username);
        let reconnect = auth_changed
            || secret.as_ref().is_some_and(|secret| !secret.is_empty())
            || previous.as_ref().is_none_or(|old| {
                old.url != server.url || old.allow_invalid_certs != server.allow_invalid_certs
            });
        match next.servers.iter_mut().find(|old| old.id == server.id) {
            Some(old) => *old = server.clone(),
            None => next.servers.push(server.clone()),
        }
        self.validate(&next)?;
        match secret.filter(|secret| !secret.is_empty()) {
            Some(secret) => {
                let sealed = seal(&server.url, &secret).ok_or_else(|| {
                    RommError::Config(crate::tr!("the server address is not a valid URL"))
                })?;
                self.inner.secrets.set(&server.id, &sealed).await?;
            }
            None if auth_changed => self.inner.secrets.delete(&server.id).await?,
            None => {}
        }
        settings::save(&self.inner.settings, &next).await?;
        if reconnect {
            self.set_state(&server.id, |state| {
                state.state = None;
                state.error = None;
            });
        }
        self.overview().await
    }

    /// Removes a server, its credentials, its library games and its cache.
    pub async fn remove_server(&self, id: &str) -> Result<RommOverview> {
        let mut next = self.settings().await?;
        next.servers.retain(|server| server.id != id);
        settings::save(&self.inner.settings, &next).await?;
        self.forget_server(id).await?;
        self.overview().await
    }

    async fn forget_server(&self, id: &str) -> Result<()> {
        let _sync = keyed(&self.inner.syncing, id).lock_owned().await;
        for rom in self.inner.store.romm_roms(id).await? {
            if let Some(cache) = &rom.cache {
                cache::remove_dir(&cache.dir).await;
            }
            self.remove_from_library(&rom).await?;
        }
        if let Err(error) = self.inner.secrets.delete(id).await {
            tracing::warn!(server = id, %error, "RomM credentials not removed");
        }
        lock(&self.inner.servers).remove(id);
        Ok(())
    }

    /// Settings, server states, the cache and running downloads.
    pub async fn overview(&self) -> Result<RommOverview> {
        let settings = self.settings().await?;
        let mut servers = Vec::new();
        for server in &settings.servers {
            servers.push(self.status(server).await?);
        }
        Ok(RommOverview {
            cache: self.cache_status_with(&settings).await?,
            downloads: self.downloads(),
            settings,
            servers,
        })
    }

    async fn status(&self, server: &RommServerConfig) -> Result<ServerStatus> {
        let state = lock(&self.inner.servers)
            .get(&server.id)
            .cloned()
            .unwrap_or_default();
        let has_secret = match self.stored_secret(server).await {
            Ok(secret) => secret.is_some(),
            Err(error) => {
                tracing::debug!(%error, "keychain unavailable");
                false
            }
        };
        Ok(ServerStatus {
            id: server.id.clone(),
            state: state.state.unwrap_or(ConnectionState::Untested),
            version: state.version,
            error: state.error,
            last_sync_ms: state.last_sync_ms,
            syncing: state.syncing,
            games: u32::try_from(self.inner.store.romm_roms(&server.id).await?.len())
                .unwrap_or(u32::MAX),
            has_secret,
        })
    }

    fn set_state(&self, server: &str, change: impl FnOnce(&mut ServerState)) {
        change(
            lock(&self.inner.servers)
                .entry(server.to_owned())
                .or_default(),
        );
    }

    /// Records how a request went: offline, an error or connected.
    fn note<T>(&self, server: &str, outcome: &Result<T>) {
        match outcome {
            Ok(_) => self.set_state(server, |state| {
                state.state = Some(ConnectionState::Connected);
                state.error = None;
            }),
            Err(error) => self.note_error(server, error),
        }
    }

    fn note_error(&self, server: &str, error: &RommError) {
        self.set_state(server, |state| match error {
            RommError::Unreachable(error) => {
                state.state = Some(ConnectionState::Offline);
                state.error = Some(error.clone());
            }
            RommError::Cancelled => {}
            error => {
                state.state = Some(ConnectionState::Error);
                state.error = Some(error.to_string());
            }
        });
    }

    /// The server answered its last request (a sync, a test or a download).
    fn connected(&self, server: &str) -> bool {
        lock(&self.inner.servers)
            .get(server)
            .is_some_and(|state| state.state == Some(ConnectionState::Connected))
    }

    fn offline(&self, server: &str) -> bool {
        lock(&self.inner.servers)
            .get(server)
            .is_some_and(|state| state.state == Some(ConnectionState::Offline))
    }

    #[cfg(feature = "net")]
    async fn client(
        &self,
        config: &RommServerConfig,
        secret: Option<String>,
    ) -> Result<client::Client> {
        let secret = match secret.filter(|secret| !secret.is_empty()) {
            Some(secret) => Some(secret),
            None => self.stored_secret(config).await?,
        };
        client::Client::new(config, secret, self.inner.timeouts)
    }

    /// The stored password or token, if it was saved for `config`'s
    /// address. Settings can name another address for a server id; the
    /// secret never follows it there (#157).
    async fn stored_secret(&self, config: &RommServerConfig) -> Result<Option<String>> {
        let Some(stored) = self.inner.secrets.get(&config.id).await? else {
            return Ok(None);
        };
        if stored.starts_with(SEALED) {
            return Ok(unseal(&stored, &config.url));
        }
        // Saved before secrets named their address: bind it to the address
        // saved for the server now.
        let Some(saved) = self.settings().await?.server(&config.id).cloned() else {
            return Ok(None);
        };
        let Some(sealed) = seal(&saved.url, &stored) else {
            return Ok(None);
        };
        self.inner.secrets.set(&config.id, &sealed).await?;
        Ok(unseal(&sealed, &config.url))
    }

    /// "Test connection": the server's version, platforms (with the Caskade
    /// platform each matches) and collections. `secret` tries a password
    /// or token before it is saved.
    pub async fn test_connection(
        &self,
        server: RommServerConfig,
        secret: Option<String>,
    ) -> Result<ConnectionReport> {
        let outcome = self.test_inner(&server, secret).await;
        if self.settings().await?.server(&server.id).is_some() {
            let was_offline = self.offline(&server.id);
            self.note(&server.id, &outcome);
            if let Ok(report) = &outcome {
                self.set_state(&server.id, |state| state.version = report.version.clone());
                if was_offline {
                    self.mark_available(&server.id).await?;
                }
                // Evictions held back while it was unreachable.
                if let Err(error) = self.enforce_limit(None, 0).await {
                    tracing::warn!(server = server.id, %error, "RomM cache limit not enforced");
                }
            }
        }
        outcome
    }

    #[cfg(feature = "net")]
    async fn test_inner(
        &self,
        server: &RommServerConfig,
        secret: Option<String>,
    ) -> Result<ConnectionReport> {
        let client = self.client(server, secret).await?;
        let version = client.heartbeat().await?;
        let platforms = client.platforms().await?;
        let collections = client.collections().await.unwrap_or_default();
        let catalog = self.inner.emulation.catalog();
        Ok(ConnectionReport {
            version,
            platforms: platforms
                .into_iter()
                .map(|platform| PlatformMatch {
                    suggested: sync::suggest(&catalog, &platform),
                    platform,
                })
                .collect(),
            collections,
        })
    }

    #[cfg(not(feature = "net"))]
    async fn test_inner(
        &self,
        _server: &RommServerConfig,
        _secret: Option<String>,
    ) -> Result<ConnectionReport> {
        Err(offline_build())
    }

    /// Downloads in progress; a download leaves the list when it ends.
    pub fn downloads(&self) -> Vec<DownloadProgress> {
        lock(&self.inner.downloads)
            .values()
            .map(|download| download.progress.clone())
            .collect()
    }

    /// Cancels a running download; `false` when there is none.
    pub fn cancel_download(&self, entry_id: &str) -> bool {
        lock(&self.inner.downloads)
            .get(entry_id)
            .filter(|download| {
                matches!(
                    download.progress.phase,
                    DownloadPhase::Downloading
                        | DownloadPhase::Verifying
                        | DownloadPhase::Unpacking
                )
            })
            .map(|download| download.cancel.cancel())
            .is_some()
    }

    fn publish(&self, progress: DownloadProgress) {
        if let Some(download) = lock(&self.inner.downloads).get_mut(&progress.entry_id) {
            download.progress = progress.clone();
        }
        let _ = self.inner.updates.send(progress);
    }

    /// A RomM game's cache state; `None` for other entries.
    pub async fn game_status(&self, entry_id: &str) -> Result<Option<RommGameStatus>> {
        let Some(rom) = self.inner.store.romm_rom_by_entry(entry_id).await? else {
            return Ok(None);
        };
        let download = lock(&self.inner.downloads)
            .get(entry_id)
            .map(|download| download.progress.clone());
        let (status, message) = if rom.orphaned {
            (
                "orphaned",
                Some(crate::tr!(
                    "Deleted on the RomM server; kept while it is downloaded."
                )),
            )
        } else if rom.cache.is_some() {
            ("cached", None)
        } else if rom.rom.missing_from_fs {
            ("unavailable", Some(missing_from_fs()))
        } else if self.offline(&rom.server_id) {
            ("unavailable", Some(self.unavailable(&rom.server_id).await))
        } else {
            ("remote", None)
        };
        Ok(Some(RommGameStatus {
            entry_id: entry_id.to_owned(),
            server_id: rom.server_id.clone(),
            status: status.into(),
            pinned: rom.pinned,
            size: rom
                .cache
                .as_ref()
                .map(|cache| cache.size)
                .or_else(|| (rom.rom.fs_size_bytes > 0).then_some(rom.rom.fs_size_bytes)),
            message,
            download,
        }))
    }

    /// The message an uncached game shows while its server is offline.
    async fn unavailable(&self, server_id: &str) -> String {
        let name = self
            .server_config(server_id)
            .await
            .map(|server| server.name)
            .ok()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| server_id.to_owned());
        crate::tr!(
            "Not downloaded yet, and the RomM server \u{201c}{server}\u{201d} can't be reached. \
             Games you've played before still work offline.",
            server = name
        )
    }
}

/// Why a ROM the server lost track of on its disk can't download.
fn missing_from_fs() -> String {
    crate::tr!(
        "The RomM server can't find this game's files on its disk. \
         It's playable again once the server finds them."
    )
}

#[cfg(not(feature = "net"))]
fn offline_build() -> RommError {
    RommError::Unreachable(crate::tr!("this build has no network support"))
}

/// The launch pipeline's preparation step: a RomM game downloads first.
#[async_trait::async_trait]
impl crate::launch::LaunchPreparer for RommService {
    async fn prepare(&self, entry_id: &str) -> crate::launch::Result<()> {
        match self.prepare_entry(entry_id).await {
            Ok(_) => Ok(()),
            Err(RommError::Cancelled) => Err(crate::launch::LaunchError::Cancelled),
            Err(error) => Err(crate::launch::LaunchError::Invalid(error.to_string())),
        }
    }
}
