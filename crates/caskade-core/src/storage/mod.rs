//! Registered storage roots, measured capabilities and managed moves (#2).
mod catalogue;
mod moving;
mod probe;
mod service;
#[cfg(test)]
mod tests;
use crate::model::StorageRoot;
pub use probe::{FileSystemProbe, RootProbe};
use serde::{Deserialize, Serialize};
pub use service::StorageService;
use specta::Type;
use std::path::PathBuf;
pub use tokio_util::sync::CancellationToken;
pub type Result<T> = std::result::Result<T, StorageError>;
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Executor(#[from] caskade_installer::exec::ExecError),
    #[error(transparent)]
    Store(#[from] crate::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Settings(#[from] crate::settings::SettingsError),
    #[error(transparent)]
    Manifest(#[from] caskade_installer::manifest::ManifestError),
    #[error("storage operation: {0}")]
    Invalid(String),
    #[error("storage root is offline or its identity changed")]
    Offline,
    #[error("storage root is not writable")]
    ReadOnly,
    #[error("select another default location before removing this one")]
    DefaultRoot,
    #[error("the game is running or another operation holds its lease")]
    Running,
    #[error("storage worker stopped: {0}")]
    Worker(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RootStatus {
    Online,
    Offline,
    ReadOnly,
    IdentityMismatch,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootCapabilities {
    pub writable: bool,
    pub executable: bool,
    pub symlinks: bool,
    pub case_sensitive: bool,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub filesystem: Option<String>,
    pub volume: Option<String>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageGame {
    pub entry_id: String,
    pub game_id: String,
    pub title: String,
    pub relative_dir: PathBuf,
    pub size_bytes: u64,
    pub available: bool,
    pub move_blocked: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootSummary {
    pub root: StorageRoot,
    pub status: RootStatus,
    pub capabilities: RootCapabilities,
    pub folder_template: String,
    pub games: Vec<StorageGame>,
    pub games_bytes: u64,
    pub other_bytes: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MoveReport {
    pub entry_id: String,
    pub game_id: String,
    pub root_id: String,
    pub directory: PathBuf,
    pub copied: bool,
    pub warnings: Vec<String>,
}
