//! Conservative fallback for identified executable installers.
use crate::{
    plan::{
        Handler, PlanContext, PlanError, Proposal,
        presets::{InstallerFamily, InstallerMode, SwitchSettings, build_args},
    },
    *,
};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct GenericHandler;
impl GenericHandler {
    pub fn new() -> Self {
        Self
    }
}
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
        for mode in [InstallerMode::Silent, InstallerMode::Interactive] {
            if mode == InstallerMode::Silent && !silent {
                continue;
            }
            let settings = SwitchSettings::from(&context.policy.preset_args);
            let mut preset = build_args(
                family,
                if family == InstallerFamily::InstallShield {
                    InstallerMode::Interactive
                } else {
                    mode
                },
                &settings,
            )
            .map_err(|e| PlanError::Handler {
                handler: handler.clone(),
                message: e.to_string(),
            })?;
            if family == InstallerFamily::InstallShield && mode == InstallerMode::Silent {
                preset.args =
                    super::installshield::identified_msi_args(&settings).map_err(|e| {
                        PlanError::Handler {
                            handler: handler.clone(),
                            message: e.to_string(),
                        }
                    })?;
                preset
                    .warnings
                    .retain(|message| !message.starts_with("Install directory cannot be preset"));
            }
            let program = Program::Source(SourcePath::Artifact {
                artifact: node.id.clone(),
                path: ".".into(),
            });
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
            let step = if mode == InstallerMode::Silent {
                Step::Run {
                    program,
                    args: preset.args,
                    env: BTreeMap::new(),
                    runtime: runtime.clone(),
                    timeout_secs: 1800,
                    expect,
                }
            } else {
                Step::RunInteractive {
                    program,
                    args: preset.args,
                    env: BTreeMap::new(),
                    runtime: runtime.clone(),
                    target_dir: ".".into(),
                }
            };
            let mut warnings: Vec<_> = preset
                .warnings
                .into_iter()
                .map(|message| Warning {
                    code: "installer.preset".into(),
                    message,
                    path: None,
                })
                .collect();
            if !silent {
                warnings.push(Warning {
                    code: "installer.interactive_fallback".into(),
                    message: "No verified silent contract for this installer variant; run the wizard with supported presets only.".into(),
                    path: None,
                });
            }
            candidates.push(Candidate {
                handler: handler.clone(),
                strategy: if mode == InstallerMode::Silent {
                    Strategy::RunSilent
                } else {
                    Strategy::RunInteractive
                },
                confidence: if burn || silent { 0.98 } else { 0.9 },
                fidelity: Fidelity::Native,
                requires: Requirements {
                    executes_code: true,
                    runtime: Some(runtime.clone()),
                    may_prompt_elevation: true,
                    interactive: mode == InstallerMode::Interactive,
                    est_disk_bytes: None,
                },
                steps: vec![step],
                warnings,
            });
        }
    }
    Ok(Proposal {
        candidates,
        question: None,
    })
}
