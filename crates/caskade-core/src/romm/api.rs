//! What the RomM REST API returns (`/api/heartbeat`, `/api/platforms`,
//! `/api/collections`, `/api/roms`), reduced to the fields Caskade uses.
//! Every field the client can live without has a default, so older and
//! newer servers that add or drop fields still parse.
#![cfg_attr(not(feature = "net"), allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

/// `GET /api/heartbeat` (no authentication).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Heartbeat {
    #[serde(default, rename = "SYSTEM")]
    pub system: HeartbeatSystem,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct HeartbeatSystem {
    #[serde(default, rename = "VERSION")]
    pub version: Option<String>,
}

/// `GET /api/firmware?platform_id=` (`FirmwareSchema`): a BIOS or firmware
/// file the server keeps for a platform.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RemoteFirmware {
    pub id: u32,
    pub platform_id: u32,
    pub file_name: String,
    #[serde(default)]
    pub md5_hash: Option<String>,
    /// The file is gone from the server's disk.
    #[serde(default)]
    pub missing_from_fs: bool,
}

/// `POST /api/token` (OAuth2 password grant).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
    /// Seconds.
    #[serde(default)]
    pub expires: Option<u64>,
}

/// A platform on the server (`PlatformSchema`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlatform {
    pub id: u32,
    pub slug: String,
    #[serde(default, alias = "fs_slug")]
    pub fs_slug: String,
    pub name: String,
    #[serde(default, alias = "custom_name")]
    pub custom_name: Option<String>,
    #[serde(default, alias = "rom_count")]
    pub rom_count: u32,
    #[serde(default, alias = "igdb_id")]
    pub igdb_id: Option<u32>,
}

impl RemotePlatform {
    pub fn display_name(&self) -> &str {
        self.custom_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.name)
    }
}

/// A collection on the server (`CollectionSchema`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCollection {
    pub id: u32,
    pub name: String,
    #[serde(default, alias = "rom_ids")]
    pub rom_ids: Vec<u32>,
}

/// One page of `GET /api/roms`. `items` is required: an answer without it
/// is not a page, and reading it as an empty one would take every ROM as
/// deleted.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RomPage {
    pub items: Vec<RemoteRom>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// A file of a ROM (`RomFileSchema`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    #[serde(default)]
    pub id: Option<u32>,
    #[serde(alias = "file_name")]
    pub file_name: String,
    /// The folder on the server, relative to its library.
    #[serde(default, alias = "file_path")]
    pub file_path: String,
    #[serde(default, alias = "file_size_bytes")]
    pub file_size_bytes: u64,
    #[serde(default, alias = "crc_hash")]
    pub crc_hash: Option<String>,
    #[serde(default, alias = "md5_hash")]
    pub md5_hash: Option<String>,
    #[serde(default, alias = "sha1_hash")]
    pub sha1_hash: Option<String>,
    /// `game`, `dlc`, `hack`, `manual`, … (RomM's `RomFileCategory`); only
    /// top-level game files are played.
    #[serde(default)]
    pub category: Option<String>,
}

/// RomM's metadata block (`RomMetadataSchema`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMetadata {
    #[serde(default)]
    pub genres: Vec<String>,
    /// Unix time; RomM gives milliseconds (seconds on older servers).
    #[serde(default, alias = "first_release_date")]
    pub first_release_date: Option<i64>,
}

/// Screenshots imported per ROM: the game page's strip.
const SCREENSHOTS: usize = 4;
/// The model's limits on a game's aliases.
const MAX_ALIASES: usize = 256;
const MAX_TEXT: usize = 4096;

/// A ROM (`SimpleRomSchema`): one game on the server, with one or more files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRom {
    pub id: u32,
    #[serde(alias = "platform_id")]
    pub platform_id: u32,
    #[serde(default, alias = "platform_slug")]
    pub platform_slug: String,
    #[serde(alias = "fs_name")]
    pub fs_name: String,
    #[serde(default, alias = "fs_name_no_ext")]
    pub fs_name_no_ext: String,
    #[serde(default, alias = "fs_extension")]
    pub fs_extension: String,
    #[serde(default, alias = "fs_size_bytes")]
    pub fs_size_bytes: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// The IGDB game RomM matched the ROM to.
    #[serde(default, alias = "igdb_id")]
    pub igdb_id: Option<u32>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default, alias = "path_cover_large")]
    pub path_cover_large: Option<String>,
    #[serde(default, alias = "path_cover_small")]
    pub path_cover_small: Option<String>,
    #[serde(default, alias = "url_cover")]
    pub url_cover: Option<String>,
    /// The game's other titles.
    #[serde(default, alias = "alternative_names")]
    pub alternative_names: Vec<String>,
    /// Screenshots: paths on the server (its resources, or the user's own
    /// under `/api/raw/`) or URLs.
    #[serde(default, alias = "merged_screenshots")]
    pub merged_screenshots: Vec<String>,
    /// `multi` on servers before 3.x.
    #[serde(default, alias = "has_multiple_files", alias = "multi")]
    pub has_multiple_files: bool,
    #[serde(default)]
    pub files: Vec<RemoteFile>,
    #[serde(default, alias = "crc_hash")]
    pub crc_hash: Option<String>,
    #[serde(default, alias = "md5_hash")]
    pub md5_hash: Option<String>,
    #[serde(default, alias = "sha1_hash")]
    pub sha1_hash: Option<String>,
    #[serde(default, alias = "updated_at")]
    pub updated_at: String,
    #[serde(default, alias = "missing_from_fs")]
    pub missing_from_fs: bool,
    #[serde(default)]
    pub metadatum: Option<RemoteMetadata>,
}

impl RemoteRom {
    /// The title shown in the library: RomM's matched name, or the file name
    /// without its extension.
    pub fn title(&self) -> String {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if self.fs_name_no_ext.trim().is_empty() {
                    self.fs_name.clone()
                } else {
                    self.fs_name_no_ext.clone()
                }
            })
    }

    /// The cover: a path on the server, or else its IGDB URL.
    pub fn cover(&self) -> Option<&str> {
        let nonempty = |value: &&str| !value.trim().is_empty();
        self.path_cover_large
            .as_deref()
            .or(self.path_cover_small.as_deref())
            .filter(nonempty)
            .or(self.url_cover.as_deref().filter(nonempty))
    }

    /// The first screenshots, as many as the game page shows.
    pub fn screenshots(&self) -> impl Iterator<Item = &str> {
        self.merged_screenshots
            .iter()
            .map(String::as_str)
            .filter(|location| !location.trim().is_empty())
            .take(SCREENSHOTS)
    }

    /// The alternative names as the library's aliases: trimmed, without
    /// the title or duplicates, within the model's limits.
    pub fn aliases(&self) -> Vec<String> {
        let title = self.title();
        let mut aliases: Vec<String> = Vec::new();
        for name in &self.alternative_names {
            let name = name.trim();
            if !name.is_empty()
                && name != title
                && name.len() <= MAX_TEXT
                && !name.contains('\0')
                && !aliases.iter().any(|alias| alias == name)
            {
                aliases.push(name.to_owned());
            }
        }
        aliases.truncate(MAX_ALIASES);
        aliases
    }

    /// The release date as `YYYY-MM-DD`.
    pub fn release_date(&self) -> Option<String> {
        let raw = self.metadatum.as_ref()?.first_release_date?;
        // Milliseconds since 1973 are larger than any plausible seconds value.
        let seconds = if raw.abs() > 100_000_000_000 {
            raw / 1000
        } else {
            raw
        };
        Some(civil_date(seconds.div_euclid(86_400)))
    }

    /// Whether the download is a zip of several files.
    pub fn is_multi(&self) -> bool {
        self.has_multiple_files || self.files.len() > 1
    }
}

/// Days since 1970-01-01 as a proleptic Gregorian `YYYY-MM-DD`
/// (Howard Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> String {
    let (year, month, day) = crate::civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_needs_its_items() {
        assert!(serde_json::from_value::<RomPage>(serde_json::json!({ "total": 3 })).is_err());
        let page: RomPage = serde_json::from_value(serde_json::json!({ "items": [] })).unwrap();
        assert!(page.items.is_empty() && page.total.is_none());
    }

    #[test]
    fn release_dates_accept_seconds_and_milliseconds() {
        assert_eq!(civil_date(0), "1970-01-01");
        let mut rom: RemoteRom = serde_json::from_value(serde_json::json!({
            "id": 1, "platform_id": 2, "fs_name": "a.sfc",
            "metadatum": { "first_release_date": 793_929_600_000_i64 }
        }))
        .unwrap();
        assert_eq!(rom.release_date().as_deref(), Some("1995-02-28"));
        rom.metadatum = Some(RemoteMetadata {
            genres: vec![],
            first_release_date: Some(793_929_600),
        });
        assert_eq!(rom.release_date().as_deref(), Some("1995-02-28"));
        assert_eq!(rom.title(), "a.sfc");
    }

    #[test]
    fn covers_screenshots_and_aliases_come_cleaned_up() {
        let rom: RemoteRom = serde_json::from_value(serde_json::json!({
            "id": 1, "platform_id": 2, "fs_name": "a.sfc", "name": "Alpha",
            "path_cover_large": " ", "url_cover": "https://images.igdb.com/a.jpg",
            "alternative_names": ["Alpha", " Beta ", "Beta", ""],
            "merged_screenshots": ["a", "", "b", "c", "d", "e"]
        }))
        .unwrap();
        // An empty path falls back to the URL.
        assert_eq!(rom.cover(), Some("https://images.igdb.com/a.jpg"));
        assert_eq!(rom.aliases(), ["Beta"]);
        assert_eq!(rom.screenshots().collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    }
}
