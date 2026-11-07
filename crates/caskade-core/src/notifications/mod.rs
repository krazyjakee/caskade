//! Notifications (#198): a feed kept in the library database, with read
//! state that survives restarts. `docs/notifications.md` has the category
//! matrix and the rules.
//!
//! Game additions don't depend on bus events: every game insert writes a
//! `game_additions` row in its own transaction (a trigger), and
//! [`Notifications::refresh`] folds the rows not yet seen into hourly
//! batches. A batch opens at its first addition and takes every addition
//! less than an hour after that; the next addition opens the next batch.
//! Writing a notification can fail without touching the import, and the
//! next refresh (at start, after library events, every minute) catches up.
//!
//! Each change worth reading gives its item the feed's next revision as its
//! `version`. Marking an item read records the version that was shown, so
//! an item changed since stays unread; mark all read takes the revision the
//! list was read at for the same reason.

mod service;
#[cfg(test)]
mod tests;

pub use service::{Clock, Notifications};

use crate::bus::SourceCount;
use crate::i18n::{Message, Text};
use serde::{Deserialize, Serialize};
use specta::Type;

/// How long a batch of game additions stays open after its first addition.
pub const BATCH_MS: i64 = 60 * 60 * 1000;
/// How long a game without an entry yet waits before it is counted anyway
/// (games are often saved a moment before their entries).
pub const ENTRY_GRACE_MS: i64 = 30 * 1000;
/// Read items untouched for this long are deleted. Unread ones never are.
pub const KEEP_READ_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// What a notification is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NotificationCategory {
    /// Games new to the library, one item per hourly batch.
    GamesAdded,
    /// An install, update, move, uninstall or verify that failed.
    JobFailed,
    /// Newer releases of installed games and managed emulators, or of
    /// Caskade itself.
    UpdatesAvailable,
    /// A backend plugin that kept crashing was turned off.
    PluginDisabled,
    /// A lifecycle script after a game started or exited failed (#179);
    /// the game and its session went on.
    ScriptFailed,
    /// Achievements unlocked since a set was last fetched (#190); never a
    /// set's first fetch, whose unlocks are history.
    AchievementsUnlocked,
    /// Someone received a game this device shared (#247).
    ShareReceived,
    /// A game someone shared with this device arrived (#247).
    ShareArrived,
}

impl NotificationCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GamesAdded => "games.added",
            Self::JobFailed => "job.failed",
            Self::UpdatesAvailable => "updates.available",
            Self::PluginDisabled => "plugin.disabled",
            Self::ScriptFailed => "script.failed",
            Self::AchievementsUnlocked => "achievements.unlocked",
            Self::ShareReceived => "share.received",
            Self::ShareArrived => "share.arrived",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "games.added" => Self::GamesAdded,
            "job.failed" => Self::JobFailed,
            "updates.available" => Self::UpdatesAvailable,
            "plugin.disabled" => Self::PluginDisabled,
            "script.failed" => Self::ScriptFailed,
            "achievements.unlocked" => Self::AchievementsUnlocked,
            "share.received" => Self::ShareReceived,
            "share.arrived" => Self::ShareArrived,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "warning" => Self::Warning,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

/// Where opening a notification goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NotificationTarget {
    /// The list of what it is about (`Notifications::members`).
    Members,
    /// A game's page.
    #[serde(rename_all = "camelCase")]
    Game { game_id: String },
    /// Activity's jobs.
    Jobs,
    /// A Settings tab, and a section of it.
    Settings {
        tab: String,
        section: Option<String>,
    },
}

/// One item of the feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub category: NotificationCategory,
    pub severity: Severity,
    pub title: String,
    pub body: String,
    /// Unix milliseconds.
    pub created_at_ms: i64,
    /// The last change worth reading.
    pub updated_at_ms: i64,
    /// What its members cover: a batch's first and last addition.
    pub first_at_ms: i64,
    pub last_at_ms: i64,
    /// How many members it has (games added, updates on offer).
    pub count: u32,
    /// Game additions per source, most first.
    pub sources: Vec<SourceCount>,
    pub target: NotificationTarget,
    /// The version shown: mark read with it.
    pub version: i64,
    pub unread: bool,
    /// The problem it was about has cleared (a failed job that worked
    /// later, an update installed).
    pub resolved: bool,
}

/// A page of the feed, newest change first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    /// Items the filter matches.
    pub total: u32,
    /// Unread items in the whole feed.
    pub unread: u32,
    /// The feed's revision when it was read: mark all read with it.
    pub revision: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MemberKind {
    Game,
    GameUpdate,
    EmulatorUpdate,
    /// A helper tool's (the streaming host or client).
    ToolUpdate,
}

impl MemberKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::GameUpdate => "gameUpdate",
            Self::EmulatorUpdate => "emulatorUpdate",
            Self::ToolUpdate => "toolUpdate",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "gameUpdate" => Self::GameUpdate,
            "emulatorUpdate" => Self::EmulatorUpdate,
            "toolUpdate" => Self::ToolUpdate,
            _ => Self::Game,
        }
    }
}

/// Where a member's game is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MemberState {
    /// Still in the library as it was.
    Present,
    /// Merged since into another game: `game_id` is that game.
    Merged,
    /// No longer in the library.
    Removed,
    /// Not a game (an emulator update).
    Other,
}

/// One thing a notification is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMember {
    pub kind: MemberKind,
    /// As it was when it arrived: a game's title, an update's versions.
    pub label: String,
    /// The library source it came from, for game additions.
    pub source: Option<String>,
    pub at_ms: i64,
    pub state: MemberState,
    /// The game to open: itself, or the one it was merged into.
    pub game_id: Option<String>,
    /// That game's title now.
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemberPage {
    pub items: Vec<NotificationMember>,
    pub total: u32,
}

/// A keyed notice: an event updates the item with its key rather than
/// adding another. The same title and body again changes nothing.
///
/// Its title and body are kept as messages and translated each time the
/// feed is read, so a language switch changes earlier items too (#214);
/// their English is stored beside them, and items from before keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub category: NotificationCategory,
    pub severity: Severity,
    pub key: String,
    pub title: Message,
    pub body: Text,
    pub target: NotificationTarget,
}

/// A member of a grouped notice, as the group is synced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    /// Unique in its notification: `game:<entry>@<version>`, …
    pub key: String,
    pub kind: MemberKind,
    pub game_id: Option<String>,
    pub entry_id: Option<String>,
    pub label: String,
}

/// A library source for people, in the language in use: `romm:<server>`
/// is RomM, and so on.
pub fn source_label(source: &str) -> String {
    source_text(&source_name(source)).render()
}

/// A library source's English name, as the feed stores it.
pub(crate) fn source_name(source: &str) -> String {
    if source.starts_with(crate::romm::SOURCE_PREFIX) {
        return "RomM".into();
    }
    if let Some(plugin) = source.strip_prefix(crate::bus::PLUGIN_SOURCE_PREFIX) {
        return plugin.to_owned();
    }
    if source.starts_with("stream:") {
        return "Other devices".into();
    }
    match source {
        "roms" => "ROM folders",
        "web" => "Web games",
        "caskade-storage" | "caskade-handoff" => "Installs",
        "steam" | "steam.shortcuts" => "Steam",
        "gog" | "minigalaxy" => "GOG",
        "epic" | "epic.app" | "legendary" | "rare" => "Epic",
        "heroic" => "Heroic",
        "lutris" => "Lutris",
        "itch" => "itch.io",
        "xbox" | "gameux" => "Xbox",
        "ea" => "EA",
        "ubisoft" => "Ubisoft",
        "battlenet" => "Battle.net",
        "amazon" => "Amazon",
        "flatpak" => "Flatpak",
        // Linux app menu entries (`.desktop` files).
        "desktop" => "Desktop entries",
        "other" => "Other",
        other => other,
    }
    .to_owned()
}

/// A stored source name to translate: the names that aren't a store's or
/// a plugin's own.
pub(crate) fn source_text(name: &str) -> Text {
    match name {
        "ROM folders" => crate::msgp!("library source", "ROM folders").into(),
        "Web games" => crate::msgp!("library source", "Web games").into(),
        "Installs" => crate::msgp!("library source", "Installs").into(),
        // Feeds stored before the name was given kept the id.
        "Desktop entries" | "desktop" => crate::msgp!("library source", "Desktop entries").into(),
        "Other devices" => crate::msgp!("library source", "Other devices").into(),
        "Other" => crate::msgp!("library source", "Other").into(),
        other => other.into(),
    }
}

/// "10 games added", or the title of a batch's only game.
pub(crate) fn batch_title(count: u32, only: Option<&str>) -> Message {
    match (count, only) {
        (1, Some(title)) => crate::msg!("{title} added", title = title),
        (n, _) => crate::msgn!("{n} game added", "{n} games added", n),
    }
}

/// "RomM 6 · Steam 4", from sources stored by their English names.
pub(crate) fn sources_body(sources: &[SourceCount]) -> Text {
    let mut items = sources.iter().rev().map(|s| {
        crate::msgp!(
            "games added from a library source",
            "{source} {count}",
            source = source_text(&s.source),
            count = s.count
        )
    });
    let Some(last) = items.next() else {
        return Text::Plain(String::new());
    };
    items
        .fold(last, |rest, item| {
            crate::msgp!(
                "list of library sources",
                "{first} · {rest}",
                first = item,
                rest = rest
            )
        })
        .into()
}
