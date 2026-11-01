//! `core.romm`: RomM servers, what they import, how their platforms map to
//! Caskade's and the download cache. Passwords and tokens are never part
//! of it: they live in the OS keychain ([`super::secrets`]).

use crate::settings::{SectionDefinition, SettingsService};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

pub const NAMESPACE: &str = "core.romm";

/// How Caskade signs in to a server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum RommAuth {
    /// Username and password, exchanged for an access token (`/api/token`).
    #[default]
    Password,
    /// An API token made in RomM (`rmm_…`), sent as a bearer token.
    Token,
}

/// One RomM platform's import and launch choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RommPlatformMapping {
    /// RomM's platform slug, e.g. `snes`.
    #[schemars(length(min = 1, max = 128))]
    pub slug: String,
    /// Import this platform's ROMs (ignored while the server imports all).
    pub enabled: bool,
    /// The Caskade platform; `None` matches it from the platform definitions.
    #[schemars(length(min = 1, max = 128))]
    pub platform: Option<String>,
    /// Launch with this emulator profile instead of the platform's default.
    #[schemars(length(min = 1, max = 128))]
    pub emulator_id: Option<String>,
    #[schemars(length(min = 1, max = 128))]
    pub profile_id: Option<String>,
}

/// A RomM server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RommServerConfig {
    /// Stable id: the library source is `romm:<id>`.
    #[schemars(regex(pattern = "^[a-z0-9][a-z0-9-]{0,31}$"))]
    pub id: String,
    #[schemars(length(max = 128))]
    pub name: String,
    /// `http(s)://host[:port][/prefix]`.
    #[schemars(regex(pattern = "^https?://\\S+$"), length(max = 2048))]
    pub url: String,
    pub auth: RommAuth,
    #[schemars(length(max = 256))]
    pub username: String,
    /// Accept self-signed or otherwise invalid certificates (a LAN server).
    pub allow_invalid_certs: bool,
    /// Import every platform and collection, including ones added later.
    pub import_all: bool,
    #[schemars(length(max = 512))]
    pub platforms: Vec<RommPlatformMapping>,
    /// RomM collection ids whose ROMs are imported even when their platform
    /// is off.
    #[schemars(length(max = 512))]
    pub collections: Vec<u32>,
    pub sync_on_startup: bool,
    #[schemars(regex(pattern = "^(manual|1h|6h|24h)$"))]
    pub sync_interval: String,
}

impl RommServerConfig {
    pub fn new(id: impl Into<String>, name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            url: url.into(),
            auth: RommAuth::Password,
            username: String::new(),
            allow_invalid_certs: false,
            import_all: true,
            platforms: Vec::new(),
            collections: Vec::new(),
            sync_on_startup: true,
            sync_interval: "6h".into(),
        }
    }

    pub fn mapping(&self, slug: &str) -> Option<&RommPlatformMapping> {
        self.platforms.iter().find(|mapping| mapping.slug == slug)
    }

    /// The background sync interval, if any.
    pub fn interval(&self) -> Option<std::time::Duration> {
        let hours = match self.sync_interval.as_str() {
            "1h" => 1,
            "6h" => 6,
            "24h" => 24,
            _ => return None,
        };
        Some(std::time::Duration::from_secs(hours * 60 * 60))
    }
}

/// The download cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RommCacheSettings {
    /// An absolute folder; empty is `romm-cache/` on the default storage
    /// location (or in the app's data folder when there is none).
    #[schemars(length(max = 4096))]
    pub dir: String,
    /// GiB; 0 is unlimited. The least recently played games go first.
    #[schemars(range(max = 65536))]
    pub limit_gb: u32,
    /// Never evict favourites.
    pub keep_favorites: bool,
    /// Check downloads against the hashes RomM reports.
    pub verify: bool,
}

impl Default for RommCacheSettings {
    fn default() -> Self {
        Self {
            dir: String::new(),
            limit_gb: 100,
            keep_favorites: true,
            verify: true,
        }
    }
}

impl RommCacheSettings {
    pub fn limit_bytes(&self) -> Option<u64> {
        (self.limit_gb > 0).then(|| u64::from(self.limit_gb) << 30)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RommSettings {
    #[schemars(length(max = 16))]
    pub servers: Vec<RommServerConfig>,
    pub cache: RommCacheSettings,
}

impl RommSettings {
    pub fn server(&self, id: &str) -> Option<&RommServerConfig> {
        self.servers.iter().find(|server| server.id == id)
    }
}

/// Registers `core.romm`.
pub fn register_settings(settings: &SettingsService) -> crate::settings::Result<()> {
    settings.register(SectionDefinition::typed::<RommSettings>(NAMESPACE, 1)?)
}

pub(crate) async fn load(settings: &SettingsService) -> crate::Result<RommSettings> {
    let snapshot = settings
        .get(NAMESPACE)
        .await
        .map_err(|error| crate::Error::Invalid(error.to_string()))?;
    Ok(serde_json::from_value(snapshot.value)?)
}

pub(crate) async fn save(settings: &SettingsService, value: &RommSettings) -> crate::Result<()> {
    settings
        .set(NAMESPACE, serde_json::to_value(value)?, None)
        .await
        .map_err(|error| crate::Error::Invalid(error.to_string()))?;
    Ok(())
}
