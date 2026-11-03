//! Plan-driven game installation, independent of the application and its UI.
//!
//! Handlers produce data; only the executor may change the installation target.
//! Library code emits `tracing` events without installing a global subscriber.
//! Execution entry points should create a span with `job_id` and `handler`
//! fields; child operations inherit that span. Do not log credentials, download
//! headers or complete user-supplied URLs. The CLI owns subscriber setup.

pub mod discover;
pub mod disk;
pub mod events;
pub mod exec;
pub mod failure;
#[cfg(feature = "fetch")]
pub mod fetch;
pub mod formats;
pub mod host;
pub mod input;
pub mod manifest;
pub mod pipeline;
pub mod plan;
pub mod probe;
pub mod quarantine;
pub mod recipes;
pub mod reconcile;
pub mod redist;
pub mod relocate;
pub mod runtime;
pub mod safety;
pub mod scan;
pub mod testing;
pub mod types;

pub use plan::{
    ExecutionPolicy, Handler, PlanContext, PlanEngine, PlanError, Policy, PresetArgs, Proposal,
    WindowsMethod,
};
pub use types::*;
