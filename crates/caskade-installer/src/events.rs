//! Structured progress sinks shared by downloads, installs and moves.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Download,
    Install,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct JobEvent {
    pub job_id: String,
    pub phase: Phase,
    pub step: Option<u32>,
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub files: u64,
    pub eta_secs: Option<u64>,
    pub message: String,
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: JobEvent);
}
