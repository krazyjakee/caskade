//! Bounded pure-Rust expansion of Apple flat packages. Package scripts remain
//! data. System payloads are delegated to the injected macOS installer tool.
mod adapter;
mod cpio;
mod digest;
mod metadata;
mod pbzx;
mod toc;
mod xar;
pub use adapter::{PkgExtractor, PkgHandler, PkgProbe, extract};
pub use metadata::{Component, Package, inspect};
pub use toc::{Toc, XarEntry, parse_toc};
pub use xar::{Xar, XarLimits};
#[derive(Debug, thiserror::Error)]
pub enum PkgError {
    #[error("package I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Safety(#[from] crate::safety::SafetyError),
    #[error(transparent)]
    Control(#[from] crate::formats::archive::ArchiveError),
    #[error("invalid package: {0}")]
    Invalid(String),
    #[error("unsupported package: {0}")]
    Unsupported(String),
    #[error("package metadata or decoder limit exceeded")]
    Limit,
    #[error("package checksum mismatch")]
    Checksum,
    #[error("package requires system installation: {0}")]
    RequiresDelegate(String),
}
pub type Result<T> = std::result::Result<T, PkgError>;
fn invalid(message: impl Into<String>) -> PkgError {
    PkgError::Invalid(message.into())
}
