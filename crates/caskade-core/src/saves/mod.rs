//! Save locations for one installed release, resolved entirely on this device.
//! The manifest supplies patterns; discovery supplies the actual install and
//! Wine/Proton prefix. Unknown games produce no automatic locations.
mod context;
mod files;
mod registry;
mod service;
mod store;

pub use context::{Context, expand};
pub use files::{expand_glob, scan_path};
pub use service::Saves;

use caskade_discovery::registry::{RegistryEntry, RegistryView};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LocationKind {
    File,
    Folder,
    Registry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocationSpec {
    pub kind: LocationKind,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavePreferences {
    /// None follows automatic Steam → GOG → title/alias matching.
    pub manifest_title: Option<String>,
    /// None follows automatic locations; an empty list explicitly removes all.
    pub locations: Option<Vec<LocationSpec>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MatchMethod {
    Manual,
    Steam,
    Gog,
    Title,
    Alias,
    Secondary,
    Emulator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveMatch {
    Unknown,
    Matched { title: String, method: MatchMethod },
    Ambiguous { titles: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySnapshot {
    pub key: String,
    pub view: RegistryView,
    pub values: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocation {
    pub spec: LocationSpec,
    pub tags: Vec<String>,
    /// Folder contents are enumerated recursively, without following cycles.
    pub files: Vec<String>,
    /// Only the selected keys, never a copy of an entire Wine registry file.
    pub registry: Vec<RegistrySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveState {
    pub entry_id: String,
    pub revision: u32,
    pub preferences: SavePreferences,
    pub matched: SaveMatch,
    pub automatic: Vec<SaveLocation>,
    pub locations: Vec<SaveLocation>,
    pub warnings: Vec<String>,
}
