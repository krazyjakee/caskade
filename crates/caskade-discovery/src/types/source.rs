use super::{DetectedGame, LauncherStatus, SourceError, SourceId};
use crate::{OperatingSystem, env::HostEnv};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// One place games come from: a launcher's records or an OS facility.
///
/// Sources read files and the registry through `HostEnv` only, never start
/// subprocesses, and treat everything they read as hostile.
pub trait Source: Send + Sync {
    fn id(&self) -> SourceId;
    /// Where the source runs natively. Windows sources also run against
    /// Wine-family prefixes on other hosts.
    fn hosts(&self) -> OsSet;
    /// Every install of the launcher (native, Flatpak, Snap, prefixes). A
    /// source without a launcher returns nothing.
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus>;
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError>;
    /// Files and directories whose changes may change `scan`'s result.
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath>;

    /// Where the launcher would live if installed, whether or not it is.
    /// Watching their nearest existing parents picks up a launcher that is
    /// installed after the watch started.
    fn launcher_locations(&self, _env: &HostEnv) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// A set of operating systems.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Type,
)]
pub struct OsSet {
    pub windows: bool,
    pub linux: bool,
    pub macos: bool,
}

impl OsSet {
    pub const NONE: Self = Self {
        windows: false,
        linux: false,
        macos: false,
    };
    pub const ALL: Self = Self {
        windows: true,
        linux: true,
        macos: true,
    };
    pub const WINDOWS: Self = Self::NONE.with(OperatingSystem::Windows);
    pub const LINUX: Self = Self::NONE.with(OperatingSystem::Linux);
    pub const MACOS: Self = Self::NONE.with(OperatingSystem::Macos);

    pub const fn with(mut self, os: OperatingSystem) -> Self {
        match os {
            OperatingSystem::Windows => self.windows = true,
            OperatingSystem::Linux => self.linux = true,
            OperatingSystem::Macos => self.macos = true,
        }
        self
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            windows: self.windows || other.windows,
            linux: self.linux || other.linux,
            macos: self.macos || other.macos,
        }
    }

    pub const fn contains(self, os: OperatingSystem) -> bool {
        match os {
            OperatingSystem::Windows => self.windows,
            OperatingSystem::Linux => self.linux,
            OperatingSystem::Macos => self.macos,
        }
    }
}

/// A path to watch for changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Type)]
pub struct WatchPath {
    pub path: PathBuf,
    pub recursive: bool,
}

/// A cancellation flag shared between the caller and every source.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Per-source scan state. The orchestrator cancels it when the caller
/// cancels or the source's time budget runs out; sources check it between
/// records and collect non-fatal warnings in it.
#[derive(Debug, Default)]
pub struct ScanCtx {
    cancel: Cancel,
    warnings: Mutex<Vec<String>>,
    config: Arc<crate::orchestrator::Config>,
}

impl ScanCtx {
    /// A context with the default `Config`.
    pub fn new(cancel: Cancel) -> Self {
        Self::with_config(cancel, Arc::default())
    }

    pub fn with_config(cancel: Cancel, config: Arc<crate::orchestrator::Config>) -> Self {
        Self {
            cancel,
            warnings: Mutex::default(),
            config,
        }
    }

    /// The scan's configuration: deep scans, exclusions, classifier.
    pub fn config(&self) -> &crate::orchestrator::Config {
        &self.config
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// `Err(SourceError::Cancelled)` once cancelled, for `?` in scan loops.
    pub fn check(&self) -> Result<(), SourceError> {
        if self.is_cancelled() {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn warn(&self, warning: impl Into<String>) {
        let warning = warning.into();
        tracing::debug!(%warning, "source warning");
        self.warnings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(warning);
    }

    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(
            &mut self
                .warnings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

/// Every built-in source, in merge priority order (launchers first). Each
/// source issue adds its entry here.
pub fn builtin_sources() -> &'static [&'static dyn Source] {
    &[
        &crate::sources::steam::Steam,
        &crate::sources::heroic::Heroic,
        &crate::sources::legendary::Legendary,
        &crate::sources::itch::Itch,
        &crate::sources::epic::EpicGamesLauncher,
        &crate::sources::ea::Ea,
        &crate::sources::ubisoft::UbisoftConnect,
        &crate::sources::gog::GogGalaxy,
        &crate::sources::battlenet::BattleNet,
        #[cfg(feature = "sqlite")]
        &crate::sources::amazon::AmazonGames,
        &crate::sources::riot::RiotClient,
        &crate::sources::rockstar::RockstarLauncher,
        #[cfg(feature = "windows-sources")]
        &crate::sources::xbox::Xbox,
        #[cfg(feature = "linux-sources")]
        &crate::sources::lutris::Lutris,
        #[cfg(feature = "linux-sources")]
        &crate::sources::bottles::Bottles,
        #[cfg(feature = "linux-sources")]
        &crate::sources::minigalaxy::Minigalaxy,
        #[cfg(feature = "linux-sources")]
        &crate::sources::faugus::Faugus,
        #[cfg(feature = "macos-sources")]
        &crate::sources::macos_apps::MacosApps,
        &crate::sources::steam::shortcuts::SteamShortcuts,
        &crate::sources::prefixes::Prefixes,
        #[cfg(feature = "linux-sources")]
        &crate::sources::flatpak::Flatpak,
        #[cfg(feature = "linux-sources")]
        &crate::sources::snap::Snap,
        #[cfg(feature = "linux-sources")]
        &crate::sources::desktop::Desktop,
        #[cfg(feature = "windows-sources")]
        &crate::sources::gameux::GameUx,
        #[cfg(feature = "windows-sources")]
        &crate::sources::uninstall::UninstallSource,
        #[cfg(feature = "windows-sources")]
        &crate::sources::startmenu::StartMenuSource,
        #[cfg(feature = "deep")]
        &crate::deep::Deep,
    ]
}
