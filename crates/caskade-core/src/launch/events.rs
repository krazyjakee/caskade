use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTicket {
    pub game_id: String,
    pub entry_id: String,
    pub session_id: String,
}

/// `game.started` and `game.exited` share identity fields. No process argv,
/// environment variables or machine paths cross the frontend event bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEvent {
    /// Trusted launch.after payload; the frontend bridge forwards only session identity/timing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<super::LaunchCommand>,
    pub game_id: String,
    pub entry_id: String,
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub duration_ms: u64,
    pub status: SessionStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Exited,
    /// The directly launched process returned a nonzero exit code.
    Crashed,
    /// App restart found no matching PID/start-time identity; time stops at
    /// the last persisted heartbeat rather than charging app downtime.
    Interrupted,
    NotStarted,
}
