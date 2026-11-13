use super::*;
use crate::{
    ArtifactId, ArtifactNode, Format, HandlerId, PayloadPlatform, Program, Role, RuntimeSpec,
    SourcePath, Step, Warning,
    plan::{
        Handler, PlanContext, PlanError, Proposal,
        presets::{InstallerFamily, InstallerMode, SwitchSettings, build_args},
    },
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::{
    collections::BTreeMap,
    io::{Seek, SeekFrom},
};

#[derive(Default)]
pub struct NsisProbe;
impl FormatProbe for NsisProbe {
    fn id(&self) -> &str {
        "nsis"
    }
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        name: &std::path::Path,
    ) -> std::result::Result<Option<Detection>, ProbeError> {
        if source.read_at(0, 2)? != b"MZ" {
            return Ok(None);
        }
        let Some(mut detection) = crate::probe::PeProbe::new().detect_named(source, name)? else {
            return Ok(None);
        };
        if detection.format != Format::Nsis {
            return Ok(None);
        }
        if detection.role != Some(Role::Uninstaller) {
            detection.role = Some(Role::Installer);
        }
        detection.container = true;
        // Header-only PE identification remains available for large installers.
        // Upstream NSIS uses a whole-input slice and bounded solid cache, so the
        // eager metadata path has an additional 4 MiB shared-probe I/O ceiling.
        if source.size() > 4 * 1024 * 1024 {
            detection
                .facts
                .insert("nsis.analysis_deferred".into(), true.into());
            return Ok(Some(detection));
        }
        source.seek(SeekFrom::Start(0))?;
        let bytes = source.read_at(0, source.size() as usize)?;
        match inspect_bytes(&bytes, NsisLimits::default()) {
            Ok(metadata) => {
                if metadata.uninstaller {
                    detection.role = Some(Role::Uninstaller);
                }
                detection.facts.insert(
                    "nsis.metadata".into(),
                    serde_json::to_value(metadata).expect("NSIS metadata serializes"),
                );
            }
            Err(error) => {
                detection
                    .facts
                    .insert("nsis.analysis_error".into(), error.to_string().into());
            }
        }
        Ok(Some(detection))
    }
    fn list(
        &self,
        _source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> std::result::Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if node.format != Format::Nsis {
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
                .filter_map(|(index, file)| {
                    Some(ArtifactNode {
                        id: ArtifactId(format!("{}:nsis:{index}", node.id.0)),
                        path: file.path.clone()?,
                        format: Format::Unknown,
                        platform: PayloadPlatform::Windows,
                        role: Role::Data,
                        size: file.size,
                        facts: BTreeMap::new(),
                        children: vec![],
                        children_complete: true,
                    })
                })
                .collect(),
        ))
    }
}

pub fn metadata_from_artifact(node: &ArtifactNode) -> Option<NsisMetadata> {
    serde_json::from_value(node.facts.get("nsis.metadata")?.clone()).ok()
}

#[derive(Default)]
pub struct NsisHandler;
impl Handler for NsisHandler {
    fn id(&self) -> HandlerId {
        HandlerId("nsis".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Nsis && node.role != Role::Uninstaller
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        let metadata = metadata_from_artifact(node);
        let partial = metadata.as_ref().is_none_or(NsisMetadata::is_partial).then(|| Warning { code: "nsis.partial".into(), message: "Static extraction suppresses shortcuts and omits registry, INI, plugin and launch actions; unresolved external destinations are omitted".into(), path: None });
        let estimate = metadata.as_ref().and_then(NsisMetadata::unpacked_bytes);
        let mut candidates = vec![super::super::extract_candidate(
            self.id(),
            node,
            metadata.is_some(),
            estimate,
            partial,
        )];
        if let Some(runtime) = super::super::installer_runtime(context.host) {
            let settings = SwitchSettings::from(&context.policy.preset_args);
            let run = super::super::Run {
                handler: self.id(),
                program: Program::Source(SourcePath::Artifact {
                    artifact: node.id.clone(),
                    path: ".".into(),
                }),
                leading_args: vec![],
                timeout_secs: 900,
                expect: metadata
                    .as_ref()
                    .into_iter()
                    .flat_map(|m| m.files.first())
                    .filter_map(|f| f.path.clone())
                    .collect(),
                may_prompt_elevation: true,
                est_disk_bytes: estimate,
                confidence: 1.0,
                warning_code: "nsis.preset",
            };
            candidates.extend(super::super::run_candidates(
                run,
                runtime,
                &[InstallerMode::Silent, InstallerMode::Interactive],
                |mode| build_args(InstallerFamily::Nsis, mode, &settings),
            )?);
        }
        Ok(Proposal {
            candidates,
            question: None,
        })
    }
}

/// NSIS documents _?= as a final unquoted argument that prevents the temporary
/// self-copy, allowing the executor to wait for this actual process:
/// https://nsis.sourceforge.io/Docs/Chapter3.html#installerusage
pub fn uninstall_steps(node: &ArtifactNode) -> Vec<Step> {
    let Some(metadata) = metadata_from_artifact(node) else {
        return vec![];
    };
    if metadata.uninstallers.len() != 1 {
        return vec![];
    }
    let path = &metadata.uninstallers[0];
    if !path
        .to_str()
        .is_some_and(|p| crate::safety::sanitize_path(p).is_ok_and(|safe| safe == *path))
    {
        return vec![];
    }
    vec![Step::Run {
        program: Program::Source(SourcePath::Game(path.clone())),
        args: vec!["/S".into(), format!("_?={}", crate::exec::INSTALL_DIR)],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 900,
        expect: vec![],
    }]
}
