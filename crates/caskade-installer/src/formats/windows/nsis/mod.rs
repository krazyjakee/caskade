//! Bounded static NSIS analysis and application-directory extraction.
//! Script actions are recorded as metadata; extraction never executes them.
use serde::{Deserialize, Serialize};
mod adapter;
mod extract;
mod metadata;
pub use adapter::{NsisHandler, NsisProbe, metadata_from_artifact, uninstall_steps};
pub use extract::NsisExtractor;
pub use metadata::{NsisEffect, NsisFile, NsisMetadata, NsisShortcut, app_path, inspect_bytes};

#[derive(Debug, Clone, Copy)]
pub struct NsisLimits {
    pub max_input_bytes: u64,
    pub max_decompressed_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_instructions: usize,
    pub max_path_updates: usize,
    pub max_path_bytes: usize,
}
impl Default for NsisLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 128 * 1024 * 1024,
            max_decompressed_bytes: 64 * 1024 * 1024,
            max_metadata_bytes: 4 * 1024 * 1024,
            max_instructions: 100_000,
            max_path_updates: 2048,
            max_path_bytes: 32 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum NsisError {
    #[error("NSIS I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid NSIS: {0}")]
    Invalid(String),
    #[error("NSIS analysis/decompression budget exceeded")]
    Limit,
    #[error(transparent)]
    Safety(#[from] crate::safety::SafetyError),
}
pub type Result<T> = std::result::Result<T, NsisError>;
pub(super) fn invalid(error: impl std::fmt::Display) -> NsisError {
    NsisError::Invalid(error.to_string())
}
