use super::{ExternalId, SourceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::BTreeMap, path::PathBuf};

/// One game as one source reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct DetectedGame {
    pub source: SourceId,
    /// Stable within the source: app id, registry key, file path, …
    pub key: String,
    pub title: String,
    pub ids: Vec<ExternalId>,
    pub install_dir: Option<PathBuf>,
    /// Ways to launch, best first.
    pub launch: Vec<LaunchTarget>,
    pub runtime: GameRuntime,
    pub state: InstallState,
    /// Bytes on disk.
    pub size: Option<u64>,
    /// Total playtime in seconds.
    pub playtime_secs: Option<u64>,
    /// Last played, in seconds since the Unix epoch.
    pub last_played_unix: Option<u64>,
    /// A local icon file.
    pub icon: Option<PathBuf>,
    /// Local cover, header, hero and logo images.
    pub artwork: Vec<Artwork>,
    pub uninstall: Option<UninstallInfo>,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub tracking: Option<TrackingHint>,
}

impl DetectedGame {
    /// A launcher-certain, installed, native entry with every optional field
    /// empty; sources fill in what they know.
    pub fn new(source: SourceId, key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            source,
            key: key.into(),
            title: title.into(),
            ids: Vec::new(),
            install_dir: None,
            launch: Vec::new(),
            runtime: GameRuntime::Native,
            state: InstallState::Installed,
            size: None,
            playtime_secs: None,
            last_played_unix: None,
            icon: None,
            artwork: Vec::new(),
            uninstall: None,
            confidence: Confidence::Certain,
            evidence: Vec::new(),
            tracking: None,
        }
    }
}

/// A local image of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Artwork {
    pub kind: ArtworkKind,
    pub path: PathBuf,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkKind {
    /// Portrait box art.
    Cover,
    /// Landscape banner.
    Header,
    /// Wide background.
    Hero,
    Logo,
}

/// How to start a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum LaunchTarget {
    /// A launcher URI such as `steam://rungameid/620`.
    Uri {
        uri: String,
    },
    /// A program with an explicit argv; never run through a shell.
    Exec {
        program: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: BTreeMap<String, String>,
    },
    Flatpak {
        app_id: String,
        args: Vec<String>,
    },
}

/// What the game runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GameRuntime {
    Native,
    /// A Windows game, on Windows itself or in a Wine-family prefix.
    Windows {
        prefix: Option<Prefix>,
        runner: Option<RunnerHint>,
    },
}

/// A Wine-family prefix and the launcher that owns it, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Prefix {
    pub path: PathBuf,
    pub arch: Option<PrefixArch>,
    /// The source that manages the prefix (`"lutris"`, `"bottles"`, …).
    pub owner: Option<SourceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum PrefixArch {
    Win32,
    Win64,
}

/// The compatibility tool a source recorded for a Windows game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RunnerHint {
    pub kind: RunnerKind,
    /// As the source names it, e.g. `GE-Proton10-34` or `wine-ge-8-26`.
    pub name: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Wine,
    Proton,
    Umu,
    CrossOver,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum InstallState {
    Installed,
    Updating,
    /// Downloaded or installed in part.
    Partial,
    Broken {
        reason: String,
    },
}

/// How sure the source is that this is an installed game.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// The classifier's review list.
    Maybe,
    Likely,
    /// A launcher's own record.
    Certain,
}

/// Why a result was reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Evidence {
    /// A file the source read.
    File { path: PathBuf },
    /// A registry key, and the value within it when one decided.
    Registry { key: String, value: Option<String> },
    /// A classifier rule and its score contribution.
    Rule { rule: String, score: i32 },
}

/// How to uninstall the game through its owner; discovery never runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct UninstallInfo {
    pub target: LaunchTarget,
    /// An unattended variant (`QuietUninstallString`).
    pub quiet: Option<LaunchTarget>,
    pub registry_key: Option<String>,
}

/// Hints for M2 process tracking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct TrackingHint {
    /// Steam's reaper and `SteamAppId` carry it.
    pub steam_app_id: Option<u32>,
    pub install_dir: Option<PathBuf>,
    /// Executable file names a launcher spawns for this game.
    pub exe_names: Vec<String>,
}
