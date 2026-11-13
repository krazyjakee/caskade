use super::*;
use crate::{
    ArtifactId, ArtifactNode, Format, HandlerId, PayloadPlatform, Program, Role, RuntimeSpec, Step,
    Warning,
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
pub struct MsiProbe;
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
        let partial = metadata.as_ref().is_none_or(MsiMetadata::is_partial).then(|| Warning { code: "msi.partial".into(), message: "Table extraction omits custom actions, external shortcuts and files outside the application directory".into(), path: None });
        let estimate = metadata.as_ref().and_then(MsiMetadata::unpacked_bytes);
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
                program: Program::Tool("msiexec".into()),
                leading_args: vec!["/i".into(), crate::exec::source_argument(&node.id)],
                timeout_secs: 900,
                expect: metadata
                    .as_ref()
                    .into_iter()
                    .flat_map(|m| m.files.first())
                    .map(|f| f.path.clone())
                    .collect(),
                may_prompt_elevation: true,
                est_disk_bytes: estimate,
                confidence: 1.0,
                warning_code: "msi.preset",
            };
            candidates.extend(super::super::run_candidates(
                run,
                runtime,
                &[InstallerMode::Silent, InstallerMode::Interactive],
                |mode| build_args(InstallerFamily::Msi, mode, &settings),
            )?);
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
