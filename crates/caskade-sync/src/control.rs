//! Companion remote-control wire messages over `caskade/sync/1`
//! ([`crate::Purpose::Control`]).
//!
//! Each message is one JSON object per line over the session's bidirectional
//! stream (`FrameReader` / `FrameWriter`), bounded on both sides so neither a
//! command nor a library page can grow without limit.

use serde::{Deserialize, Serialize};

/// Longest command line accepted from a companion peer.
pub const MAX_COMMAND_BYTES: usize = 4096;

/// Longest event line accepted from a desktop host (a bounded page of library
/// summaries or a state snapshot).
pub const MAX_EVENT_BYTES: usize = 256 * 1024;

/// Maximum number of games returned in one `ListGames` page.
pub const MAX_GAMES_PAGE: u32 = 200;

/// Commands sent from a paired companion device to a Caskade instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CompanionCommand {
    /// Request the current instance state snapshot.
    GetState,
    /// Request a page of library game summaries visible to the active account.
    ListGames {
        #[serde(default)]
        offset: u32,
        #[serde(default)]
        limit: u32,
    },
    /// Toggle favourite state for a library game.
    ToggleFavourite { game_id: String },
    /// Start a game through Caskade's launch pipeline (age limits, PINs and
    /// saves still apply on the host).
    LaunchGame { game_id: String },
    /// Stop the running game on the connected instance.
    StopGame {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        game_id: Option<String>,
    },
    /// Switch between desktop mode and fullscreen console mode.
    SetConsoleMode { enabled: bool },
    /// Deliver a normalised `caskade-input` action (`nav.up`, `confirm`,
    /// `back`, `details`, `search`, `menu`, `view`, `tab.prev`, `tab.next`,
    /// `page.prev`, `page.next`, `guide`).
    PadAction { action: String },
    /// Navigate the Caskade window to a top-level route (`/`, `/library`,
    /// `/collections`, `/store`, `/settings`, `/activity`).
    OpenView { route: String },
    /// Request an immediate library sync round with this peer.
    SyncNow,
}

/// Live state snapshot of a connected Caskade instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceState {
    pub device_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_account: Option<String>,
    pub console_mode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_game_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_game_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_session_id: Option<String>,
}

/// High-level availability status of a game on the host instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompanionGameStatus {
    Installed,
    Available,
    Emulated,
    Missing,
}

/// Compact library summary sent to a paired companion app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionGame {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub source: String,
    pub status: CompanionGameStatus,
    pub playtime_ms: u64,
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Events and replies sent from a Caskade host back to the paired companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CompanionEvent {
    /// Current state snapshot (sent on connect, on `GetState`, and whenever
    /// running game, active account, or console mode changes).
    State(InstanceState),
    /// Page of games answering `ListGames`.
    Games {
        games: Vec<CompanionGame>,
        total: u64,
    },
    /// Outcome of a control command.
    CommandResult {
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

/// Normalises a `route` requested by a companion into an allowlisted Caskade
/// top-level route, or `None` if it isn't permitted.
pub fn normalize_route(route: &str) -> Option<&'static str> {
    match route.trim() {
        "/" | "/home" => Some("/"),
        "/library" => Some("/library"),
        "/collections" => Some("/collections"),
        "/store" => Some("/store"),
        "/settings" => Some("/settings"),
        "/activity" => Some("/activity"),
        _ => None,
    }
}

/// Allowlisted `caskade-input` action wire names accepted over `Purpose::Control`.
pub const ALLOWED_PAD_ACTIONS: &[&str] = &[
    "nav.up",
    "nav.down",
    "nav.left",
    "nav.right",
    "confirm",
    "back",
    "details",
    "search",
    "menu",
    "view",
    "tab.prev",
    "tab.next",
    "page.prev",
    "page.next",
    "guide",
];

/// Returns `true` when `action` is a valid `caskade-input` wire action name.
pub fn is_valid_pad_action(action: &str) -> bool {
    ALLOWED_PAD_ACTIONS.contains(&action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip_as_single_json_lines() {
        let commands = [
            CompanionCommand::GetState,
            CompanionCommand::ListGames {
                offset: 0,
                limit: 50,
            },
            CompanionCommand::ToggleFavourite {
                game_id: "g-1".into(),
            },
            CompanionCommand::LaunchGame {
                game_id: "g-1".into(),
            },
            CompanionCommand::StopGame { game_id: None },
            CompanionCommand::SetConsoleMode { enabled: true },
            CompanionCommand::PadAction {
                action: "nav.up".into(),
            },
            CompanionCommand::OpenView {
                route: "/library".into(),
            },
            CompanionCommand::SyncNow,
        ];

        for command in commands {
            let line = serde_json::to_string(&command).unwrap();
            assert!(!line.contains('\n'));
            assert!(line.len() < MAX_COMMAND_BYTES);
            let parsed: CompanionCommand = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed, command);
        }
    }

    #[test]
    fn events_use_camel_case_wire_shape() {
        let state = CompanionEvent::State(InstanceState {
            device_name: "Living room TV".into(),
            active_account: Some("Alex".into()),
            console_mode: true,
            running_game_id: Some("hades-ii".into()),
            running_game_title: Some("Hades II".into()),
            running_session_id: Some("sess-1".into()),
        });
        let encoded = serde_json::to_string(&state).unwrap();
        assert!(encoded.contains(r#""type":"state""#), "{encoded}");
        assert!(
            encoded.contains(r#""deviceName":"Living room TV""#),
            "{encoded}"
        );
        assert!(encoded.contains(r#""consoleMode":true"#), "{encoded}");
        let decoded: CompanionEvent = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn route_and_pad_action_allowlists_reject_unknown_values() {
        assert_eq!(normalize_route("/home"), Some("/"));
        assert_eq!(normalize_route("/library"), Some("/library"));
        assert_eq!(normalize_route("/settings"), Some("/settings"));
        assert_eq!(normalize_route("javascript:alert(1)"), None);
        assert_eq!(normalize_route("/plugins/evil"), None);

        assert!(is_valid_pad_action("nav.up"));
        assert!(is_valid_pad_action("details"));
        assert!(is_valid_pad_action("guide"));
        assert!(!is_valid_pad_action("quick_look"));
        assert!(!is_valid_pad_action("exec"));
    }
}
