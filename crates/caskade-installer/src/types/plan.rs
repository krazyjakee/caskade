use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ArtifactNode, Input, Question, RuntimeSpec, Step, Warning};

/// Built-in or plugin handler identifier; unrelated to the Cargo feature set.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(transparent)]
pub struct HandlerId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Copy,
    Extract,
    RunSilent,
    RunInteractive,
    Delegate,
    HandOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    Exact,
    Equivalent,
    Partial,
    Native,
}

/// Capabilities and side effects used by policy to filter/rank a proposal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Requirements {
    pub executes_code: bool,
    /// None means no process runtime is needed (e.g. pure extraction).
    pub runtime: Option<RuntimeSpec>,
    pub may_prompt_elevation: bool,
    pub interactive: bool,
    pub est_disk_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct Candidate {
    pub handler: HandlerId,
    pub strategy: Strategy,
    /// A finite probability in 0..=1; validated by the plan engine.
    #[schemars(range(min = 0, max = 1))]
    pub confidence: f32,
    pub fidelity: Fidelity,
    pub requires: Requirements,
    pub steps: Vec<Step>,
    pub warnings: Vec<Warning>,
}

/// Complete, serializable execution request. Source paths may be absolute;
/// `relative_dir` is always relative to the root identified by `root_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct InstallPlan {
    pub schema_version: u32,
    pub input: Input,
    pub artifacts: Vec<ArtifactNode>,
    /// Resolved to an absolute root only at the executor boundary.
    pub root_id: String,
    pub relative_dir: PathBuf,
    pub candidate: Candidate,
}

/// Ranked proposals, with an optional selection or a question for ambiguity.
/// A question means execution must wait for an answer. `selected`, when set,
/// indexes `candidates` and must be validated before execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct PlanSet {
    pub candidates: Vec<Candidate>,
    pub selected: Option<u32>,
    pub question: Option<Question>,
}
