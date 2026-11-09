//! The add-on catalogue (#181): a curated, versioned list of plugins and
//! themes, each with its releases and their downloads, read from one
//! signed origin and kept for offline browsing. See `docs/plugins/catalogue.md`.
//!
//! The catalogue is data. Descriptions are plain text and media are
//! images the app re-encodes before showing; nothing in it runs. It lists
//! plugins and themes only: a plugin that provides metadata (the reserved
//! `metadata-provider` interface) is refused. Fetching it sends nothing
//! about the user or their library: it is one GET of a public file.

pub mod author;
mod compat;
pub mod package;
mod service;
mod validate;

#[cfg(test)]
mod author_tests;
#[cfg(test)]
mod package_tests;
#[cfg(test)]
mod tests;

pub use compat::{
    AppMode, CatalogueItem, Compatibility, CompatibilityReason, HostFacts, InstalledAddon,
    ItemState, ReasonCode, evaluate, evaluate_entry,
};
pub use service::{CatalogueService, CatalogueStatus};
pub use validate::{MAX_CATALOGUE_BYTES, MAX_SIGNATURE_BYTES, parse};

use serde::{Deserialize, Serialize};
use specta::Type;

/// The catalogue format this Caskade reads.
pub const CATALOGUE_FORMAT: u32 = 1;

/// The curated origin: a `catalogue` release on Caskade's public repo, so
/// installed copies read it without an account. A build can point
/// elsewhere with `CASKADE_CATALOGUE_URL`.
pub const DEFAULT_ORIGIN: &str = match option_env!("CASKADE_CATALOGUE_URL") {
    Some(url) => url,
    None => "https://github.com/krazyjakee/caskade/releases/download/catalogue/catalogue.json",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AddonKind {
    Plugin,
    Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AddonOs {
    Linux,
    Windows,
    Macos,
}

impl AddonOs {
    /// The OS this build runs on.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// An image a catalogue entry shows (a screenshot, a theme preview).
/// Fetched and re-encoded by the artwork cache before it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueMedia {
    /// Absolute, resolved against the catalogue's address.
    pub url: String,
    /// What it shows, for screen readers.
    pub alt: String,
}

/// A release's download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueArtifact {
    /// Absolute, resolved against the catalogue's address.
    pub url: String,
    /// Lowercase hex SHA-256 of the archive; an install checks it.
    pub sha256: String,
    /// Its size in bytes; an install refuses anything else.
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueRelease {
    /// A semantic version.
    pub version: String,
    /// The plugin API range it needs (`^0.1`); none for themes.
    pub api: Option<String>,
    /// The OSes it runs on; empty means every one.
    pub os: Vec<AddonOs>,
    /// The app modes its UI supports; empty means both.
    pub modes: Vec<AppMode>,
    /// What it asks for, as its manifest does (`network`, `library.read`,
    /// …), for the review before installing; the manifest decides.
    pub permissions: Vec<String>,
    /// What changed, as plain text.
    pub notes: String,
    /// `YYYY-MM-DD`.
    pub published: Option<String>,
    pub artifact: CatalogueArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueEntry {
    /// The plugin's or theme's manifest id.
    pub id: String,
    pub kind: AddonKind,
    pub name: String,
    pub author: String,
    /// One line.
    pub summary: String,
    /// Plain text; shown as text, never as HTML.
    pub description: String,
    /// An SPDX expression.
    pub license: String,
    pub homepage: Option<String>,
    pub categories: Vec<String>,
    pub screenshots: Vec<CatalogueMedia>,
    /// Newest first.
    pub releases: Vec<CatalogueRelease>,
    /// Why it shouldn't be installed any more; it stays listed for those
    /// who have it.
    pub deprecated: Option<String>,
}

/// A record the catalogue lists that can't be used, and why. The other
/// records stay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueProblem {
    /// The record's id, when it has a readable one.
    pub id: Option<String>,
    pub message: String,
}

/// A validated catalogue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    /// When it was generated, as its publisher says.
    pub generated: Option<String>,
    pub entries: Vec<CatalogueEntry>,
    pub problems: Vec<CatalogueProblem>,
}

impl Catalogue {
    pub fn entry(&self, id: &str) -> Option<&CatalogueEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}
