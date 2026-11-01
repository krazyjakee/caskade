//! Shared wire types for input, analysis, planning and outcomes.
//!
//! These types have no dependency on handlers, host detection or execution.
//! Every type supports serde, JSON Schema and Specta reflection, regardless of
//! Cargo features. JSON enums use an adjacent `type`/`value` representation and
//! snake_case variant names. Ordered maps keep serialized plans deterministic.
//!
//! `PathBuf` deliberately preserves OS paths at the input boundary. Serialized
//! paths must be Unicode. Source inputs may be absolute; plan destinations,
//! expected files, working directories and launch targets are relative to the
//! staging/game directory. `Installed::dir` is relative to the storage root.
//! These are untrusted descriptions, not filesystem capabilities: the safety
//! layer must validate every path before executing a deserialized plan.

mod artifact;
mod input;
mod outcome;
mod plan;
mod step;

pub use artifact::*;
pub use input::*;
pub use outcome::*;
pub use plan::*;
pub use step::*;
