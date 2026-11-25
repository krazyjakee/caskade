//! IGDB data types and decimal/URL serde helpers.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;

/// A game as the metadata server describes it. Ids serialize as decimal
/// strings in JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbGame {
    #[serde(with = "decimal")]
    pub id: u64,
    #[serde(default)]
    pub slug: Option<String>,
    /// The game's IGDB page.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub storyline: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Themes, then game modes, then player perspectives.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<IgdbPlatform>,
    /// `YYYY-MM-DD` (UTC).
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// IGDB's aggregate rating (0–100), not the user's own.
    #[serde(default)]
    pub total_rating: Option<f64>,
    /// Store ids (Steam, GOG, Epic, …), for matching library entries.
    #[serde(default)]
    pub external_ids: Vec<IgdbExternalId>,
    /// IGDB's raw `game_type` reference.
    #[serde(default)]
    pub game_type: Option<Value>,
    /// Set on editions and other versions of a parent game.
    #[serde(default, with = "decimal_option")]
    pub version_parent: Option<u64>,
    #[serde(default)]
    pub images: IgdbImages,
    /// Each board's rating ([`super::age`] turns them into a minimum age). Empty
    /// when IGDB has none, and in answers from servers before ratings.
    #[serde(default)]
    pub age_ratings: Vec<IgdbAgeRating>,
    /// One per platform and region, dated ones first.
    #[serde(default)]
    pub release_dates: Vec<IgdbReleaseDate>,
    /// Storefront pages IGDB knows for the game. A link says nothing of
    /// whether it is on sale there, or at what price.
    #[serde(default)]
    pub store_links: Vec<IgdbStoreLink>,
    /// Every other website: official, wiki, social, video, community.
    #[serde(default)]
    pub websites: Vec<IgdbWebsite>,
    /// IGDB's similar games, without the game itself or duplicates.
    #[serde(default, with = "decimals")]
    pub similar_games: Vec<u64>,
    /// IGDB's game status: `Released`, `Early Access`, `Cancelled`, …
    #[serde(default)]
    pub status: Option<String>,
    /// Trailers and other videos on YouTube, in IGDB's order. Empty in
    /// answers from servers before videos.
    #[serde(default)]
    pub videos: Vec<IgdbVideo>,
}

/// A video of the game on YouTube.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbVideo {
    /// The YouTube video id; the server sends only well-formed ones.
    pub video_id: String,
    /// IGDB's name for it: `Trailer`, `Gameplay video`, …
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbPlatform {
    /// IGDB's platform id, for the catalog's platform filter.
    #[serde(default, with = "decimal_option")]
    pub id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub abbreviation: Option<String>,
}

/// One board's rating: `board` is `pegi`, `esrb`, `usk`, `cero`, `acb`,
/// `grac`, `classind` or a board IGDB adds later; `rating` its label
/// (`18`, `M`, `E10+`, `MA15+`, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbAgeRating {
    pub board: String,
    pub rating: String,
}

/// How much of a release date is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleasePrecision {
    Day,
    Month,
    Quarter,
    Year,
    /// Undated, or a precision this build doesn't know.
    #[default]
    #[serde(other)]
    Tbd,
}

/// A release on one platform in one region.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbReleaseDate {
    #[serde(default)]
    pub platform: Option<IgdbPlatform>,
    /// IGDB's region name: `europe`, `north_america`, `worldwide`, …
    #[serde(default)]
    pub region: Option<String>,
    /// The UTC day IGDB files it under (`YYYY-MM-DD`). For month, quarter
    /// and year precision it stands in for the whole span: sort by it, but
    /// show only what `precision` covers.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub precision: ReleasePrecision,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub month: Option<u8>,
    /// 1–4, for quarter precision.
    #[serde(default)]
    pub quarter: Option<u8>,
    /// IGDB's own text for it ("Q3 2026", "TBD").
    #[serde(default)]
    pub human: Option<String>,
    /// `Full Release`, `Early Access`, `Cancelled`, …
    #[serde(default)]
    pub status: Option<String>,
}

/// A storefront's page for the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbStoreLink {
    /// `steam`, `gog`, `epic`, `itch`, `playstation`, `nintendo`, …
    pub store: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
    /// Platform names it is for; empty when IGDB doesn't say.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// ISO 3166-1 numeric codes it is listed for; empty when IGDB doesn't say.
    #[serde(default)]
    pub countries: Vec<u32>,
}

/// Any other website of the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbWebsite {
    /// `official`, `wiki`, `wikipedia`, `social`, `video`, `community` or `other`.
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbExternalId {
    pub uid: String,
    /// IGDB's raw `external_game_source` reference.
    #[serde(default)]
    pub source: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbImages {
    #[serde(default)]
    pub cover: Option<IgdbImage>,
    /// The best banner among the artworks.
    #[serde(default)]
    pub hero: Option<IgdbImage>,
    #[serde(default)]
    pub logo: Option<IgdbImage>,
    #[serde(default)]
    pub artworks: Vec<IgdbImage>,
    #[serde(default)]
    pub screenshots: Vec<IgdbImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbImage {
    pub image_id: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub alpha_channel: Option<bool>,
    #[serde(default)]
    pub image_type: Option<Value>,
    /// The download URL, at the size the server picked for this kind;
    /// always `https:` ([`https_image_url`]).
    #[serde(deserialize_with = "https_image")]
    pub url: String,
}

/// What a lookup found.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbLookup {
    /// In the order asked for, without duplicates.
    pub games: Vec<IgdbGame>,
    /// Ids IGDB has no game for.
    #[serde(with = "decimals")]
    pub not_found: Vec<u64>,
    /// Ids without an answer yet: the server couldn't be reached or sent nothing usable for them.
    #[serde(with = "decimals")]
    pub pending: Vec<u64>,
}

/// A game's id in a store IGDB links to. `store` is the server's name for
/// it: `steam`, `gog`, `epic`, `itch`, `amazon`, `amazon_adg` or
/// `microsoft`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoreId {
    pub store: String,
    pub uid: String,
}

impl StoreId {
    pub fn new(store: &str, uid: impl Into<String>) -> Self {
        Self {
            store: store.to_owned(),
            uid: uid.into(),
        }
    }
}

/// An IGDB id serialized as either a JSON number or a decimal string.
#[derive(Deserialize)]
#[serde(untagged)]
enum Decimal {
    Number(u64),
    Text(String),
}

impl Decimal {
    fn parse<E: de::Error>(self) -> Result<u64, E> {
        match self {
            Self::Number(id) => Ok(id),
            Self::Text(text) => text.parse().map_err(E::custom),
        }
    }
}

/// `url` with IGDB's protocol-relative form (`//images.igdb.com/…`) made
/// `https:`, so the artwork cache and the Store's image fetch, which take
/// only absolute https URLs, accept it (#163). Other URLs are unchanged.
pub fn https_image_url(url: &str) -> String {
    let url = url.trim();
    match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    }
}

fn https_image<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|url| https_image_url(&url))
}

pub(crate) mod decimal {
    use super::*;

    pub fn serialize<S: Serializer>(id: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(id)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        Decimal::deserialize(deserializer)?.parse()
    }
}

pub(crate) mod decimal_option {
    use super::*;

    pub fn serialize<S: Serializer>(id: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        id.map(|id| id.to_string()).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Option::<Decimal>::deserialize(deserializer)?
            .map(Decimal::parse)
            .transpose()
    }
}

pub(crate) mod decimals {
    use super::*;

    pub fn serialize<S: Serializer>(ids: &[u64], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(ids.iter().map(u64::to_string))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u64>, D::Error> {
        Vec::<Decimal>::deserialize(deserializer)?
            .into_iter()
            .map(Decimal::parse)
            .collect()
    }
}
