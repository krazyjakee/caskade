//! Execute plans with staging, journaling, progress and cancellation.
//!
//! The final `completed` event is cancellable. Success becomes durable immediately
//! afterwards; recovery preserves completed installs and rolls back unfinished
//! jobs. Root exclusion is process-wide and includes detached blocking workers.
//!
//! The app resolves the root and artifact source paths into `ExecRequest` and
//! supplies the chosen `PlanSet`. A pending question is rejected. The selected
//! candidate is attempted first, followed by the remaining ranked alternatives.
//! `ExecReport` supplies the actual directory, selected candidate, failure chain
//! and external effects to the subsequent discovery/manifest pipeline.
//!
//! Native installer argument builders should use `INSTALL_DIR` rather than a
//! guessed folder path: it expands to the actual final folder after collision
//! resolution. Extraction helpers use `Extractor` and `StepContext`; OS watchers
//! implement `ProcessObserver` to report side effects and uninstall compensation.
//! Watchers must stop their threads by `finish` (and on drop after an unwind).
//!
//! Original-input `Move` and OS-specific mount/registry steps return an explicit
//! unsupported-step error until their source-retention/OS adapters are installed.
//! `Move` within the game directory is transactional and supported here.

mod api;
mod context;
mod engine;
mod exclusive;
pub mod interactive;
mod journal;
mod steps;

pub(crate) use api::source_path;
pub use api::{
    AttemptFailure, ExecConfig, ExecError, ExecReport, ExecRequest, Extractor, FinalizationContext,
    Finalizer, ProcessObservation, ProcessObserver, Result, folder_name,
};
pub use context::{CheckedReader, StepContext};
pub use engine::Executor;
pub use exclusive::{with_root_exclusive, with_roots_exclusive};
pub use journal::{JournalEntry, JournalHandle};
pub use steps::{INSTALL_DIR, source_argument};
pub use tokio_util::sync::CancellationToken;
