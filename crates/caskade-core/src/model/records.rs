use super::{ListGames, new_id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkKind {
    Cover,
    Hero,
    Logo,
    Icon,
    Screenshot,
}
impl From<caskade_discovery::ArtworkKind> for ArtworkKind {
    fn from(kind: caskade_discovery::ArtworkKind) -> Self {
        match kind {
            caskade_discovery::ArtworkKind::Cover => Self::Cover,
            caskade_discovery::ArtworkKind::Header | caskade_discovery::ArtworkKind::Hero => {
                Self::Hero
            }
            caskade_discovery::ArtworkKind::Logo => Self::Logo,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkThumbnail {
    pub size: u32,
    pub path: String,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    pub id: String,
    pub content_hash: String,
    pub game_id: String,
    pub kind: ArtworkKind,
    pub source: String,
    /// Relative to the artwork cache, including every thumbnail path.
    pub cached_path: String,
    pub width: u32,
    pub height: u32,
    pub thumbnails: Vec<ArtworkThumbnail>,
}
impl Artwork {
    pub fn new(
        game_id: impl Into<String>,
        kind: ArtworkKind,
        content_hash: impl Into<String>,
        source: impl Into<String>,
        cached_path: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            id: new_id(),
            game_id: game_id.into(),
            kind,
            content_hash: content_hash.into(),
            source: source.into(),
            cached_path: cached_path.into(),
            width,
            height,
            thumbnails: vec![],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaySession {
    pub id: String,
    pub game_id: String,
    pub entry_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    /// Monotonic elapsed playtime supplied by the tracking service, not end-start.
    pub duration_ms: u64,
    pub heartbeat_at_ms: i64,
    /// Tracking service's versioned recovery payload (PID + process start time).
    pub tracking: Option<serde_json::Value>,
}
impl PlaySession {
    pub fn new(
        game_id: impl Into<String>,
        entry_id: impl Into<String>,
        started_at_ms: i64,
    ) -> Self {
        Self {
            id: new_id(),
            game_id: game_id.into(),
            entry_id: entry_id.into(),
            started_at_ms,
            ended_at_ms: None,
            duration_ms: 0,
            heartbeat_at_ms: started_at_ms,
            tracking: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CollectionKind {
    Manual,
    Smart { query: ListGames },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Pinned collections come first.
    pub pinned: bool,
    pub kind: CollectionKind,
    /// A manual collection's games, in the order they were added.
    pub game_ids: Vec<String>,
    /// When it was made, or last changed (its games included).
    pub updated_at_ms: i64,
}
impl Collection {
    pub fn manual(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            description: String::new(),
            pinned: false,
            kind: CollectionKind::Manual,
            game_ids: vec![],
            updated_at_ms: crate::now_ms(),
        }
    }
}
/// A new collection, or an existing one's name, description and rules.
/// Its pin changes through `set_collection_pinned`, a manual one's games
/// through `add_to_collection` and `remove_from_collection`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveCollection {
    /// `None` makes a new collection.
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub kind: CollectionKind,
}
/// A collection as its card shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub pinned: bool,
    pub kind: CollectionKind,
    pub updated_at_ms: i64,
    /// Its games that aren't hidden.
    pub games: u64,
    pub playtime_ms: u64,
    /// The covers of its first four games (those that have one).
    pub covers: Vec<Artwork>,
}

/// A release's identity at an achievement service that its store ids don't
/// give (#189).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementLink {
    pub entry_id: String,
    pub provider: String,
    /// The service's game id.
    pub product_id: String,
    pub method: LinkMethod,
    pub linked_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum LinkMethod {
    /// The service recognised this hash of the release's ROM.
    Hash { hash: String },
    /// The user chose the game.
    Manual,
    /// No game could be verified: an unsupported system or file, a ROM the
    /// service doesn't know (another revision or region), or more than one
    /// game. `product_id` is empty. Tried again after a while; never
    /// guessed from the title.
    Unresolved { reason: String },
}
