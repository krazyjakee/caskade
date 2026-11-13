//! The discovery service and library imports.
//! No global state, filesystem mutations in launcher data or ambient scans.
pub mod emulation;
mod import;
mod progress;
mod roms;
pub use progress::DiscoveryProgress;
mod service;
mod settings;
pub mod storage;
pub mod tools;
mod watch;

pub use import::{ImportOptions, ImportOutcome, ImportedGame, SkippedEntry};
pub use roms::RomsSource;
pub use service::{DiscoveryService, ScanOutcome};
pub use settings::{IMPORT_SETTINGS, ImportSettings, import_options, register_settings};
pub use watch::DiscoveryWatch;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] caskade_core::Error),
    #[error(transparent)]
    Discovery(#[from] caskade_discovery::Error),
    #[error("discovery task: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Settings(#[from] caskade_core::settings::SettingsError),
    #[error("discovery import cancelled")]
    Cancelled,
}
pub type Result<T> = std::result::Result<T, Error>;
