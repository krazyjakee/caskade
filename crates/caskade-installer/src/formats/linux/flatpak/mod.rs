//! Flatpak bundles and references: static identity, native user delegation.
mod adapter;
mod install;
mod metadata;
pub use adapter::{FlatpakHandler, FlatpakLaunchSpec, FlatpakProbe, metadata_from_artifact};
pub use install::{FlatpakInstallReport, FlatpakInstaller};
pub use metadata::{FlatpakMetadata, inspect_bundle, inspect_ref};
#[derive(Debug, thiserror::Error)]
pub enum FlatpakError {
    #[error("Flatpak IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Flatpak input: {0}")]
    Invalid(String),
    #[error("Flatpak limit exceeded: {0}")]
    Limit(&'static str),
}
pub type Result<T> = std::result::Result<T, FlatpakError>;
