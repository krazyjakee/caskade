//! Finds games that launchers, stores and the operating system have already
//! installed, and returns them as plain data.
//!
//! The crate knows nothing about the database or Tauri. Sources read files and
//! the registry directly and never start subprocesses. The library emits
//! `tracing` events and spans without setting a global subscriber.

pub mod classify;
pub mod data;
#[cfg(feature = "deep")]
pub mod deep;
pub mod dump;
pub mod env;
pub mod error;
pub mod identity;
pub mod merge;
pub mod orchestrator;
pub mod parsers;
pub mod probes;
pub mod registry;
pub mod sources;
pub mod testing;
pub mod types;
pub mod uri;
pub mod watch;

pub use caskade_installer::OperatingSystem;
pub use env::HostEnv;
pub use error::{Error, Result};
pub use orchestrator::{
    ClassifierConfig, Config, DeepScans, Exclusions, SourceKey, SourceSelection, detect_launchers,
    detect_launchers_with_sources, scan, scan_with_sources,
};
pub use types::*;
