use crate::tracking::{ProcessIdentity, TrackingTarget};
use serde::{Deserialize, Serialize};

/// Versioned payload stored alongside PlaySession. Paths are runtime recovery
/// metadata, never rewritten into a managed entry's relative LaunchSpec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<crate::launch::LaunchCommand>,
    pub version: u32,
    pub target: TrackingTarget,
    pub processes: Vec<ProcessIdentity>,
    pub launcher: Option<ProcessIdentity>,
    pub phase: SessionPhase,
    /// The play action the session started (#177); `None` before actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The lifecycle scripts' context and whether the after-start ones
    /// ran (#179); `None` for sessions from before scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<ScriptState>,
}

/// What a session's after-start and after-exit scripts need across a
/// restart: the context the launch had, and whether after-start ran.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptState {
    pub context: crate::scripts::ScriptContext,
    pub started: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    AwaitingGame,
    Playing,
}
impl RecoveryState {
    pub fn pending(target: TrackingTarget) -> Self {
        Self {
            command: None,
            version: 1,
            target,
            processes: vec![],
            launcher: None,
            phase: SessionPhase::AwaitingGame,
            action: None,
            scripts: None,
        }
    }
}
