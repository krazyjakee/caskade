use super::{DmgError, DmgMetadata, MountedImage, Result, inspect};
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, OperatingSystem,
    PayloadPlatform, Requirements, Role, RuntimeSpec, Step, Strategy, Warning,
    exec::{ExecError, Extractor, StepContext},
    formats::archive::ExtractionControl,
    host::HostCaps,
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
    runtime::Runtime,
    safety::SafeDir,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Default)]
pub struct DmgProbe;
impl DmgProbe {
    pub fn new() -> Self {
        Self
    }
}
impl FormatProbe for DmgProbe {
    fn id(&self) -> &str {
        "macos-dmg"
    }
    fn detect(
        &self,
        source: &mut ProbeSource<'_>,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        if source.size() < 512 || source.read_at(source.size() - 512, 4)? != b"koly" {
            return Ok(None);
        }
        let mut detection = Detection::new(Format::Dmg, PayloadPlatform::Macos, true);
        detection.role = Some(Role::Installer);
        match inspect(source) {
            Ok(metadata) => {
                detection
                    .facts
                    .insert("macos.dmg".into(), serde_json::to_value(metadata).unwrap());
            }
            Err(error) => {
                detection
                    .facts
                    .insert("macos.dmg_error".into(), error.to_string().into());
            }
        }
        Ok(Some(detection))
    }
}
#[derive(Default)]
pub struct DmgHandler;
impl DmgHandler {
    pub fn new() -> Self {
        Self
    }
}
impl Handler for DmgHandler {
    fn id(&self) -> HandlerId {
        HandlerId("macos-dmg".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Dmg
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        if context.host.os != OperatingSystem::Macos
            || !context.host.packages.managers.contains_key("hdiutil")
            || !node.facts.contains_key("macos.dmg")
        {
            return Ok(Proposal::default());
        }
        let metadata: DmgMetadata = serde_json::from_value(node.facts["macos.dmg"].clone())
            .map_err(|_| PlanError::Handler {
                handler: self.id(),
                message: "invalid DMG metadata fact".into(),
            })?;
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Extract,
                confidence: 1.0,
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    executes_code: false,
                    runtime: Some(RuntimeSpec::Native),
                    may_prompt_elevation: false,
                    interactive: false,
                    est_disk_bytes: None,
                },
                steps: vec![Step::Extract {
                    from: node.id.clone(),
                    select: vec![],
                    strip: None,
                    to: ".".into(),
                }],
                warnings: if metadata.sla {
                    vec![Warning {
                        code: "dmg_license".into(),
                        message:
                            "The disk image licence agreement is accepted while mounting read-only"
                                .into(),
                        path: None,
                    }]
                } else {
                    vec![]
                },
            }],
            question: None,
        })
    }
}
pub struct DmgExtractor {
    sources: BTreeMap<ArtifactId, PathBuf>,
    host: HostCaps,
    runtime: Arc<dyn Runtime>,
    artifacts: Mutex<BTreeMap<ArtifactId, Vec<ArtifactNode>>>,
}
impl DmgExtractor {
    pub fn new(
        sources: BTreeMap<ArtifactId, PathBuf>,
        host: HostCaps,
        runtime: Arc<dyn Runtime>,
    ) -> Self {
        Self {
            sources,
            host,
            runtime,
            artifacts: Mutex::default(),
        }
    }
    pub fn take_artifacts(&self, id: &ArtifactId) -> Option<Vec<ArtifactNode>> {
        self.artifacts
            .lock()
            .expect("DMG artifacts poisoned")
            .remove(id)
    }
    pub fn runtime(&self) -> &Arc<dyn Runtime> {
        &self.runtime
    }
}
fn map_error(error: DmgError) -> ExecError {
    match error {
        DmgError::Io(error) => error.into(),
        DmgError::Control(error) => error.into(),
        DmgError::Runtime(error) => error.into(),
        DmgError::App(super::super::app::AppError::Safety(error)) => error.into(),
        other => ExecError::Extract {
            message: other.to_string(),
        },
    }
}
/// Copy only real top-level .app directories. The ubiquitous /Applications
/// symlink and decorative DMG metadata are never followed or imported.
pub fn copy_apps(
    mountpoint: &Path,
    destination: &mut SafeDir,
    control: &mut dyn ExtractionControl,
) -> Result<Vec<ArtifactNode>> {
    let root = cap_std::fs::Dir::open_ambient_dir(mountpoint, cap_std::ambient_authority())?;
    let mut entries = Vec::new();
    for entry in root.entries()? {
        if entries.len() >= 4096 {
            return Err(DmgError::Limit);
        }
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    let mut nodes = Vec::new();
    for entry in entries {
        control.checkpoint(0)?;
        let name = entry.file_name();
        let name = PathBuf::from(name);
        if !name
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        {
            continue;
        }
        let metadata = root.symlink_metadata(&name)?;
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(DmgError::Unsupported(
                "app root is not a real directory".into(),
            ));
        }
        let bundle = super::super::app::copy_bundle(&mountpoint.join(&name), destination, control)?;
        let metadata = super::super::app::installed_metadata(destination, &bundle)?;
        nodes.push(ArtifactNode {
            id: ArtifactId(format!("extracted:dmg:{}", nodes.len())),
            path: bundle,
            format: Format::AppBundle,
            platform: PayloadPlatform::Macos,
            role: Role::Application,
            size: None,
            facts: BTreeMap::from([(
                "macos.bundle".into(),
                serde_json::to_value(metadata).unwrap(),
            )]),
            children: vec![],
            children_complete: false,
        });
    }
    if nodes.is_empty() {
        return Err(DmgError::Unsupported(
            "mounted image has no top-level app bundle".into(),
        ));
    }
    Ok(nodes)
}
impl Extractor for DmgExtractor {
    fn extract(
        &self,
        artifact: &ArtifactId,
        select: &[String],
        strip: Option<&Path>,
        destination: &Path,
        context: &StepContext,
    ) -> std::result::Result<(), ExecError> {
        if self.host.os != OperatingSystem::Macos {
            return Err(ExecError::Extract {
                message: "DMG payload is unsupported off macOS".into(),
            });
        }
        if !select.is_empty() || strip.is_some() {
            return Err(ExecError::Extract {
                message: "DMG installs preserve whole app bundles".into(),
            });
        }
        let tool =
            self.host
                .packages
                .managers
                .get("hdiutil")
                .ok_or_else(|| ExecError::Extract {
                    message: "hdiutil capability is unavailable".into(),
                })?;
        let source = crate::exec::source_path(&self.sources, artifact)?;
        let input = std::fs::symlink_metadata(source)?;
        if !input.is_file() || input.is_symlink() {
            return Err(ExecError::Extract {
                message: "DMG source must be a regular file".into(),
            });
        }
        inspect(&mut std::fs::File::open(source)?).map_err(map_error)?;
        let mut control = context;
        let mounted = MountedImage::attach(
            self.runtime.clone(),
            tool,
            source,
            &mut control,
            Duration::from_secs(120),
        )
        .map_err(map_error)?;
        let mut safe = context.open_destination(destination, input.len())?;
        let copied = copy_apps(mounted.mountpoint(), &mut safe, &mut control);
        #[cfg(target_os = "macos")]
        let copied = copied.and_then(|nodes| {
            if let Some(quarantine) = xattr::get(source, "com.apple.quarantine")? {
                if quarantine.len() > 1024 * 1024 {
                    return Err(DmgError::Limit);
                }
                for node in &nodes {
                    if xattr::get(
                        mounted.mountpoint().join(&node.path),
                        "com.apple.quarantine",
                    )?
                    .is_none()
                    {
                        safe.set_entry_xattr(
                            &node.path.to_string_lossy(),
                            std::ffi::OsStr::new("com.apple.quarantine"),
                            &quarantine,
                        )
                        .map_err(super::super::app::AppError::from)?;
                    }
                }
            }
            Ok(nodes)
        });
        // Cleanup is deliberately performed before observing cancellation or
        // returning a copy/metadata error, and never consults its token.
        mounted.detach().map_err(map_error)?;
        context.checkpoint(0)?;
        let nodes = copied.map_err(map_error)?;
        self.artifacts
            .lock()
            .expect("DMG artifacts poisoned")
            .insert(artifact.clone(), nodes);
        Ok(())
    }
}
