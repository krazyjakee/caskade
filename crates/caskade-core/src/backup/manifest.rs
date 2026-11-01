//! The archive's table of contents: `manifest.json`, written last.

use serde::{Deserialize, Serialize};
use specta::Type;

/// The archive format this Caskade writes. A reader refuses a newer one.
pub const FORMAT: u32 = 1;
/// The manifest's name in the archive.
pub const MANIFEST: &str = "manifest.json";
/// The library database snapshot's name in the archive.
pub const DATABASE: &str = "library/library.sqlite";

/// What a backup holds and how to check it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupManifest")]
pub struct Manifest {
    /// [`FORMAT`] when written.
    pub format: u32,
    /// The Caskade that wrote it.
    pub app_version: String,
    /// The library database's newest migration.
    pub schema_version: i64,
    /// Unix milliseconds.
    pub created_at_ms: i64,
    /// The OS it was made on (`linux`, `windows`, `macos`).
    pub os: String,
    /// The automatic backup set it belongs to (#193); `None` for a manual
    /// backup.
    pub set_id: Option<String>,
    pub components: Vec<ComponentSummary>,
    /// Rows per table in the database snapshot.
    pub tables: Vec<TableRows>,
    /// What a backup never holds.
    pub excluded: Vec<Exclusion>,
    /// Every file in the archive but the manifest.
    pub files: Vec<FileEntry>,
}

/// The parts a backup can hold. The library is always in it.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Type,
    schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", rename = "BackupComponent")]
pub enum Component {
    /// The library database: games, entries, play time, collections,
    /// accounts, edits, launch actions, settings and plugins' stored state.
    Library,
    /// Artwork that can't be downloaded again: images the user chose or
    /// imported, web games' pictures, executables' icons.
    Artwork,
    /// Artwork downloaded from IGDB, which a restore can fetch again.
    DownloadedArtwork,
    /// Installed plugins (`<data>/plugins`).
    Plugins,
    /// Plugins' files (`<data>/plugin-data`).
    PluginData,
    /// Installed themes (`<data>/themes`).
    Themes,
}

impl Component {
    pub const ALL: [Self; 6] = [
        Self::Library,
        Self::Artwork,
        Self::DownloadedArtwork,
        Self::Plugins,
        Self::PluginData,
        Self::Themes,
    ];

    /// The archive folder its files go in.
    pub(super) fn folder(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Artwork | Self::DownloadedArtwork => "artwork",
            Self::Plugins => "plugins",
            Self::PluginData => "plugin-data",
            Self::Themes => "themes",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupComponentSummary")]
pub struct ComponentSummary {
    pub id: Component,
    pub files: u32,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupTableRows")]
pub struct TableRows {
    pub name: String,
    pub rows: u64,
}

/// One file: its path in the archive (`/`-separated, relative), its size
/// and its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupFile")]
pub struct FileEntry {
    pub path: String,
    pub component: Component,
    pub size: u64,
    pub sha256: String,
}

/// What a backup leaves out, and so what a restore can't bring back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "BackupExclusion")]
pub enum Exclusion {
    /// Passwords and tokens (RomM's, stores'): they stay in the OS
    /// keychain and are signed in again after a restore.
    Credentials,
    /// Web games' browser data: cookies, sign-ins and their saves.
    BrowserData,
    /// Games' own files and saves, wherever they are installed.
    GameFiles,
    /// Emulators Caskade installed; Get emulators installs them again.
    ManagedEmulators,
    /// ROMs downloaded from RomM to play.
    DownloadedRoms,
    /// Caches Caskade fills again: metadata, thumbnails it can download,
    /// compiled plugins, icon checks.
    Caches,
    /// Installs, moves and downloads under way.
    PendingJobs,
    Logs,
}

impl Exclusion {
    pub const ALL: [Self; 8] = [
        Self::Credentials,
        Self::BrowserData,
        Self::GameFiles,
        Self::ManagedEmulators,
        Self::DownloadedRoms,
        Self::Caches,
        Self::PendingJobs,
        Self::Logs,
    ];
}
