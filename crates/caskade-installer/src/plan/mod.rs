//! Handler proposals, policy, ranking and serializable execution plans.
//!
//! Planning is pure: handlers inspect supplied artifacts and capabilities, and
//! describe steps without running programs or changing the installation root.
//! An empty `PlanSet` means no available handler can install the input. A set
//! with a question must not execute until that question has been answered.

mod engine;
mod policy;
pub mod presets;
mod ranking;
pub(crate) use ranking::executes;

#[cfg(test)]
mod tests;

pub use engine::{CandidateTransform, Handler, PlanContext, PlanEngine, PlanError, Proposal};
pub use policy::{ExecutionPolicy, Policy, PresetArgs, WindowsMethod};

pub use crate::types::{
    Candidate, Fidelity, HandlerId, InstallPlan, PlanSet, Requirements, Step, Strategy,
};
