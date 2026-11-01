use super::{Artwork, LaunchSpec, new_id};
use caskade_discovery::{ExternalId, GameRuntime, InstallState, SourceId, TrackingHint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub title: String,
    pub sort_title: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    /// ISO calendar date (YYYY-MM-DD), without an invented time zone.
    pub release_date: Option<String>,
    pub platforms: Vec<String>,
    pub favorite: bool,
    pub hidden: bool,
    /// User rating, 0 through 100.
    pub rating: Option<u8>,
    pub notes: String,
    /// The entry the game's Play starts, when it has several (releases on
    /// different platforms). Without one, or when it is gone, the first
    /// that can launch.
    #[serde(default)]
    pub default_entry: Option<String>,
}
impl Game {
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            id: new_id(),
            sort_title: title.clone(),
            title,
            description: String::new(),
            aliases: vec![],
            genres: vec![],
            tags: vec![],
            release_date: None,
            platforms: vec![],
            favorite: false,
            hidden: false,
            rating: None,
            notes: String::new(),
            default_entry: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum InstallLocation {
    Managed { root_id: String, rel_dir: PathBuf },
    External { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    pub id: String,
    pub game_id: String,
    pub source_id: SourceId,
    /// Stable source-local key, including sources that have no store ID.
    pub external_id: String,
    /// Cross-source store identities, shared with discovery.
    pub ids: Vec<ExternalId>,
    pub state: InstallState,
    pub location: Option<InstallLocation>,
    pub size: Option<u64>,
    pub launch: Option<LaunchSpec>,
    pub runtime: GameRuntime,
    pub tracking: Option<TrackingHint>,
    /// Stable installer alias identifier, never a stored alias path.
    pub alias_id: Option<String>,
}
impl GameEntry {
    pub fn new(
        game_id: impl Into<String>,
        source_id: SourceId,
        external_id: impl Into<String>,
    ) -> Self {
        Self {
            id: new_id(),
            game_id: game_id.into(),
            source_id,
            external_id: external_id.into(),
            ids: vec![],
            state: InstallState::Partial,
            location: None,
            size: None,
            launch: None,
            runtime: GameRuntime::Native,
            tracking: None,
            alias_id: None,
        }
    }
    /// Preserve discovery's store identities, runtime and tracking information.
    pub fn from_detected(
        game_id: impl Into<String>,
        detected: &caskade_discovery::DetectedGame,
    ) -> Self {
        Self {
            id: new_id(),
            game_id: game_id.into(),
            source_id: detected.source.clone(),
            external_id: detected.key.clone(),
            ids: detected.ids.clone(),
            state: detected.state.clone(),
            location: detected
                .install_dir
                .clone()
                .map(|path| InstallLocation::External { path }),
            size: detected.size,
            launch: detected.launch.first().cloned().map(LaunchSpec::from),
            runtime: detected.runtime.clone(),
            tracking: detected.tracking.clone(),
            alias_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageRoot {
    /// Exactly the identifier in `<path>/.caskade/root.json`.
    pub id: String,
    pub path: PathBuf,
    pub is_default: bool,
    pub label: Option<String>,
}
impl StorageRoot {
    pub fn from_manifest(
        path: PathBuf,
        manifest: &caskade_installer::manifest::RootManifest,
    ) -> Self {
        Self {
            id: manifest.root_id().to_owned(),
            path,
            is_default: false,
            label: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum GameFilter {
    #[default]
    All,
    Installed,
    Favorites,
    Recent,
    Emulated,
    /// Games played in the game window (`LaunchSpec::Web`).
    Web,
    /// Owned by a store account and not installed: ready to install (#170).
    Owned,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum GameSort {
    #[default]
    Title,
    Recent,
    Playtime,
    /// The last added first: the game, or its newest entry (a release on
    /// another platform, say).
    Added,
    /// The newest release date first; games without one last.
    Released,
    /// IGDB Visits PopScore, highest first; missing scores follow by title.
    Popular,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct ListGames {
    pub query: Option<String>,
    pub filter: GameFilter,
    /// Only games on this platform.
    pub platform: Option<String>,
    pub sort: GameSort,
    pub offset: u32,
    pub limit: u32,
}
impl Default for ListGames {
    fn default() -> Self {
        Self {
            query: None,
            filter: GameFilter::All,
            platform: None,
            sort: GameSort::Title,
            offset: 0,
            limit: 50,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    /// The querying account needs an admin PIN; a search is not an approval.
    #[serde(default)]
    pub admin_pin_needed: bool,
    pub id: String,
    pub title: String,
    pub sort_title: String,
    pub description: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    /// ISO calendar date (YYYY-MM-DD).
    pub release_date: Option<String>,
    pub platforms: Vec<String>,
    pub favorite: bool,
    pub hidden: bool,
    pub rating: Option<u8>,
    pub cover: Option<Artwork>,
    /// The game's icon, for compact rows.
    pub icon: Option<Artwork>,
    pub installed: bool,
    pub playtime_ms: u64,
    pub last_played_at_ms: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GamePage {
    pub items: Vec<GameSummary>,
    pub total: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameDetails {
    pub game: Game,
    pub entries: Vec<GameEntry>,
    pub artwork: Vec<Artwork>,
    pub playtime_ms: u64,
    pub last_played_at_ms: Option<i64>,
}
/// The first game whose title starts with `letter` (`#` for anything but
/// A to Z), as an index into the library sorted by title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct TitleLetter {
    pub letter: String,
    pub offset: u64,
}
/// A platform's games in one library filter, for the platform breakdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSummary {
    pub name: String,
    pub games: u64,
    pub playtime_ms: u64,
    pub last_played_at_ms: Option<i64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub total: u64,
    pub installed: u64,
    pub favorites: u64,
    pub recent: u64,
    pub emulated: u64,
    pub web: u64,
    /// Owned by a store account and not installed (#170).
    pub owned: u64,
    pub playtime_ms: u64,
}
