//! Self-describing managed storage roots (#54). The root registry belongs to
//! core StorageService (#2); this source reconciles and imports its manifests.
mod identity;
mod provider;
mod remount;

use caskade_core::{bus::Bus, db::LibraryStore, model::StorageRoot};
use caskade_installer::{Question, Warning, reconcile::Reconciler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Core(#[from] caskade_core::Error),
    #[error(transparent)]
    Reconcile(#[from] caskade_installer::reconcile::ReconcileError),
    #[error(transparent)]
    Manifest(#[from] caskade_installer::manifest::ManifestError),
    #[error(transparent)]
    Executor(#[from] caskade_installer::exec::ExecError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("storage scan cancelled")]
    Cancelled,
    #[error("invalid storage adoption: {0}")]
    Invalid(String),
}
pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum StorageGameStatus {
    Unchanged,
    Reconciled,
    NeedsUser,
    Pinned,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageGameReport {
    pub relative_dir: PathBuf,
    pub status: StorageGameStatus,
    pub game_id: Option<String>,
    pub entry_id: Option<String>,
    pub question: Option<Question>,
    pub warnings: Vec<Warning>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageRootReport {
    pub root: StorageRoot,
    pub games: Vec<StorageGameReport>,
    pub warnings: Vec<Warning>,
    pub error: Option<String>,
}
impl StorageRootReport {
    pub fn imported(&self) -> usize {
        self.games.iter().filter(|g| g.entry_id.is_some()).count()
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageScanReport {
    pub roots: Vec<StorageRootReport>,
}
impl StorageScanReport {
    pub fn imported(&self) -> usize {
        self.roots.iter().map(StorageRootReport::imported).sum()
    }
}

#[derive(Clone)]
pub struct StorageSource {
    store: LibraryStore,
    bus: Bus,
    reconcile: Arc<Reconciler>,
    imports: Arc<tokio::sync::Mutex<()>>,
    remounts: Option<Arc<remount::Remounts>>,
}
impl StorageSource {
    pub fn new(store: LibraryStore, bus: Bus, reconcile: Arc<Reconciler>) -> Self {
        Self {
            store,
            bus,
            reconcile,
            imports: Arc::default(),
            remounts: None,
        }
    }
    /// Enable bounded automatic recovery when a drive letter/mount point
    /// changes. Hints live beside the root catalogue, outside SQLite. Tests
    /// inject a fixture HostEnv; this builder never discovers ambient paths.
    pub fn with_remounts(
        mut self,
        registry: caskade_core::storage::StorageService,
        env: caskade_discovery::HostEnv,
        paths: &caskade_core::AppPaths,
    ) -> Self {
        self.remounts = Some(Arc::new(remount::Remounts::new(
            registry,
            env,
            paths.config.clone(),
        )));
        self
    }

    /// Register this configured instance as a bus library provider. Call
    /// builders before registration so the provider retains remount support.
    pub fn register(&self) {
        self.bus.register_library_source(Arc::new(self.clone()));
    }
}
