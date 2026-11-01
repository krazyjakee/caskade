//! Static Windows Installer table analysis and pure Rust cabinet extraction.
//! No install action, custom action or shortcut is executed during extraction.
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek};

mod adapter;
mod extract;
mod metadata;
mod preflight;
pub use adapter::{MsiHandler, MsiProbe, metadata_from_artifact, uninstall_steps};
pub use extract::MsiExtractor;
pub use metadata::{MsiFile, MsiMedia, MsiMetadata};

#[derive(Debug, Clone, Copy)]
pub struct MsiLimits {
    pub max_input_bytes: u64,
    pub max_metadata_bytes: u64,
    pub max_read_bytes: u64,
    pub max_string_bytes: u64,
    pub max_rows: usize,
}
impl Default for MsiLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 * 1024 * 1024 * 1024,
            max_metadata_bytes: 8 * 1024 * 1024,
            max_read_bytes: 32 * 1024 * 1024,
            max_string_bytes: 256 * 1024,
            max_rows: 100_000,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum MsiError {
    #[error("MSI I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid MSI: {0}")]
    Invalid(String),
    #[error("MSI metadata budget exceeded")]
    Limit,
    #[error("unsupported MSI: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Safety(#[from] crate::safety::SafetyError),
}
pub type Result<T> = std::result::Result<T, MsiError>;

/// Bounded metadata-only entry point, also used by the fuzz harness.
pub fn inspect_reader(reader: impl Read + Seek, limits: MsiLimits) -> Result<MsiMetadata> {
    let (mut package, _) = preflight::open(reader, limits)?;
    metadata::read(&mut package, limits)
}
