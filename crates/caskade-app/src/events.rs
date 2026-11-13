//! What the app tells its UI (#86): typed [`Event`]s, sent through the
//! shell's [`Emit`] sink. The Tauri shell forwards each one to the webview
//! under its event name; a native shell reads them directly. [`bridge`]
//! turns the core bus's events into these.

// The commands' own events.
pub use crate::commands::addon_packages::AddonProgress;
pub use crate::commands::app_update::AppUpdateChanged;
pub use crate::commands::backup::BackupChanged;
pub use crate::commands::game_edit::GamesRefreshChanged;
pub use crate::commands::i18n::LocaleChanged;
pub use crate::commands::input::InputActionEvent;
pub use crate::commands::install_requests::InstallRefused;
pub use crate::commands::install_requests::InstallRequested;
pub use crate::commands::notifications::NotificationsChanged;
pub use crate::commands::presence::PlayingElsewhereChanged;
pub use crate::commands::remote_play::RemotePlayRequested;
pub use crate::commands::romm::RommDownloadChanged;
pub use crate::commands::saves::SaveVersionsChanged;
pub use crate::commands::shares::SharesChanged;
pub use crate::commands::store_accounts::StoreAccountsChanged;
pub use crate::commands::streaming::StreamingPairingFinished;
pub use crate::commands::sync::DevicesChanged;
pub use crate::commands::sync::PairingLinkOpened;
pub use crate::commands::system_stats::SystemStatsSampled;
pub use crate::commands::themes::ThemeReloaded;
pub use crate::discovery::DiscoveryChanged;
use caskade_core::bus::{Event as BusEvent, GameSession};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// `game.started`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GameStarted(pub GameSession);

/// `game.exited`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GameExited(pub GameSession);

/// How the library changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LibraryChange {
    Added,
    Updated,
    Removed,
    /// A scan finished; any game may have changed.
    Scanned,
}

/// `library.game.added/updated/removed` and `library.scan.completed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub change: LibraryChange,
    /// `None` after a scan.
    pub game_id: Option<String>,
}

/// A complete revisioned install/download/move snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct JobChanged(pub caskade_core::install::JobSnapshot);

/// `settings.changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    pub namespace: String,
    pub version: u32,
}

/// `emulation.changed`: refetch emulators, platforms or scanners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmulationChanged {
    /// `emulators`, `platforms` or `scanners`.
    pub scope: String,
}

/// Every change of an emulator detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EmulationDetectChanged(pub caskade_core::emulation::DetectJob);

/// Every change of a ROM scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EmulationScanChanged(pub caskade_core::emulation::ScanJob);

/// Every change of an emulator install or update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EmulationSetupChanged(pub caskade_core::emulation::SetupJob);

/// `plugin.disabled`: a backend plugin kept crashing and was turned off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginDisabled {
    pub plugin: String,
    pub reason: String,
}

/// `plugins.changed`: a plugin was enabled, disabled, installed or removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsChanged {
    pub plugin: String,
}

/// `plugin.reloaded`: a dev plugin changed on disk (#131).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginReloaded {
    pub plugin: String,
    /// Import the UI half again.
    pub ui: bool,
    /// The broken rebuild's error; the previous UI stays.
    pub error: Option<String>,
}

/// `account.changed`: an account was made, changed or deleted, or the
/// active one changed. Refetch the accounts and what the active account
/// owns (its collections).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountChanged {
    /// The signed-in account; `None` when nobody is.
    pub active_id: Option<String>,
}

/// `achievements.changed`: a game's achievements were fetched (#185).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsChanged {
    pub game_id: String,
}

// Activity's background tasks: a whole revisioned snapshot. (No doc
// comment: the bindings carry none for it.)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BackgroundChanged(pub caskade_core::jobs::Snapshot);

/// Everything the app tells its UI.
// Sent one at a time, never stored in bulk: boxing the job snapshots would
// buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Event {
    GameStarted(GameStarted),
    GameExited(GameExited),
    LibraryChanged(LibraryChanged),
    JobChanged(JobChanged),
    SettingsChanged(SettingsChanged),
    EmulationChanged(EmulationChanged),
    EmulationDetectChanged(EmulationDetectChanged),
    EmulationScanChanged(EmulationScanChanged),
    EmulationSetupChanged(EmulationSetupChanged),
    PluginDisabled(PluginDisabled),
    PluginsChanged(PluginsChanged),
    PluginReloaded(PluginReloaded),
    AccountChanged(AccountChanged),
    AchievementsChanged(AchievementsChanged),
    BackgroundChanged(BackgroundChanged),
    RommDownloadChanged(RommDownloadChanged),
    DiscoveryChanged(DiscoveryChanged),
    NotificationsChanged(NotificationsChanged),
    BackupChanged(BackupChanged),
    ThemeReloaded(ThemeReloaded),
    PlayingElsewhereChanged(PlayingElsewhereChanged),
    RemotePlayRequested(RemotePlayRequested),
    StoreAccountsChanged(StoreAccountsChanged),
    AddonProgress(AddonProgress),
    SharesChanged(SharesChanged),
    DevicesChanged(DevicesChanged),
    PairingLinkOpened(PairingLinkOpened),
    InstallRequested(InstallRequested),
    InstallRefused(InstallRefused),
    StreamingPairingFinished(StreamingPairingFinished),
    LocaleChanged(LocaleChanged),
    SystemStatsSampled(SystemStatsSampled),
    AppUpdateChanged(AppUpdateChanged),
    InputActionEvent(InputActionEvent),
    GamesRefreshChanged(GamesRefreshChanged),
    SaveVersionsChanged(SaveVersionsChanged),
}

/// Where a shell receives [`Event`]s: called on the async runtime, in the
/// order they happen. A failure is the shell's to explain.
pub trait Emit: Send + Sync + 'static {
    fn emit(&self, event: Event) -> Result<(), String>;
}

/// The shell's sink, shared by everything that tells the UI something.
#[derive(Clone)]
pub struct Events(Arc<dyn Emit>);

impl Events {
    pub fn new(sink: impl Emit) -> Self {
        Self(Arc::new(sink))
    }

    /// Sends `event`; the shell's reason when it couldn't, for a caller
    /// that acts on it (a request nobody saw is answered as failed).
    pub fn emit(&self, event: Event) -> Result<(), String> {
        self.0.emit(event)
    }

    /// Sends `event`; a failure is logged, as nothing can answer it.
    pub fn send(&self, event: Event) {
        let name = event.name();
        if let Err(error) = self.0.emit(event) {
            tracing::warn!(event = name, %error, "could not tell the UI");
        }
    }
}

impl std::fmt::Debug for Events {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Events")
    }
}

impl Event {
    /// For logs: the bus event's name it carries, or what it is.
    pub fn name(&self) -> &'static str {
        match self {
            Self::GameStarted(_) => "game.started",
            Self::GameExited(_) => "game.exited",
            Self::LibraryChanged(_) => "library.changed",
            Self::JobChanged(_) => "install.job.changed",
            Self::SettingsChanged(_) => "settings.changed",
            Self::EmulationChanged(_) => "emulation.changed",
            Self::EmulationDetectChanged(_) => "emulation.detect.changed",
            Self::EmulationScanChanged(_) => "emulation.scan.changed",
            Self::EmulationSetupChanged(_) => "emulation.setup.changed",
            Self::PluginDisabled(_) => "plugin.disabled",
            Self::PluginsChanged(_) => "plugins.changed",
            Self::PluginReloaded(_) => "plugin.reloaded",
            Self::AccountChanged(_) => "account.changed",
            Self::AchievementsChanged(_) => "achievements.changed",
            Self::BackgroundChanged(_) => "background.changed",
            Self::RommDownloadChanged(_) => "romm.download.changed",
            Self::DiscoveryChanged(_) => "discovery.changed",
            Self::NotificationsChanged(_) => "notifications.changed",
            Self::BackupChanged(_) => "backup.changed",
            Self::ThemeReloaded(_) => "theme.reloaded",
            Self::PlayingElsewhereChanged(_) => "playing.elsewhere.changed",
            Self::RemotePlayRequested(_) => "remote.play.requested",
            Self::StoreAccountsChanged(_) => "store_accounts.changed",
            Self::AddonProgress(_) => "addon.progress",
            Self::SharesChanged(_) => "shares.changed",
            Self::DevicesChanged(_) => "devices.changed",
            Self::PairingLinkOpened(_) => "pairing.link.opened",
            Self::InstallRequested(_) => "install.requested",
            Self::InstallRefused(_) => "install.refused",
            Self::StreamingPairingFinished(_) => "streaming.pairing.finished",
            Self::LocaleChanged(_) => "locale.changed",
            Self::SystemStatsSampled(_) => "system.stats.sampled",
            Self::AppUpdateChanged(_) => "app_update.changed",
            Self::InputActionEvent(_) => "input.action",
            Self::GamesRefreshChanged(_) => "games.refresh.changed",
            Self::SaveVersionsChanged(_) => "save.versions.changed",
        }
    }
}

/// The UI event for a library bus event.
fn library_change(event: &BusEvent) -> Option<LibraryChanged> {
    let changed = |change, game_id: &String| LibraryChanged {
        change,
        game_id: Some(game_id.clone()),
    };
    Some(match event {
        BusEvent::LibraryGameAdded { game_id } => changed(LibraryChange::Added, game_id),
        BusEvent::LibraryGameUpdated { game_id } => changed(LibraryChange::Updated, game_id),
        BusEvent::LibraryGameRemoved { game_id } => changed(LibraryChange::Removed, game_id),
        BusEvent::LibraryScanCompleted { .. } | BusEvent::LibraryPopularityUpdated => {
            LibraryChanged {
                change: LibraryChange::Scanned,
                game_id: None,
            }
        }
        _ => return None,
    })
}

/// The UI event for one bus event; `None` for events the UI does not use.
/// Library events are [`bridge`]'s to coalesce.
pub fn translate(event: BusEvent) -> Option<Event> {
    if let Some(changed) = library_change(&event) {
        return Some(Event::LibraryChanged(changed));
    }
    Some(match event {
        BusEvent::GameStarted(session) => Event::GameStarted(GameStarted(session)),
        BusEvent::GameExited(session) => Event::GameExited(GameExited(session)),
        BusEvent::InstallJobChanged(snapshot) => Event::JobChanged(JobChanged(*snapshot)),
        BusEvent::SettingsChanged { namespace, version } => {
            Event::SettingsChanged(SettingsChanged { namespace, version })
        }
        BusEvent::EmulationChanged { scope } => Event::EmulationChanged(EmulationChanged { scope }),
        BusEvent::EmulationDetectChanged(job) => {
            Event::EmulationDetectChanged(EmulationDetectChanged(*job))
        }
        BusEvent::EmulationScanChanged(job) => {
            Event::EmulationScanChanged(EmulationScanChanged(*job))
        }
        BusEvent::EmulationSetupChanged(job) => {
            Event::EmulationSetupChanged(EmulationSetupChanged(*job))
        }
        BusEvent::PluginDisabled { plugin, reason } => {
            Event::PluginDisabled(PluginDisabled { plugin, reason })
        }
        BusEvent::PluginsChanged { plugin } => Event::PluginsChanged(PluginsChanged { plugin }),
        BusEvent::PluginReloaded { plugin, ui, error } => {
            Event::PluginReloaded(PluginReloaded { plugin, ui, error })
        }
        BusEvent::AccountChanged { active_id } => {
            Event::AccountChanged(AccountChanged { active_id })
        }
        BusEvent::AchievementsChanged { game_id } => {
            Event::AchievementsChanged(AchievementsChanged { game_id })
        }
        _ => return None,
    })
}

/// How long library events are merged after one went out. Imports and ROM
/// scans emit one event per game; each library change makes the UI refetch
/// the library and the storage list.
pub const LIBRARY_WINDOW: Duration = Duration::from_millis(200);

/// Rate-limits [`LibraryChanged`]: the first event of a quiet period goes out at
/// once and opens a window. Events inside the window are merged and go out
/// when it closes, which opens the next window; a window with nothing to send
/// ends the burst.
#[derive(Debug, Default)]
struct LibraryCoalescer {
    window: Option<Instant>,
    pending: Option<LibraryChanged>,
}
impl LibraryCoalescer {
    /// The event to send now, if any.
    fn push(&mut self, event: LibraryChanged, now: Instant) -> Option<LibraryChanged> {
        if self.window.is_none() {
            self.window = Some(now + LIBRARY_WINDOW);
            return Some(event);
        }
        self.pending = Some(match self.pending.take() {
            None => event,
            Some(pending) => merge(pending, event),
        });
        None
    }
    /// When the open window closes.
    fn deadline(&self) -> Option<Instant> {
        self.window
    }
    /// The window closed: the merged event to send, if any.
    fn expire(&mut self, now: Instant) -> Option<LibraryChanged> {
        let flushed = self.pending.take();
        self.window = flushed.as_ref().map(|_| now + LIBRARY_WINDOW);
        flushed
    }
    /// The bus closed: whatever is still pending.
    fn flush(&mut self) -> Option<LibraryChanged> {
        self.window = None;
        self.pending.take()
    }
}

/// One event for two changes: the game's own if both are about one game,
/// otherwise a scan (any game may have changed).
fn merge(earlier: LibraryChanged, later: LibraryChanged) -> LibraryChanged {
    match (&earlier.game_id, &later.game_id) {
        (Some(a), Some(b)) if a == b => LibraryChanged {
            // A game added in this window is still new to the UI.
            change: match (earlier.change, later.change) {
                (LibraryChange::Added, LibraryChange::Updated) => LibraryChange::Added,
                (_, change) => change,
            },
            game_id: later.game_id,
        },
        _ => LibraryChanged {
            change: LibraryChange::Scanned,
            game_id: None,
        },
    }
}

/// Sends the bus stream to `events` until the bus is gone. Library events
/// are coalesced ([`LIBRARY_WINDOW`]); every other event goes out at once,
/// in bus order.
pub fn bridge(events: Events, bus: &caskade_core::bus::Bus) {
    let mut stream = bus.stream();
    crate::runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        let send = |changed: LibraryChanged| events.send(Event::LibraryChanged(changed));
        let mut library = LibraryCoalescer::default();
        loop {
            let deadline = library.deadline();
            let received = tokio::select! {
                received = stream.recv() => received,
                () = tokio::time::sleep_until(
                    tokio::time::Instant::from_std(deadline.unwrap_or_else(Instant::now)),
                ), if deadline.is_some() => {
                    if let Some(changed) = library.expire(Instant::now()) {
                        send(changed);
                    }
                    continue;
                }
            };
            match received {
                Ok(event) => {
                    if let Some(changed) = library_change(&event) {
                        if let Some(changed) = library.push(changed, Instant::now()) {
                            send(changed);
                        }
                        continue;
                    }
                    if let Some(event) = translate(event) {
                        events.send(event);
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "UI bridge fell behind the bus");
                    // Library events may be among the lost ones.
                    let scanned = LibraryChanged {
                        change: LibraryChange::Scanned,
                        game_id: None,
                    };
                    if let Some(changed) = library.push(scanned, Instant::now()) {
                        send(changed);
                    }
                }
                Err(RecvError::Closed) => {
                    if let Some(changed) = library.flush() {
                        send(changed);
                    }
                    break;
                }
            }
        }
    });
}

/// Sends Activity's background snapshot to `events` whenever it changes,
/// at most every 100 ms: progress must not flood the UI with a snapshot for
/// every file or game in a large library.
pub fn bridge_background(events: Events) {
    let mut updates = caskade_core::jobs::global().subscribe();
    crate::runtime::spawn(async move {
        while updates.changed().await.is_ok() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let snapshot =
                caskade_core::i18n::translated_tasks(updates.borrow_and_update().clone());
            events.send(Event::BackgroundChanged(BackgroundChanged(snapshot)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(change: LibraryChange, id: &str) -> LibraryChanged {
        LibraryChanged {
            change,
            game_id: Some(id.into()),
        }
    }
    const SCANNED: LibraryChanged = LibraryChanged {
        change: LibraryChange::Scanned,
        game_id: None,
    };

    #[test]
    fn a_burst_sends_the_first_event_then_one_merged_event() {
        let start = Instant::now();
        let mut library = LibraryCoalescer::default();
        assert_eq!(
            library.push(game(LibraryChange::Added, "a"), start),
            Some(game(LibraryChange::Added, "a"))
        );
        assert_eq!(library.deadline(), Some(start + LIBRARY_WINDOW));
        for id in ["b", "c", "d"] {
            assert_eq!(library.push(game(LibraryChange::Added, id), start), None);
        }
        let closed = start + LIBRARY_WINDOW;
        assert_eq!(library.expire(closed), Some(SCANNED));
        // Sending opened another window; an empty one ends the burst.
        assert_eq!(library.deadline(), Some(closed + LIBRARY_WINDOW));
        assert_eq!(library.expire(closed + LIBRARY_WINDOW), None);
        assert_eq!(library.deadline(), None);
        assert_eq!(
            library.push(game(LibraryChange::Removed, "a"), closed + LIBRARY_WINDOW),
            Some(game(LibraryChange::Removed, "a"))
        );
    }

    #[test]
    fn changes_to_one_game_stay_that_game() {
        let now = Instant::now();
        let mut library = LibraryCoalescer::default();
        library.push(game(LibraryChange::Updated, "x"), now);
        library.push(game(LibraryChange::Added, "a"), now);
        library.push(game(LibraryChange::Updated, "a"), now);
        assert_eq!(library.expire(now), Some(game(LibraryChange::Added, "a")));
        library.push(game(LibraryChange::Updated, "a"), now);
        library.push(game(LibraryChange::Removed, "a"), now);
        assert_eq!(library.flush(), Some(game(LibraryChange::Removed, "a")));
        assert_eq!(library.flush(), None);
        assert_eq!(library.deadline(), None);
        library.push(SCANNED, now);
        library.push(game(LibraryChange::Updated, "a"), now);
        library.push(game(LibraryChange::Updated, "a"), now);
        assert_eq!(library.flush(), Some(game(LibraryChange::Updated, "a")));
    }
}
