//! Read-only native DMG mounts with unconditional bounded detach cleanup.
mod adapter;
mod metadata;
mod mount;
pub use adapter::{DmgExtractor, DmgHandler, DmgProbe, copy_apps};
pub use metadata::{DmgMetadata, inspect, parse_resource_plist, parse_trailer};
pub use mount::MountedImage;
#[derive(Debug, thiserror::Error)]
pub enum DmgError {
    #[error("DMG I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Runtime(#[from] crate::runtime::RuntimeError),
    #[error(transparent)]
    Control(#[from] crate::formats::archive::ArchiveError),
    #[error(transparent)]
    App(#[from] super::app::AppError),
    #[error("invalid DMG: {0}")]
    Invalid(String),
    #[error("DMG metadata limit exceeded")]
    Limit,
    #[error("unsupported DMG: {0}")]
    Unsupported(String),
    #[error("hdiutil {operation} failed with exit code {code}")]
    Process { operation: &'static str, code: i32 },
    #[error("DMG detach failed; mount retained at {mountpoint}: {message}")]
    Detach {
        mountpoint: std::path::PathBuf,
        message: String,
    },
}
pub type Result<T> = std::result::Result<T, DmgError>;
fn invalid(message: impl Into<String>) -> DmgError {
    DmgError::Invalid(message.into())
}
