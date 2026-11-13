//! Static Inno metadata and confined extraction. Never executes Pascal [Code].
//!
//! `inno` 0.7.1 supplies metadata and streaming external-slice extraction;
//! `innospect` 0.2.0 supplies bounded embedded extraction and the encrypted
//! 6.4 metadata fallback. Every selected file is drained through checksum EOF.
//! Password-protected inputs produce no executable plan and fail extraction
//! with `PasswordRequired`. Registry/INI/Code/custom effects are retained as
//! metadata and make static extraction Partial; they are never executed here.
//!
//! Register `InnoProbe` before the generic `PeProbe` in first-match registries.
//! After successful execution, consume the extractor's discovery hints.
//! Native-only uninstall recording is coordinated by the
//! Windows handler module so extracted installs never run a native uninstaller.
mod adapter;
mod extract;
mod metadata;
mod paths;
mod prefix;
pub use metadata::{
    FileMetadata, IconMetadata, IniMetadata, InnoMetadata, RegistryMetadata, RunMetadata,
    inspect_bytes, inspect_path,
};
pub use paths::{app_path, split_arguments};

pub type Result<T> = std::result::Result<T, InnoError>;
#[derive(Debug, thiserror::Error)]
pub enum InnoError {
    #[error("Inno I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Inno path safety: {0}")]
    Safety(#[from] crate::safety::SafetyError),
    #[error("invalid Inno installer: {0}")]
    Invalid(String),
    #[error("unsupported Inno installer: {0}")]
    Unsupported(String),
    #[error("password-protected Inno installer requires user-supplied credentials")]
    PasswordRequired,
    #[error("Inno extraction control: {0}")]
    Control(#[from] crate::formats::archive::ArchiveError),
    #[error("Inno limit exceeded: {0}")]
    Limit(&'static str),
}
#[derive(Debug, Clone, Copy)]
pub struct InnoLimits {
    /// The encrypted-metadata fallback consumes bytes; plain metadata streams.
    pub max_metadata_input_bytes: u64,
    pub max_entries: usize,
    /// innospect caches solid chunks. Larger chunks use inno's streaming reader.
    pub max_cached_chunk_bytes: u64,
}
impl Default for InnoLimits {
    fn default() -> Self {
        Self {
            max_metadata_input_bytes: 8 * 1024 * 1024,
            max_entries: 100_000,
            max_cached_chunk_bytes: 16 * 1024 * 1024,
        }
    }
}

pub use adapter::{InnoExtractor, InnoHandler, InnoProbe, metadata_from_artifact, uninstall_steps};
pub use extract::extract_path;
