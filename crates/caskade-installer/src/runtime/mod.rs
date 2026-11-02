//! Process runtimes: native, Proton prefixes and Wine prefixes.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

pub use crate::types::RuntimeSpec;

pub mod alias;
pub mod appimage;
mod native;
pub mod proton;
pub mod registry;
pub mod umu;
mod watch;
pub use appimage::outside_appimage;
pub use native::NativeRuntime;
pub use watch::{OutOfRootWatcher, default_watch_roots};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    Type,
)]
#[serde(transparent)]
pub struct ProcessId(pub u64);

/// Resolved process invocation. Arguments remain separate OS arguments, and
/// `env` contains overrides rather than an implicit shell command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ProcessCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ProcessExit {
    pub code: i32,
}

#[derive(
    Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuntimeError {
    #[error("runtime operation {operation} is not implemented")]
    NotImplemented { operation: String },
    #[error("no fake script queued for {program}")]
    NoScript { program: PathBuf },
    #[error("unknown process {id:?}")]
    UnknownProcess { id: ProcessId },
    #[error("process {id:?} timed out")]
    TimedOut { id: ProcessId },
    #[error("process {id:?} was killed")]
    Killed { id: ProcessId },
    #[error("runtime I/O failed: {message}")]
    Io { message: String },
}

/// Synchronous process seam, used from the executor's blocking task. A runtime
/// must bound wait by `timeout` and terminate the process tree on expiration.
pub trait Runtime: Send + Sync {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError>;
    fn wait(&self, process: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError>;
    /// Terminate the process and all descendants, including after parent exit.
    fn kill(&self, process: ProcessId) -> Result<(), RuntimeError>;
    /// Spawn with at most [`MAX_STDIN_BYTES`] written to stdin, which is then
    /// closed (e.g. a licence acceptance answer). Runtimes without stdin
    /// support report `NotImplemented`.
    fn spawn_with_input(
        &self,
        command: &ProcessCommand,
        input: &[u8],
    ) -> Result<ProcessId, RuntimeError> {
        let _ = (command, input);
        Err(RuntimeError::NotImplemented {
            operation: "spawn_with_input".into(),
        })
    }
}

/// Upper bound for [`Runtime::spawn_with_input`]; it fits a pipe buffer, so
/// writing never blocks on a child that does not read.
pub const MAX_STDIN_BYTES: usize = 4096;
