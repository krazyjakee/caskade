//! Repair externally moved game folders without moving any files. Storage
//! integration (#54) calls [`Reconciler::scan_root`] after resolving a root ID.
//! Manifest location anchors are provenance, never filesystem capabilities.

mod engine;
mod references;
mod scan;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
    Question, Warning,
    exec::ExecError,
    manifest::{InstallManifest, ManifestError},
    relocate::{RelocateError, Relocator},
    runtime::alias::AliasStore,
};

#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    #[error("reconciliation cancelled")]
    Cancelled,
    #[error("invalid reconcile request: {0}")]
    Invalid(String),
    #[error("root scan exceeds its {0}-entry limit")]
    Limit(usize),
    #[error("game {0:?} has an alias but no alias store was supplied")]
    MissingAliases(PathBuf),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Repair(#[from] RelocateError),
    #[error(transparent)]
    Scan(#[from] crate::scan::ScanError),
    #[error(transparent)]
    Executor(#[from] ExecError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ReconcileError>;

/// An explicit caller-selected current location. `old_dir`, when supplied,
/// overrides a legacy manifest's missing location anchor; it is only text to
/// search for, never a directory to move, write into, or delete.
#[derive(Debug, Clone)]
pub struct ReconcileRequest {
    pub root: PathBuf,
    pub relative_dir: PathBuf,
    pub old_dir: Option<String>,
    pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileStatus {
    Unchanged,
    Reconciled,
    NeedsUser,
    Pinned,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameReport {
    pub relative_dir: PathBuf,
    pub status: ReconcileStatus,
    pub old_dir: Option<String>,
    pub rewritten_files: usize,
    pub rewritten_values: usize,
    pub question: Option<Question>,
    pub warnings: Vec<Warning>,
    pub error: Option<String>,
    pub manifest: Option<InstallManifest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootReport {
    pub root: PathBuf,
    pub root_id: String,
    pub games: Vec<GameReport>,
    /// Unmanaged folders, symlinks and folders below the configured scan depth
    /// are not adopted or modified.
    pub warnings: Vec<Warning>,
}

/// A web game's offline copy (`caskade_core::web::bundle`): the scan skips
/// the folder holding it, which can have thousands of files.
pub const WEB_BUNDLE_MANIFEST: &str = "caskade-web.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    pub max_entries: usize,
    pub max_depth: usize,
}
impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_depth: 4,
        }
    }
}

/// Validated reference evidence for a managed move. The caller must keep its
/// storage-root lease from preparation through the rewrite transaction.
#[derive(Debug, Clone)]
pub struct PreparedReferences {
    pub references: Vec<crate::scan::Reference>,
    pub verdict: crate::manifest::RelocationVerdict,
    pub skipped: Vec<String>,
}

/// The rewrite/journal implementation is shared with managed relocation (#60).
/// The caller supplies registry, shortcut and process-observation capabilities
/// through `Relocator`; reconciliation never discovers ambient registry scopes.
#[derive(Clone)]
pub struct Reconciler {
    repair: Arc<Relocator>,
    factory: Option<Arc<dyn RepairFactory>>,
    aliases: Option<AliasStore>,
    external_roots: Vec<PathBuf>,
    limits: ScanLimits,
}

impl Reconciler {
    pub fn new(repair: Arc<Relocator>) -> Self {
        Self {
            repair,
            factory: None,
            aliases: None,
            external_roots: vec![],
            limits: ScanLimits::default(),
        }
    }
    /// Select per-game prefix/registry capabilities while scanning a root
    /// containing games that use different runtimes. The fixed engine supplied
    /// to `new` remains the default.
    pub fn with_repair_factory(mut self, factory: Arc<dyn RepairFactory>) -> Self {
        self.factory = Some(factory);
        self
    }
    pub fn with_aliases(mut self, aliases: AliasStore) -> Self {
        self.aliases = Some(aliases);
        self
    }
    /// Only these explicitly selected host directories may contain rewritten
    /// external files. Manifest reference strings cannot grant this authority.
    pub fn with_external_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.external_roots = roots;
        self
    }
    /// Scan the current game plus explicitly granted external/registry scopes
    /// before a physical relocation. `old_dir` is search text, never authority
    /// to write into that directory. This synchronous operation belongs inside
    /// the caller's root lease and blocking worker.
    pub fn prepare_references(
        &self,
        game: &Path,
        old_dir: &str,
        manifest: &InstallManifest,
    ) -> Result<PreparedReferences> {
        let game = dunce::canonicalize(game)?;
        let repair = match &self.factory {
            Some(factory) => factory.for_game(&game, manifest)?,
            None => self.repair.clone(),
        };
        let (references, verdict, skipped) = self.rescan(&game, old_dir, manifest, &repair)?;
        Ok(PreparedReferences {
            references,
            verdict,
            skipped,
        })
    }

    pub fn with_limits(mut self, limits: ScanLimits) -> Self {
        self.limits = limits;
        self
    }
}

pub trait RepairFactory: Send + Sync {
    fn for_game(&self, game: &Path, manifest: &InstallManifest) -> Result<Arc<Relocator>>;
}

impl<F> RepairFactory for F
where
    F: Fn(&Path, &InstallManifest) -> Result<Arc<Relocator>> + Send + Sync,
{
    fn for_game(&self, game: &Path, manifest: &InstallManifest) -> Result<Arc<Relocator>> {
        self(game, manifest)
    }
}
