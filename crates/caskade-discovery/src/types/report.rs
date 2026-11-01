use super::{DetectedGame, SourceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use thiserror::Error;

/// The result of one scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ScanReport {
    /// Merged results, one group per game.
    pub groups: Vec<GameGroup>,
    pub launchers: Vec<LauncherStatus>,
    pub sources: Vec<SourceReport>,
}

/// What changed between two scans of the same machine (§9). A group that
/// keeps at least one of its entries is the same group: `updated` when its
/// contents changed, otherwise not listed. `removed` holds the old groups.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ScanDelta {
    pub added: Vec<GameGroup>,
    pub updated: Vec<GameGroup>,
    pub removed: Vec<GameGroup>,
    /// Reports of the sources that were scanned for this delta.
    pub sources: Vec<SourceReport>,
    /// Current launcher statuses of those sources.
    pub launchers: Vec<LauncherStatus>,
}

impl ScanDelta {
    /// No group was added, updated or removed.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

/// One game reported by one or more sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct GameGroup {
    /// Index into `entries`, chosen by source priority.
    pub primary: u32,
    /// One entry per source that reported the game.
    pub entries: Vec<DetectedGame>,
}

impl GameGroup {
    pub fn primary_entry(&self) -> Option<&DetectedGame> {
        self.entries.get(self.primary as usize)
    }
}

/// One install of a launcher, or a launcher that was looked for and not found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct LauncherStatus {
    pub source: SourceId,
    pub found: bool,
    /// `None` when not found.
    pub kind: Option<LauncherKind>,
    /// The executable, bundle or install directory.
    pub path: Option<PathBuf>,
    /// The launcher's data or config directory that sources read.
    pub data_dir: Option<PathBuf>,
    pub version: Option<String>,
}

impl LauncherStatus {
    pub fn not_found(source: SourceId) -> Self {
        Self {
            source,
            found: false,
            kind: None,
            path: None,
            data_dir: None,
            version: None,
        }
    }

    /// An install of the launcher, with no version.
    pub fn found(
        source: SourceId,
        kind: LauncherKind,
        path: Option<PathBuf>,
        data_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            source,
            found: true,
            kind: Some(kind),
            path,
            data_dir,
            version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum LauncherKind {
    Native,
    Flatpak {
        app_id: String,
    },
    Snap {
        name: String,
    },
    /// A Windows launcher inside a Wine-family prefix.
    Wine {
        prefix: PathBuf,
    },
}

impl LauncherKind {
    /// The Flatpak `app_id` when `flatpak`, else a native install.
    pub fn native_or_flatpak(flatpak: bool, app_id: &str) -> Self {
        if flatpak {
            Self::Flatpak {
                app_id: app_id.into(),
            }
        } else {
            Self::Native
        }
    }
}

/// How one source did during a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct SourceReport {
    pub source: SourceId,
    pub duration_ms: u64,
    /// Games the source returned.
    pub count: u32,
    /// Unknown fields, newer format versions, skipped records, …
    pub warnings: Vec<String>,
    pub error: Option<SourceError>,
}

/// Why a source returned no games. It never fails the scan.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SourceError {
    #[error("{}: {message}", path.display())]
    Io { path: PathBuf, message: String },
    #[error("{}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("registry {key}: {message}")]
    Registry { key: String, message: String },
    #[error("cancelled")]
    Cancelled,
    #[error("timed out")]
    TimedOut,
    #[error("panicked: {message}")]
    Panicked { message: String },
    #[error("{message}")]
    Other { message: String },
}

impl SourceError {
    pub fn io(path: impl Into<PathBuf>, error: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.into(),
            message: error.to_string(),
        }
    }

    pub fn parse(path: impl Into<PathBuf>, error: impl std::fmt::Display) -> Self {
        Self::Parse {
            path: path.into(),
            message: error.to_string(),
        }
    }
}
