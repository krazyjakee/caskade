//! Conservative fallback for identified executable installers.
use crate::{
    plan::{
        Handler, PlanContext, PlanError, Proposal,
        presets::{InstallerFamily, InstallerMode, SwitchSettings, build_args},
    },
    *,
};
#[derive(Default)]
pub struct GenericHandler;
impl Handler for GenericHandler {
    fn id(&self) -> HandlerId {
        HandlerId("windows-generic".into())
    }
    fn claims(&self, node: &ArtifactNode) -> bool {
        node.format == Format::Pe
            && node.role == Role::Installer
            && !node.facts.contains_key("pe.sfx_format")
    }
    fn propose(
        &self,
        node: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> std::result::Result<Proposal, PlanError> {
        propose(
            self.id(),
            node,
            context,
            InstallerFamily::Custom,
            false,
            false,
        )
    }
}
pub(crate) fn propose(
    handler: HandlerId,
    node: &ArtifactNode,
    context: &PlanContext<'_>,
    family: InstallerFamily,
    silent: bool,
    burn: bool,
) -> std::result::Result<Proposal, PlanError> {
    let mut candidates = vec![];
    let overlay = node
        .facts
        .get("pe.overlay_size")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0;
    if burn || (overlay && context.host.seven_zip.is_some()) {
        candidates.push(Candidate {
            handler: handler.clone(),
            strategy: Strategy::Extract,
            confidence: if burn { 0.98 } else { 0.8 },
            fidelity: Fidelity::Partial,
            requires: Requirements::default(),
            steps: vec![Step::Extract {
                from: node.id.clone(),
                select: vec![],
                strip: None,
                to: ".".into(),
            }],
            warnings: vec![Warning {
                code: "installer.static_partial".into(),
                message: if burn {
                    "Embedded MSI payload extraction skips bundle prerequisites, custom actions and registration."
                } else {
                    "Best-effort 7-Zip overlay extraction; authored installer actions are skipped."
                }
                .into(),
                path: None,
            }],
        });
    }
    if let Some(runtime) = super::installer_runtime(context.host) {
        let expect = node
            .facts
            .get("installer.expect")
            .cloned()
            .map(serde_json::from_value::<Vec<std::path::PathBuf>>)
            .transpose()
            .map_err(|e| PlanError::Handler {
                handler: handler.clone(),
                message: e.to_string(),
            })?
            .unwrap_or_default();
        for path in &expect {
            if path
                .to_str()
                .is_none_or(|text| crate::safety::sanitize_path(text).is_err())
            {
                return Err(PlanError::Handler {
                    handler: handler.clone(),
                    message: "unsafe installer expected path".into(),
                });
            }
        }
        let settings = SwitchSettings::from(&context.policy.preset_args);
        let run = super::Run {
            handler: handler.clone(),
            program: Program::Source(SourcePath::Artifact {
                artifact: node.id.clone(),
                path: ".".into(),
            }),
            leading_args: vec![],
            timeout_secs: 1800,
            expect,
            may_prompt_elevation: true,
            est_disk_bytes: None,
            confidence: if burn || silent { 0.98 } else { 0.9 },
            warning_code: "installer.preset",
        };
        let modes: &[InstallerMode] = if silent {
            &[InstallerMode::Silent, InstallerMode::Interactive]
        } else {
            &[InstallerMode::Interactive]
        };
        let mut runs = super::run_candidates(run, runtime, modes, |mode| {
            if family != InstallerFamily::InstallShield {
                return build_args(family, mode, &settings);
            }
            let mut preset = build_args(family, InstallerMode::Interactive, &settings)?;
            if mode == InstallerMode::Silent {
                preset.args = super::installshield::identified_msi_args(&settings)?;
                preset
                    .warnings
                    .retain(|message| !message.starts_with("Install directory cannot be preset"));
            }
            Ok(preset)
        })?;
        if !silent {
            for candidate in &mut runs {
                candidate.warnings.push(Warning {
                    code: "installer.interactive_fallback".into(),
                    message: "No verified silent contract for this installer variant; run the wizard with supported presets only.".into(),
                    path: None,
                });
            }
        }
        candidates.extend(runs);
    }
    Ok(Proposal {
        candidates,
        question: None,
    })
}
