//! Typed events from the core to the frontend. The bus bridge forwards the
//! core bus events the UI needs.

use caskade_core::bus::{Event as BusEvent, GameSession};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::{Duration, Instant};
use tauri_specta::Event;

/// Emitted after every `hello` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct Hello(pub crate::commands::Greeting);

/// `game.started`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct GameStarted(pub GameSession);

/// `game.exited`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub change: LibraryChange,
    /// `None` after a scan.
    pub game_id: Option<String>,
}

/// A complete revisioned install/download/move snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Event)]
pub struct JobChanged(pub caskade_core::install::JobSnapshot);

/// `settings.changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    pub namespace: String,
    pub version: u32,
}

/// `emulation.changed`: refetch emulators, platforms or scanners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct EmulationChanged {
    /// `emulators`, `platforms` or `scanners`.
    pub scope: String,
}

/// Every change of an emulator detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct EmulationDetectChanged(pub caskade_core::emulation::DetectJob);

/// Every change of a ROM scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct EmulationScanChanged(pub caskade_core::emulation::ScanJob);

/// Every change of an emulator install or update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct EmulationSetupChanged(pub caskade_core::emulation::SetupJob);

/// `plugin.disabled`: a backend plugin kept crashing and was turned off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PluginDisabled {
    pub plugin: String,
    pub reason: String,
}

/// `plugins.changed`: a plugin was enabled, disabled, installed or removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PluginsChanged {
    pub plugin: String,
}

/// `plugin.reloaded`: a dev plugin changed on disk (#131).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AccountChanged {
    /// The signed-in account; `None` when nobody is.
    pub active_id: Option<String>,
}

/// `lifecycle.scripts.ran`: a launch's scripts of one phase ran (#179),
/// with their bounded output. The user's own; plugins never see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct LifecycleScriptsRan(pub caskade_core::scripts::ScriptReport);

/// `achievements.changed`: a game's achievements were fetched (#185).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsChanged {
    pub game_id: String,
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

/// Forwards one bus event to the webview; events the UI does not use are
/// ignored.
pub fn forward<R: tauri::Runtime>(app: &tauri::AppHandle<R>, event: BusEvent) -> tauri::Result<()> {
    if let Some(changed) = library_change(&event) {
        return changed.emit(app);
    }
    match event {
        BusEvent::GameStarted(session) => GameStarted(session).emit(app),
        BusEvent::GameExited(session) => GameExited(session).emit(app),
        BusEvent::InstallJobChanged(snapshot) => JobChanged(*snapshot).emit(app),
        BusEvent::SettingsChanged { namespace, version } => {
            SettingsChanged { namespace, version }.emit(app)
        }
        BusEvent::EmulationChanged { scope } => EmulationChanged { scope }.emit(app),
        BusEvent::EmulationDetectChanged(job) => EmulationDetectChanged(*job).emit(app),
        BusEvent::EmulationScanChanged(job) => EmulationScanChanged(*job).emit(app),
        BusEvent::EmulationSetupChanged(job) => EmulationSetupChanged(*job).emit(app),
        BusEvent::PluginDisabled { plugin, reason } => PluginDisabled { plugin, reason }.emit(app),
        BusEvent::PluginsChanged { plugin } => PluginsChanged { plugin }.emit(app),
        BusEvent::PluginReloaded { plugin, ui, error } => {
            PluginReloaded { plugin, ui, error }.emit(app)
        }
        BusEvent::AccountChanged { active_id } => AccountChanged { active_id }.emit(app),
        BusEvent::LifecycleScriptsRan(report) => LifecycleScriptsRan(*report).emit(app),
        BusEvent::AchievementsChanged { game_id } => AchievementsChanged { game_id }.emit(app),
        _ => Ok(()),
    }
}

/// How long library events are merged after one went out. Imports and ROM
/// scans emit one event per game; each `libraryChanged` makes the UI refetch
/// the library and the storage list.
pub const LIBRARY_WINDOW: Duration = Duration::from_millis(200);

/// Rate-limits `libraryChanged`: the first event of a quiet period goes out at
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

/// Forwards the bus stream to the webview until the bus is gone. Library
/// events are coalesced ([`LIBRARY_WINDOW`]); every other event goes out at
/// once, in bus order.
pub fn bridge<R: tauri::Runtime>(app: tauri::AppHandle<R>, bus: &caskade_core::bus::Bus) {
    let mut stream = bus.stream();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        let send = |changed: LibraryChanged| {
            if let Err(error) = changed.emit(&app) {
                tracing::warn!(event = "library.changed", %error, "could not forward a bus event");
            }
        };
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
                    let name = event.name();
                    if let Err(error) = forward(&app, event) {
                        tracing::warn!(event = name, %error, "could not forward a bus event");
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
