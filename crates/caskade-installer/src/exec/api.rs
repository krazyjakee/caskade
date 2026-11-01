use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    ArtifactId, Candidate, PlanSet, RuntimeSpec,
    runtime::{ProcessCommand, Runtime},
    safety,
};

use super::{JournalHandle, StepContext};

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("installation needs a decision: {question:?}")]
    NeedsUser { question: crate::Question },
    #[error("installation cancelled")]
    Cancelled,
    #[error("a plan choice is required before execution")]
    DecisionRequired,
    #[error("invalid execution request: {message}")]
    InvalidRequest { message: String },
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsafe plan path or entry: {0}")]
    Safety(#[from] safety::SafetyError),
    #[error("journal encoding failed: {0}")]
    Journal(#[from] serde_json::Error),
    #[error("only {available} bytes free; {required} required")]
    DiskFull { available: u64, required: u64 },
    #[error("artifact {artifact:?} has no resolved source")]
    MissingSource { artifact: ArtifactId },
    #[error("no registered runtime for {runtime:?}")]
    MissingRuntime { runtime: RuntimeSpec },
    #[error("runtime failed: {0}")]
    Runtime(#[from] crate::runtime::RuntimeError),
    #[error("process exited with code {code}")]
    ProcessFailed { code: i32 },
    #[error("expected output is missing: {path:?}")]
    MissingExpected { path: PathBuf },
    #[error("no extractor is registered for {artifact:?}")]
    MissingExtractor { artifact: ArtifactId },
    #[error("extraction failed: {message}")]
    Extract { message: String },
    #[error("unsupported execution step: {step}")]
    UnsupportedStep { step: String },
    #[error("every candidate failed: {failures:?}")]
    AllCandidatesFailed { failures: Vec<AttemptFailure> },
    #[error("rollback failed: {message}")]
    Rollback { message: String },
    #[error("blocking worker failed: {message}")]
    Worker { message: String },
}

impl ExecError {
    /// A rollback that failed, naming the failure that started it.
    pub(super) fn after(self, cause: &ExecError) -> ExecError {
        let message = match self {
            ExecError::Rollback { message } => message,
            other => other.to_string(),
        };
        ExecError::Rollback {
            message: format!("{message}; rolling back after: {cause}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, ExecError>;

/// Trusted discovery/manifest adapter, invoked on the blocking worker after
/// steps succeed and before managed staging is committed atomically.
pub trait Finalizer: Send + Sync {
    fn finalize(&self, context: &FinalizationContext<'_>) -> Result<()>;
}

pub struct FinalizationContext<'a> {
    pub game_dir: &'a Path,
    pub final_dir: &'a Path,
    pub relative_dir: &'a Path,
    pub candidate: &'a Candidate,
    pub steps: &'a StepContext,
    /// Alias ID the game will be reachable through after commit (#31).
    pub alias: Option<&'a str>,
    /// Durable journal entries so far (out-of-root effects, compensations),
    /// so a finalizer can record them in the manifest before commit.
    pub journal: &'a [super::JournalEntry],
}

/// The app resolves root IDs and source artifacts before crossing this boundary.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub job_id: String,
    pub root: PathBuf,
    pub relative_dir: PathBuf,
    pub plans: PlanSet,
    pub sources: BTreeMap<ArtifactId, PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ExecConfig {
    pub interactive: super::interactive::InteractiveConfig,
    pub limits: safety::Limits,
    pub case_sensitive: bool,
    pub disk_margin_bytes: u64,
    pub interactive_timeout: Duration,
    pub delegate_timeout: Duration,
    /// The job's execution policy. Unless it is `Allow`, a failed strategy
    /// that runs no installer code never falls back to one that does: the
    /// job asks first (CSK-01).
    pub execution: crate::plan::ExecutionPolicy,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            interactive: super::interactive::InteractiveConfig::default(),
            limits: safety::Limits::default(),
            case_sensitive: true,
            disk_margin_bytes: 16 * 1024 * 1024,
            interactive_timeout: Duration::from_secs(3600),
            delegate_timeout: Duration::from_secs(3600),
            execution: crate::plan::ExecutionPolicy::Allow,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptFailure {
    pub candidate: u32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ExecReport {
    pub dir: PathBuf,
    pub relative_dir: PathBuf,
    pub candidate_index: u32,
    pub candidate: Candidate,
    pub failures: Vec<AttemptFailure>,
    /// Retained for the manifest even after successful journal cleanup.
    pub journal: Vec<super::JournalEntry>,
    /// Stable alias ID pointing at `dir`, when aliases are enabled (#31).
    pub alias: Option<String>,
}

/// Synchronous format seam called on the executor's blocking worker. Use the
/// context's confined writer; check cancellation and space while streaming.
pub trait Extractor: Send + Sync {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> Result<()>;
}

pub(super) struct NoExtractor;
impl Extractor for NoExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        _: &[String],
        _: Option<&Path>,
        _: &Path,
        _: &StepContext,
    ) -> Result<()> {
        Err(ExecError::MissingExtractor {
            artifact: artifact.clone(),
        })
    }
}

/// Native runtime watchers (#25) record effects as they happen, and can register
/// a native uninstaller as a compensation. The journal is durable before the
/// call returns. `finish` is called after success, failure, timeout or cancellation.
pub trait ProcessObserver: Send + Sync {
    fn start(
        &self,
        command: &ProcessCommand,
        journal: &JournalHandle,
        context: &StepContext,
    ) -> Result<Box<dyn ProcessObservation>>;
}

pub trait ProcessObservation: Send {
    fn finish(self: Box<Self>, journal: &JournalHandle) -> Result<()>;
}

pub(super) struct NoObserver;
impl ProcessObserver for NoObserver {
    fn start(
        &self,
        _: &ProcessCommand,
        _: &JournalHandle,
        _: &StepContext,
    ) -> Result<Box<dyn ProcessObservation>> {
        Ok(Box::new(NoObserver))
    }
}
impl ProcessObservation for NoObserver {
    fn finish(self: Box<Self>, _: &JournalHandle) -> Result<()> {
        Ok(())
    }
}

/// Runtime selection is explicit; prefix providers can register their adapter.
pub(super) type Runtimes = Vec<(RuntimeSpec, Arc<dyn Runtime>)>;

/// The input file or folder an extractor was given for `artifact`.
pub(crate) fn source_path<'a>(
    sources: &'a BTreeMap<ArtifactId, PathBuf>,
    artifact: &ArtifactId,
) -> Result<&'a PathBuf> {
    sources
        .get(artifact)
        .ok_or_else(|| ExecError::MissingSource {
            artifact: artifact.clone(),
        })
}

/// Render a single safe folder component. Unknown placeholders are rejected.
pub fn folder_name(template: &str, title: &str, id: &str) -> Result<PathBuf> {
    let name = template.replace("{title}", title).replace("{id}", id);
    if name.contains(['{', '}']) {
        return Err(ExecError::InvalidRequest {
            message: "unknown folder template placeholder".into(),
        });
    }
    one_component(Path::new(&name))
}

pub(super) fn one_component(path: &Path) -> Result<PathBuf> {
    let name = path.to_str().ok_or_else(|| ExecError::InvalidRequest {
        message: "non-UTF8 managed folder".into(),
    })?;
    let safe = safety::sanitize_path(name)?;
    if safe.components().count() != 1 || name.eq_ignore_ascii_case(".caskade") || safe != path {
        return Err(ExecError::InvalidRequest {
            message: "expected one safe folder component outside .caskade".into(),
        });
    }
    Ok(safe)
}
