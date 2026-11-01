//! Read-only, evidence-based game classification for OS and deep-scan sources.
mod rules;
mod scan;

use crate::{ClassifierConfig, Confidence, Evidence, LaunchTarget};
pub use scan::classify_path;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    NotAGame,
    Maybe,
    Likely,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default)]
pub struct Metadata {
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub category: Option<String>,
    pub install_dir: Option<PathBuf>,
    pub executable: Option<PathBuf>,
    pub known_game_folders: Vec<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Classification {
    pub score: i32,
    pub verdict: Verdict,
    pub evidence: Vec<Evidence>,
    pub launch: Option<LaunchTarget>,
    pub warnings: Vec<String>,
}
impl Classification {
    /// Classifier heuristics alone never claim Certain.
    pub fn confidence(&self) -> Option<Confidence> {
        match self.verdict {
            Verdict::NotAGame => None,
            Verdict::Maybe => Some(Confidence::Maybe),
            Verdict::Likely => Some(Confidence::Likely),
        }
    }
}
/// Classify a caller-supplied bounded folder inventory. Paths are relative to
/// the candidate root and include directories; this entry point performs no I/O.
/// Fingerprint arrays in engines.toml are conjunctive; separate rules express alternatives.
pub fn classify_inventory(
    metadata: &Metadata,
    files: &[String],
    config: &ClassifierConfig,
) -> crate::Result<Classification> {
    rules::classify(metadata, files, config)
}
