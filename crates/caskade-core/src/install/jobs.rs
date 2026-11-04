//! Install job snapshots: what the UI lists and follows.

use caskade_installer::{Question, Warning, events::Phase};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Install,
    Uninstall,
    Move,
    Fetch,
    Verify,
    /// A newer release installed over a game (`crate::updates`).
    Update,
    /// A game copied from another of the user's devices (#233).
    PeerCopy,
    /// Every game on a storage location moved to another, then the location
    /// removed (#64).
    MoveLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Downloading,
    Installing,
    NeedsUser,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    Download,
    Install,
    Move,
}

impl From<Phase> for JobPhase {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Download => Self::Download,
            Phase::Install => Self::Install,
            Phase::Move => Self::Move,
        }
    }
}

/// The last progress report of the running phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub files: u64,
    pub eta_secs: Option<u64>,
    pub message: String,
}

/// One install, uninstall or move as the UI sees it. `revision` grows on
/// every change, so stale updates and answers can be told apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: String,
    pub revision: u32,
    pub kind: JobKind,
    pub status: JobStatus,
    pub title: String,
    /// The path or URL as given.
    pub input: String,
    pub root_id: Option<String>,
    pub phase: Option<JobPhase>,
    pub progress: Option<JobProgress>,
    pub warnings: Vec<Warning>,
    pub question: Option<Question>,
    /// The library game, once installed or handed off.
    pub game_id: Option<String>,
    pub error: Option<String>,
    pub candidates: Vec<caskade_installer::Candidate>,
    pub selected_candidate_index: Option<u32>,
    pub target_dir: Option<std::path::PathBuf>,
    pub dependencies: Vec<caskade_installer::Dependency>,
    pub verification: Option<super::Verification>,
    /// Why the job failed and how it can recover (#158); `None` while it
    /// runs and after it succeeds.
    pub failure: Option<caskade_installer::failure::Failure>,
    /// Unix milliseconds.
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl JobSnapshot {
    /// A new job, queued now.
    pub(super) fn queued(
        kind: JobKind,
        title: String,
        input: String,
        root_id: Option<String>,
    ) -> Self {
        let now = crate::now_ms();
        Self {
            id: crate::model::new_id(),
            revision: 0,
            kind,
            status: JobStatus::Queued,
            title,
            input,
            root_id,
            phase: None,
            progress: None,
            warnings: Vec::new(),
            question: None,
            game_id: None,
            error: None,
            candidates: Vec::new(),
            selected_candidate_index: None,
            target_dir: None,
            dependencies: Vec::new(),
            verification: None,
            failure: None,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }
}
