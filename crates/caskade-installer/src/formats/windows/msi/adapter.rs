use super::*;
use crate::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, PayloadPlatform, Program,
    Requirements, Role, RuntimeSpec, Step, Strategy, Warning,
    plan::{
        Handler, PlanContext, PlanError, Proposal,
        presets::{InstallerFamily, InstallerMode, SwitchSettings, build_args},
    },
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::{
    collections::BTreeMap,
    io::{Seek, SeekFrom},
    path::PathBuf,
};

#[derive(Default)]
pub struct MsiProbe;
impl MsiProbe {
    pub fn new() -> Self {
        Self
    }
}
impl FormatProbe for MsiProbe {
    fn id(&self) -> &str {
        "msi"
    }
    fn detect(
        &self,
        source: &mut ProbeSource<'_>,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        if source.read_at(0, 8)? != b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" {
            return Ok(None);
        }
        source.seek(SeekFrom::Start(0))?;
        let metadata =
            inspect_reader(source, MsiLimits::default()).map_err(std::io::Error::other)?;
        let mut detection = Detection::new(Format::Msi, PayloadPlatform::Windows, true);
        detection.role = Some(Role::Installer);
        if let Some(title) = metadata.properties.get("ProductName") {
            detection.facts.insert("title".into(), title.clone().into());
        }
        detection.facts.insert(
            "msi.metadata".into(),
            serde_json::to_value(metadata).expect("MSI metadata serializes"),
        );
        Ok(Some(detection))
    }
    fn list(
        &self,
        _source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> std::result::Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if node.format != Format::Msi {
            return Ok(None);
        }
        let Some(metadata) = metadata_from_artifact(node) else {
            return Ok(None);
        };
        Ok(Some(
            metadata
                .files
                .iter()
                .enumerate()
                .map(|(i, file)| ArtifactNode {
                    id: ArtifactId(format!("{}:msi:{i}", node.id.0)),
                    path: file.path.clone(),
                    format: Format::Unknown,
                    platform: PayloadPlatform::Windows,
                    role: Role::Data,
                    size: Some(file.size),
                    facts: BTreeMap::new(),
                    children: vec![],
                    children_complete: true,
                })
                .collect(),
        ))
    }
}
pub fn metadata_from_artifact(node: &ArtifactNode) -> Option<MsiMetadata> {
    serde_json::from_value(node.facts.get("msi.metadata")?.clone()).ok()
}

#[derive(Default)]
pub struct MsiHandler;
impl MsiHandler {
    pub fn new() -> Self {
        Self
    }
}
impl Handler for MsiHandler {
    fn id(&self) -> HandlerId {
        HandlerId("msi".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Msi
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let metadata = metadata_from_artifact(node);
        let partial = metadata.as_ref().is_none_or(MsiMetadata::is_partial);
        let estimate = metadata.as_ref().and_then(MsiMetadata::unpacked_bytes);
        let mut warnings = vec![];
        if partial {
            warnings.push(Warning { code: "msi.partial".into(), message: "Table extraction omits custom actions, external shortcuts and files outside the application directory".into(), path: None });
        }
        let mut candidates = vec![Candidate {
            handler: self.id(),
            strategy: Strategy::Extract,
            confidence: if metadata.is_some() { 1.0 } else { 0.8 },
            fidelity: if partial {
                Fidelity::Partial
            } else {
                Fidelity::Exact
            },
            requires: Requirements {
                est_disk_bytes: estimate,
                ..Default::default()
            },
            steps: vec![Step::Extract {
                from: node.id.clone(),
                select: vec![],
                strip: None,
                to: PathBuf::from("."),
            }],
            warnings,
        }];
        if let Some(runtime) = super::super::installer_runtime(context.host) {
            for (mode, strategy) in [
                (InstallerMode::Silent, Strategy::RunSilent),
                (InstallerMode::Interactive, Strategy::RunInteractive),
            ] {
                let preset = build_args(
                    InstallerFamily::Msi,
                    mode,
                    &SwitchSettings::from(&context.policy.preset_args),
                )
                .map_err(|e| PlanError::Handler {
                    handler: self.id(),
                    message: e.to_string(),
                })?;
                let mut args = vec!["/i".into(), crate::exec::source_argument(&node.id)];
                args.extend(preset.args);
                let program = Program::Tool("msiexec".into());
                let step = if mode == InstallerMode::Silent {
                    Step::Run {
                        program,
                        args,
                        env: BTreeMap::new(),
                        runtime: runtime.clone(),
                        timeout_secs: 900,
                        expect: metadata
                            .as_ref()
                            .into_iter()
                            .flat_map(|m| m.files.first())
                            .map(|f| f.path.clone())
                            .collect(),
                    }
                } else {
                    Step::RunInteractive {
                        program,
                        args,
                        env: BTreeMap::new(),
                        runtime: runtime.clone(),
                        target_dir: ".".into(),
                    }
                };
                candidates.push(Candidate {
                    handler: self.id(),
                    strategy,
                    confidence: 1.0,
                    fidelity: Fidelity::Native,
                    requires: Requirements {
                        executes_code: true,
                        runtime: Some(runtime.clone()),
                        may_prompt_elevation: true,
                        interactive: mode == InstallerMode::Interactive,
                        est_disk_bytes: estimate,
                    },
                    steps: vec![step],
                    warnings: preset
                        .warnings
                        .into_iter()
                        .map(|message| Warning {
                            code: "msi.preset".into(),
                            message,
                            path: None,
                        })
                        .collect(),
                });
            }
        }
        Ok(Proposal {
            candidates,
            question: None,
        })
    }
}
/// Call only for a selected native/prefix install, never for table extraction.
pub fn uninstall_steps(node: &ArtifactNode) -> Vec<Step> {
    let Some(metadata) = metadata_from_artifact(node) else {
        return vec![];
    };
    let Some(code) = metadata
        .properties
        .get("ProductCode")
        .filter(|code| metadata::product_code(code))
    else {
        return vec![];
    };
    vec![Step::Run {
        program: Program::Tool("msiexec".into()),
        args: vec!["/x".into(), code.clone(), "/qn".into(), "/norestart".into()],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 900,
        expect: vec![],
    }]
}
