//! Process identity and launcher-independent game tracking (#108).
//!
//! Snapshots contain only the Steam app id from process environments; full
//! command lines and environments are never persisted or logged.
mod matching;
mod monitor;
mod system;

pub use matching::{ProcessTracker, TrackingMode, TrackingTarget};
pub use monitor::{ProcessMonitor, TrackingError};
pub use system::SystemProcesses;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A PID alone is unsafe across app restarts: the OS may reuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Seconds since the Unix epoch, as reported by sysinfo.
    pub started_at: u64,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub identity: ProcessIdentity,
    pub parent: Option<u32>,
    /// OS process name (Wine can expose game.exe while exe is its loader).
    pub name: Option<String>,
    pub executable: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub arguments: Vec<PathBuf>,
    pub steam_app_id: Option<u32>,
}

/// Zombies/dead processes are omitted; inaccessible metadata remains None.
#[derive(Debug, Clone, Default)]
pub struct ProcessSnapshot {
    pub processes: Vec<ProcessInfo>,
}
