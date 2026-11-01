//! Portable folder handler and application-boundary adapter. Move deliberately
//! stages a copy and retires the original only after atomic commit succeeds.
use crate::{
    ArtifactNode, Candidate, Fidelity, Format, HandlerId, Input, InstallOutcome, InstallPlan,
    Installed, PayloadPlatform, Requirements, Role, SourcePath, Step, Strategy, Warning,
    discover::{DiscoveryHints, discover},
    exec::{self, ExecRequest, Executor, FinalizationContext, Finalizer},
    host::HostCaps,
    input::{FolderKind, ResolvedInput},
    manifest::{
        self, HandlerVersion, InstallManifest, InstallRecord, InstalledFile, ManifestPath,
        ManifestSource, RelocationRecord, RootManifest, Sha256Digest,
    },
    plan::{Handler, PlanContext, PlanEngine, PlanError, Policy, Proposal},
    probe::ProbeEngine,
};
use cap_std::{ambient_authority, fs::Dir};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderSourceHandling {
    #[default]
    Copy,
    Move,
}

#[derive(Debug, Default)]
pub struct FolderHandler;
impl Handler for FolderHandler {
    fn id(&self) -> HandlerId {
        HandlerId("portable-folder".into())
    }
    fn claims(&self, artifact: &ArtifactNode) -> bool {
        artifact.format == Format::Directory
            && (artifact.role == Role::Application
                || artifact
                    .facts
                    .get("folder.portable")
                    .and_then(|v| v.as_bool())
                    == Some(true))
    }
    fn propose(&self, artifact: &ArtifactNode, _: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        if !self.claims(artifact) {
            return Ok(Proposal::default());
        }
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Copy,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    est_disk_bytes: artifact.size,
                    ..Default::default()
                },
                steps: vec![Step::Copy {
                    from: SourcePath::Artifact {
                        artifact: artifact.id.clone(),
                        path: ".".into(),
                    },
                    to: ".".into(),
                }],
                warnings: vec![],
            }],
            question: None,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PortableError {
    #[error("portable input is invalid: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Execution(#[from] exec::ExecError),
    #[error(transparent)]
    Manifest(#[from] manifest::ManifestError),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error("portable worker failed: {0}")]
    Worker(String),
}
pub struct FolderInstallRequest {
    pub job_id: String,
    pub source: PathBuf,
    pub root: PathBuf,
    pub root_id: String,
    pub relative_dir: PathBuf,
    pub host: HostCaps,
    pub hints: DiscoveryHints,
    pub source_handling: FolderSourceHandling,
}

/// Uses the real planner, confined executor, discovery and manifest writer.
/// A failed/cancelled attempt retains the source. A failed Move cleanup returns
/// Installed with a warning, because the managed installation already committed.
pub async fn install_folder(
    executor: Executor,
    request: FolderInstallRequest,
    cancellation: CancellationToken,
) -> Result<InstallOutcome, PortableError> {
    if cancellation.is_cancelled() {
        return Err(exec::ExecError::Cancelled.into());
    }
    let prepared = caskade_jobs::blocking("Preparing portable game", move || prepare(request))
        .await
        .map_err(|e| PortableError::Worker(e.to_string()))??;
    let (request, artifact, before) = prepared;
    let mut planner = PlanEngine::new();
    planner.register(Box::new(FolderHandler))?;
    let plans = planner.plan(
        std::slice::from_ref(&artifact),
        &request.host,
        &Policy::default(),
    )?;
    let finalizer = Arc::new(FolderFinalizer {
        artifact: artifact.clone(),
        input: Input::Path(request.source.clone()),
        root_id: request.root_id.clone(),
        host: request.host,
        hints: request.hints,
        warnings: Mutex::new(vec![]),
    });
    let report = executor
        .with_finalizer(finalizer.clone())
        .execute(
            ExecRequest {
                job_id: request.job_id,
                root: request.root,
                relative_dir: request.relative_dir,
                plans,
                sources: BTreeMap::from([(artifact.id, request.source.clone())]),
            },
            cancellation,
        )
        .await?;
    let dir = report.dir.clone();
    let manifest =
        caskade_jobs::blocking("Reading game manifest", move || InstallManifest::read(&dir))
            .await
            .map_err(|e| PortableError::Worker(e.to_string()))??;
    let record = manifest.record();
    let mut installed = Installed {
        root_id: record.plan.root_id.clone(),
        dir: report.relative_dir,
        title: record.title.clone(),
        ids: record.ids.clone(),
        launch_targets: record.launch_targets.clone(),
        runtime: record.runtime.clone(),
        dependencies: vec![],
        warnings: {
            let mut warnings = report.candidate.warnings;
            warnings.extend(
                finalizer
                    .warnings
                    .lock()
                    .map_err(|_| PortableError::Invalid("finalizer lock poisoned".into()))?
                    .clone(),
            );
            warnings
        },
        manifest: manifest::INSTALL_MANIFEST_PATH.into(),
    };
    if let Some(before) = before {
        let source = request.source;
        let cleanup = caskade_jobs::blocking("Cleaning install files", move || {
            // Reject source changes during copying before touching the original.
            if snapshot(&source)? != before {
                return Err(io::Error::other(
                    "source changed during installation; retained original",
                ));
            }
            let parent = source
                .parent()
                .ok_or_else(|| io::Error::other("source has no parent"))?;
            let parent = Dir::open_ambient_dir(parent, ambient_authority())?;
            parent.remove_dir_all(
                source
                    .file_name()
                    .ok_or_else(|| io::Error::other("source has no filename"))?,
            )
        })
        .await
        .map_err(|e| PortableError::Worker(e.to_string()))?;
        if let Err(error) = cleanup {
            installed.warnings.push(Warning {
                code: "source_retained".into(),
                message: error.to_string(),
                path: None,
            });
        }
    }
    // An ambiguous launch selection is still recoverable from the durable
    // manifest. Do not delete the source for an installation lacking a target.
    Ok(InstallOutcome::Installed(installed))
}

type Snapshot = BTreeMap<PathBuf, String>;
fn prepare(
    mut request: FolderInstallRequest,
) -> Result<(FolderInstallRequest, ArtifactNode, Option<Snapshot>), PortableError> {
    request.source = fs::canonicalize(&request.source)?;
    request.root = fs::canonicalize(&request.root)?;
    if request.root.starts_with(&request.source) || request.source.starts_with(&request.root) {
        return Err(PortableError::Invalid(
            "source and managed root must be disjoint".into(),
        ));
    }
    if request.source.join(".caskade").exists() {
        return Err(PortableError::Invalid(
            "source already contains managed metadata".into(),
        ));
    }
    match crate::input::resolve_path(&request.source)
        .map_err(|e| PortableError::Invalid(e.to_string()))?
    {
        ResolvedInput::Folder {
            classification: FolderKind::PortableGame,
            ..
        } => {}
        _ => {
            return Err(PortableError::Invalid(
                "input is not a portable game folder".into(),
            ));
        }
    }
    let mut artifact = ProbeEngine::default()
        .probe_path(&request.source)
        .map_err(|e| PortableError::Invalid(e.to_string()))?
        .root;
    artifact.role = Role::Application;
    artifact.platform = PayloadPlatform::Unknown;
    artifact.facts.insert("folder.portable".into(), true.into());
    // Respect an existing root identity; never silently replace another one.
    if request.root.join(manifest::ROOT_MANIFEST_PATH).exists() {
        if RootManifest::read(&request.root)?.root_id() != request.root_id {
            return Err(PortableError::Invalid(
                "managed root identity mismatch".into(),
            ));
        }
    } else {
        RootManifest::new(&request.root_id)?;
    }
    let before = if request.source_handling == FolderSourceHandling::Move {
        Some(snapshot(&request.source)?)
    } else {
        None
    };
    Ok((request, artifact, before))
}

struct FolderFinalizer {
    artifact: ArtifactNode,
    input: Input,
    root_id: String,
    host: HostCaps,
    hints: DiscoveryHints,
    warnings: Mutex<Vec<Warning>>,
}
impl Finalizer for FolderFinalizer {
    fn finalize(&self, context: &FinalizationContext<'_>) -> exec::Result<()> {
        context.steps.checkpoint(0)?;
        // The executor holds the managed-root lock during finalization.
        let root = context
            .final_dir
            .parent()
            .ok_or_else(|| invalid("missing managed root"))?;
        if root.join(manifest::ROOT_MANIFEST_PATH).exists() {
            if RootManifest::read(root).map_err(invalid)?.root_id() != self.root_id {
                return Err(invalid("managed root identity mismatch"));
            }
        } else {
            RootManifest::new(&self.root_id)
                .map_err(invalid)?
                .write(root)
                .map_err(invalid)?;
        }
        let mut hints = self.hints.clone();
        if hints.source_name.is_none() {
            hints.source_name = self.artifact.path.to_str().map(str::to_owned);
        }
        let found = discover(context.game_dir, &self.host, &hints).map_err(invalid)?;
        if found.launch_targets.is_empty() || found.question.is_some() {
            return Err(invalid("portable launch target is missing or ambiguous"));
        }
        let mut files = vec![];
        inventory(
            &Dir::open_ambient_dir(context.game_dir, ambient_authority())?,
            Path::new(""),
            &mut files,
            &mut BTreeMap::new(),
            &mut 0,
            Some(context.steps),
        )
        .map_err(invalid)?;
        *self
            .warnings
            .lock()
            .map_err(|_| invalid("finalizer lock poisoned"))? = found.warnings;
        let runtime = found.launch_targets[0].runtime.clone();
        InstallManifest::new(InstallRecord {
            title: found.title,
            ids: found.ids,
            source: ManifestSource::from_input(&self.input, None, None).map_err(invalid)?,
            plan: InstallPlan {
                schema_version: 1,
                input: self.input.clone(),
                artifacts: vec![self.artifact.clone()],
                root_id: self.root_id.clone(),
                relative_dir: context.relative_dir.into(),
                candidate: context.candidate.clone(),
            },
            handlers: vec![HandlerVersion {
                id: context.candidate.handler.clone(),
                version: env!("CARGO_PKG_VERSION").into(),
            }],
            files,
            launch_targets: found.launch_targets,
            alias: context.alias.map(Into::into),
            runtime,
            uninstall_steps: vec![],
            external_effects: vec![],
            relocation: {
                // Scan the staged tree for the final path the game will have.
                let scan = crate::scan::scan_references(&crate::scan::ScanInput {
                    game_dir: context.game_dir,
                    real_path: &context.final_dir.to_string_lossy(),
                    alias_path: None,
                    real_variants: &[],
                    registry: &[],
                    external_files: &[],
                    delegated: false,
                    limits: crate::scan::ScanLimits::default(),
                })
                .map_err(invalid)?;
                RelocationRecord {
                    verdict: scan.verdict,
                    references: scan.references,
                    last_path: Some(context.final_dir.to_string_lossy().into_owned()),
                }
            },
        })
        .map_err(invalid)?
        .write(context.game_dir)
        .map_err(invalid)?;
        context.steps.checkpoint(0)
    }
}
fn invalid(error: impl std::fmt::Display) -> exec::ExecError {
    exec::ExecError::InvalidRequest {
        message: error.to_string(),
    }
}
fn snapshot(path: &Path) -> io::Result<Snapshot> {
    let mut stamps = BTreeMap::new();
    inventory(
        &Dir::open_ambient_dir(path, ambient_authority())?,
        Path::new(""),
        &mut vec![],
        &mut stamps,
        &mut 0,
        None,
    )?;
    Ok(stamps)
}
fn inventory(
    dir: &Dir,
    prefix: &Path,
    files: &mut Vec<InstalledFile>,
    stamps: &mut Snapshot,
    count: &mut u64,
    steps: Option<&exec::StepContext>,
) -> io::Result<()> {
    if prefix.components().count() > 64 {
        return Err(io::Error::other("portable inventory depth limit"));
    }
    let remaining = 100_001_u64.saturating_sub(*count) as usize;
    let mut entries = dir
        .entries()?
        .take(remaining)
        .collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        *count += 1;
        if *count > 100_000 {
            return Err(io::Error::other("portable inventory entry limit"));
        }
        if let Some(steps) = steps {
            steps.checkpoint(0).map_err(io::Error::other)?;
        }
        let name = entry.file_name();
        let path = prefix.join(&name);
        let meta = dir.symlink_metadata(&name)?;
        if meta.is_symlink() {
            stamps.insert(path, format!("link:{:?}", dir.read_link(&name)?));
        } else if meta.is_dir() {
            stamps.insert(path.clone(), "directory".into());
            inventory(&dir.open_dir(&name)?, &path, files, stamps, count, steps)?;
        } else if meta.is_file() {
            let mut file = dir.open(&name)?;
            let mut hasher = Sha256::new();
            let mut buffer = [0; 64 * 1024];
            let mut size = 0;
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                if let Some(steps) = steps {
                    steps.checkpoint(0).map_err(io::Error::other)?;
                }
                size += n as u64;
                hasher.update(&buffer[..n]);
            }
            let digest = format!("{:x}", hasher.finalize());
            stamps.insert(path.clone(), format!("file:{size}:{digest}"));
            let path = path
                .to_str()
                .ok_or_else(|| io::Error::other("non-Unicode portable path"))?;
            files.push(InstalledFile {
                path: ManifestPath::new(path).map_err(io::Error::other)?,
                size,
                sha256: Some(Sha256Digest::new(digest).map_err(io::Error::other)?),
            });
        } else {
            return Err(io::Error::other("special file in portable inventory"));
        }
    }
    Ok(())
}
