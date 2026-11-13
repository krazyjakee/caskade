use super::{FlatpakMetadata, inspect_bundle, inspect_ref};
use crate::{
    ArtifactNode, Candidate, DelegateCheck, DelegateCommand, Fidelity, Format, HandOff, HandlerId,
    InstallOutcome, OperatingSystem, PayloadPlatform, Requirements, Role, RuntimeSpec, Step,
    Strategy, Warning,
};
use crate::{
    exec::source_argument,
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::path::Path;
#[derive(Default)]
pub struct FlatpakProbe;
impl FormatProbe for FlatpakProbe {
    fn id(&self) -> &str {
        "flatpak"
    }
    fn detect(
        &self,
        source: &mut ProbeSource<'_>,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        self.detect_named(source, Path::new("input"))
    }
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        let head = source.read_at(0, source.size().min(32) as usize)?;
        let metadata = if head.starts_with(b"flatpak\0") {
            Some(inspect_bundle(source))
        } else if name
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("flatpakref"))
            || head.starts_with(b"[Flatpak Ref]")
        {
            if source.size() > 64 * 1024 {
                return Err(ProbeError::Io(std::io::Error::other(
                    "flatpakref exceeds limit",
                )));
            }
            Some(inspect_ref(&source.read_at(0, source.size() as usize)?))
        } else {
            None
        };
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        let mut detection = Detection::new(Format::Flatpak, PayloadPlatform::Linux, false);
        detection.role = Some(Role::Installer);
        match metadata {
            Ok(metadata) => {
                detection.facts.insert(
                    "flatpak.metadata".into(),
                    serde_json::to_value(metadata).unwrap(),
                );
            }
            Err(error) => {
                detection
                    .facts
                    .insert("flatpak.error".into(), error.to_string().into());
            }
        }
        Ok(Some(detection))
    }
}
pub fn metadata_from_artifact(node: &ArtifactNode) -> Option<FlatpakMetadata> {
    let metadata: FlatpakMetadata =
        serde_json::from_value(node.facts.get("flatpak.metadata")?.clone()).ok()?;
    metadata.valid().then_some(metadata)
}
#[derive(Default)]
pub struct FlatpakHandler;
impl Handler for FlatpakHandler {
    fn id(&self) -> HandlerId {
        HandlerId("flatpak".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Flatpak
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let Some(mut metadata) =
            metadata_from_artifact(node).filter(|metadata| metadata.application)
        else {
            return Ok(Proposal::default());
        };
        if metadata.arch.is_empty() {
            metadata.arch = context.host.arch.clone();
        }
        if !metadata.arch.is_empty() && metadata.arch != context.host.arch {
            return Ok(Proposal::default());
        }
        let Some(tool) = context
            .host
            .packages
            .managers
            .get("flatpak")
            .filter(|_| context.host.os == OperatingSystem::Linux)
        else {
            return Ok(Proposal::default());
        };
        let _manager = tool; // Capability is injected; the runtime resolves the logical host tool.
        let tool = "flatpak".to_owned();
        Ok(Proposal {
            question: None,
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::Delegate,
                confidence: 1.0,
                fidelity: Fidelity::Native,
                requires: Requirements {
                    executes_code: true,
                    runtime: Some(RuntimeSpec::Native),
                    may_prompt_elevation: false,
                    interactive: false,
                    est_disk_bytes: None,
                },
                steps: vec![Step::Delegate {
                    tool: tool.clone(),
                    args: vec![
                        "install".into(),
                        "--user".into(),
                        "--noninteractive".into(),
                        "--".into(),
                        source_argument(&node.id),
                    ],
                    uninstall: vec![DelegateCommand {
                        tool,
                        args: metadata.uninstall_args(),
                        only_if: Some(metadata.installed()),
                    }],
                    only_if: None,
                }],
                warnings: vec![Warning {
                    code: "flatpak.pinned".into(),
                    message: "Flatpak manages this user installation outside the storage root"
                        .into(),
                    path: None,
                }],
            }],
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FlatpakLaunchSpec {
    pub tool: String,
    pub args: Vec<String>,
}
impl FlatpakMetadata {
    fn uninstall_args(&self) -> Vec<String> {
        vec![
            "uninstall".into(),
            "--user".into(),
            "--noninteractive".into(),
            "--".into(),
            if self.arch.is_empty() {
                format!("{}/{}", self.id, self.branch)
            } else {
                format!("app/{}/{}/{}", self.id, self.arch, self.branch)
            },
        ]
    }
    /// Exits 0 while the app is installed for the user. `info` reads
    /// `id//branch` where `uninstall` takes `id/branch`.
    pub fn installed(&self) -> DelegateCheck {
        DelegateCheck {
            tool: "flatpak".into(),
            args: vec![
                "info".into(),
                "--user".into(),
                "--".into(),
                if self.arch.is_empty() {
                    format!("{}//{}", self.id, self.branch)
                } else {
                    format!("app/{}/{}/{}", self.id, self.arch, self.branch)
                },
            ],
        }
    }
    pub fn launch_spec(&self) -> FlatpakLaunchSpec {
        let mut args = vec![
            "run".into(),
            "--user".into(),
            format!("--branch={}", self.branch),
        ];
        if !self.arch.is_empty() {
            args.push(format!("--arch={}", self.arch))
        }
        args.push(self.id.clone());
        FlatpakLaunchSpec {
            tool: "flatpak".into(),
            args,
        }
    }
    pub fn handoff(&self, tool: &str) -> InstallOutcome {
        InstallOutcome::HandOff(HandOff::Delegated {
            launch_uri: format!("flatpak:{}", self.id),
            uninstall_steps: vec![Step::Delegate {
                tool: tool.into(),
                args: self.uninstall_args(),
                uninstall: vec![],
                only_if: Some(self.installed()),
            }],
        })
    }
}
