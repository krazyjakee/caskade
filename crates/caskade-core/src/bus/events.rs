//! Events: observe-only notifications (TECH-PLAN §4.3).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

/// One play of a game: what `game.started` and `game.exited` carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameSession {
    pub game_id: String,
    pub entry_id: String,
    pub session_id: String,
    /// Unix milliseconds.
    pub started_at_ms: i64,
    /// Unix milliseconds; `None` while running.
    pub ended_at_ms: Option<i64>,
    pub duration_ms: u64,
}

/// Per-source counts of a finished library scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceCount {
    pub source: String,
    pub count: u32,
}

/// Every core event. The dotted name (`Event::name`) is the stable
/// identifier plugins subscribe to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
#[non_exhaustive]
pub enum Event {
    AppReady,
    LibraryScanCompleted {
        sources: Vec<SourceCount>,
    },
    /// Cached rankings changed; library pages should reload.
    LibraryPopularityUpdated,
    LibraryGameAdded {
        game_id: String,
    },
    LibraryGameUpdated {
        game_id: String,
    },
    LibraryGameRemoved {
        game_id: String,
    },
    GameLaunching {
        game_id: String,
        entry_id: String,
    },
    GameStarted(GameSession),
    GameExited(GameSession),
    SettingsChanged {
        namespace: String,
        version: u32,
    },
    /// The full snapshot after every change of an install, uninstall or move.
    InstallJobChanged(Box<crate::install::JobSnapshot>),
    /// An install was queued (#55).
    InstallQueued {
        job_id: String,
        title: String,
    },
    /// Install, uninstall or move progress. For the UI: plugin backends
    /// can't subscribe to it.
    InstallProgress {
        job_id: String,
        phase: crate::install::JobPhase,
        progress: crate::install::JobProgress,
    },
    /// A URL input finished downloading.
    InstallDownloaded {
        job_id: String,
    },
    InstallCompleted {
        job_id: String,
        game_id: Option<String>,
    },
    InstallFailed {
        job_id: String,
        error: String,
    },
    UninstallCompleted {
        job_id: String,
        game_id: Option<String>,
    },
    /// A game moved to `dir`.
    GameMoved {
        job_id: String,
        game_id: Option<String>,
        dir: Option<std::path::PathBuf>,
    },
    /// Emulators, platform settings or ROM scanners changed; `scope` is
    /// `emulators`, `platforms` or `scanners`.
    EmulationChanged {
        scope: String,
    },
    /// Every change of an emulator detection.
    EmulationDetectChanged(Box<crate::emulation::DetectJob>),
    /// A managed tool was installed, removed, checked or skipped
    /// (`crate::managed`).
    ToolsChanged,
    /// The streaming hosts this device can stream from changed
    /// (`crate::streaming::hosts`).
    StreamHostsChanged,
    /// A stream from another device is about to start (#264): its client's
    /// windows may show before `game.started`, or `game.exited` if it
    /// doesn't start.
    StreamStarting {
        game_id: String,
        session_id: String,
    },
    /// An event a backend plugin emitted (`host.emit`); `payload` is JSON.
    PluginEvent {
        plugin: String,
        name: String,
        payload: String,
    },
    /// A dev plugin's folder changed and it was reloaded (#131): `ui` when
    /// its UI half should be imported again; `error` when the rebuild is
    /// broken (the previous UI stays).
    PluginReloaded {
        plugin: String,
        ui: bool,
        error: Option<String>,
    },
    /// A gamepad connected or disconnected (#146); `connected` pads remain.
    InputDevicesChanged {
        connected: u32,
    },
    /// A plugin was enabled, disabled, installed or removed.
    PluginsChanged {
        plugin: String,
    },
    /// A backend plugin that kept crashing was disabled (#125); `reason`
    /// is shown to the user.
    PluginDisabled {
        plugin: String,
        reason: String,
    },
    /// Every change of a ROM scan.
    EmulationScanChanged(Box<crate::emulation::ScanJob>),
    /// A ROM scan imported into the library, with counts per scanner.
    EmulationScanCompleted {
        scanners: Vec<crate::emulation::ScannerCount>,
    },
    /// Every change of an emulator install or update (EMULATOR-SETUP-PLAN §3.4).
    EmulationSetupChanged(Box<crate::emulation::SetupJob>),
    /// An account was made, changed or deleted, an account signed in or
    /// out, or wrong PINs locked one (#202). What the active account owns
    /// (its collections) may have changed with it.
    AccountChanged {
        active_id: Option<String>,
    },
    /// A launch's lifecycle scripts of one phase ran (#179), with their
    /// output. Not in the plugin event list: scripts are the user's.
    LifecycleScriptsRan(Box<crate::scripts::ScriptReport>),
    /// A game's achievements were fetched (#185): show them again.
    AchievementsChanged {
        game_id: String,
    },
    /// Achievements unlocked since a set's last complete fetch (#185):
    /// never the first fetch's history.
    AchievementsUnlocked(Box<crate::achievements::NewUnlocks>),
}

impl Event {
    /// The stable dotted name, e.g. `game.started`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AppReady => "app.ready",
            Self::LibraryPopularityUpdated => "library.popularity.updated",
            Self::LibraryScanCompleted { .. } => "library.scan.completed",
            Self::LibraryGameAdded { .. } => "library.game.added",
            Self::LibraryGameUpdated { .. } => "library.game.updated",
            Self::LibraryGameRemoved { .. } => "library.game.removed",
            Self::GameLaunching { .. } => "game.launching",
            Self::GameStarted(_) => "game.started",
            Self::GameExited(_) => "game.exited",
            Self::SettingsChanged { .. } => "settings.changed",
            Self::InstallJobChanged { .. } => "install.job.changed",
            Self::InstallQueued { .. } => "install.queued",
            Self::InstallProgress { .. } => "install.progress",
            Self::InstallDownloaded { .. } => "install.downloaded",
            Self::InstallCompleted { .. } => "install.completed",
            Self::InstallFailed { .. } => "install.failed",
            Self::UninstallCompleted { .. } => "uninstall.completed",
            Self::GameMoved { .. } => "game.moved",
            Self::InputDevicesChanged { .. } => "input.devices.changed",
            Self::EmulationChanged { .. } => "emulation.changed",
            Self::EmulationDetectChanged(_) => "emulation.detect.changed",
            Self::ToolsChanged => "tools.changed",
            Self::StreamHostsChanged => "streaming.hosts.changed",
            Self::StreamStarting { .. } => "stream.starting",
            Self::PluginEvent { .. } => "plugin.event",
            Self::PluginDisabled { .. } => "plugin.disabled",
            Self::PluginsChanged { .. } => "plugins.changed",
            Self::PluginReloaded { .. } => "plugin.reloaded",
            Self::EmulationScanChanged(_) => "emulation.scan.changed",
            Self::EmulationScanCompleted { .. } => "emulation.scan.completed",
            Self::EmulationSetupChanged(_) => "emulation.setup.changed",
            Self::AccountChanged { .. } => "account.changed",
            Self::LifecycleScriptsRan(_) => "lifecycle.scripts.ran",
            Self::AchievementsChanged { .. } => "achievements.changed",
            Self::AchievementsUnlocked(_) => "achievements.unlocked",
        }
    }
}

/// Which events a subscriber receives: every event, or exact names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventFilter {
    All,
    Names(Vec<&'static str>),
}

impl EventFilter {
    pub fn matches(&self, event: &Event) -> bool {
        match self {
            Self::All => true,
            Self::Names(names) => names.contains(&event.name()),
        }
    }
}
