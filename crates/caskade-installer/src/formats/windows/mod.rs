//! Windows installer detection and strategies, available on every host.

pub mod inno;

pub mod msi;

mod portable_path;
mod uninstall;
pub use uninstall::uninstall_steps;

pub mod nsis;

pub mod burn;
pub mod generic;
pub mod installshield;
pub mod wise;

use crate::{
    ArtifactNode, Candidate, Fidelity, HandlerId, OperatingSystem, Program, Requirements,
    RuntimeSpec, Step, Strategy, Warning,
    host::{HostCaps, RunnerKind},
    plan::{
        PlanError,
        presets::{BuiltArgs, InstallerMode, PresetError},
    },
};
use std::{collections::BTreeMap, path::PathBuf};

/// Where a Windows installer runs on this host: natively on Windows, else in
/// the default Proton (Linux only) or Wine prefix.
fn installer_runtime(host: &HostCaps) -> Option<RuntimeSpec> {
    match host.os {
        OperatingSystem::Windows => Some(RuntimeSpec::Native),
        OperatingSystem::Linux if host.has_runner(RunnerKind::Proton) => {
            Some(RuntimeSpec::ProtonPrefix {
                id: "default".into(),
            })
        }
        _ if host.has_runner(RunnerKind::Wine) => Some(RuntimeSpec::WinePrefix {
            id: "default".into(),
        }),
        _ => None,
    }
}

/// The static extraction of a whole installer: exact unless a `partial`
/// warning says what it skips, and less certain without parsed metadata.
fn extract_candidate(
    handler: HandlerId,
    node: &ArtifactNode,
    known: bool,
    est_disk_bytes: Option<u64>,
    partial: Option<Warning>,
) -> Candidate {
    Candidate {
        handler,
        strategy: Strategy::Extract,
        confidence: if known { 1.0 } else { 0.8 },
        fidelity: if partial.is_some() {
            Fidelity::Partial
        } else {
            Fidelity::Exact
        },
        requires: Requirements {
            est_disk_bytes,
            ..Default::default()
        },
        steps: vec![Step::Extract {
            from: node.id.clone(),
            select: vec![],
            strip: None,
            to: PathBuf::from("."),
        }],
        warnings: partial.into_iter().collect(),
    }
}

/// How an installer's own program runs, for [`run_candidates`].
struct Run {
    handler: HandlerId,
    program: Program,
    /// Before the mode's switches (`msiexec`'s `/i <source>`).
    leading_args: Vec<String>,
    timeout_secs: u64,
    /// What a silent run must leave behind.
    expect: Vec<PathBuf>,
    may_prompt_elevation: bool,
    est_disk_bytes: Option<u64>,
    confidence: f32,
    /// The code of the switches' warnings.
    warning_code: &'static str,
}

/// One candidate per mode (silent, then interactive) running the installer in
/// `runtime` with that mode's switches from `switches`.
fn run_candidates(
    run: Run,
    runtime: RuntimeSpec,
    modes: &[InstallerMode],
    mut switches: impl FnMut(InstallerMode) -> Result<BuiltArgs, PresetError>,
) -> Result<Vec<Candidate>, PlanError> {
    let mut candidates = vec![];
    for &mode in modes {
        let preset = switches(mode).map_err(|error| PlanError::Handler {
            handler: run.handler.clone(),
            message: error.to_string(),
        })?;
        let mut args = run.leading_args.clone();
        args.extend(preset.args);
        let interactive = mode == InstallerMode::Interactive;
        let step = if interactive {
            Step::RunInteractive {
                program: run.program.clone(),
                args,
                env: BTreeMap::new(),
                runtime: runtime.clone(),
                target_dir: ".".into(),
            }
        } else {
            Step::Run {
                program: run.program.clone(),
                args,
                env: BTreeMap::new(),
                runtime: runtime.clone(),
                timeout_secs: run.timeout_secs,
                expect: run.expect.clone(),
            }
        };
        candidates.push(Candidate {
            handler: run.handler.clone(),
            strategy: if interactive {
                Strategy::RunInteractive
            } else {
                Strategy::RunSilent
            },
            confidence: run.confidence,
            fidelity: Fidelity::Native,
            requires: Requirements {
                executes_code: true,
                runtime: Some(runtime.clone()),
                may_prompt_elevation: run.may_prompt_elevation,
                interactive,
                est_disk_bytes: run.est_disk_bytes,
            },
            steps: vec![step],
            warnings: preset
                .warnings
                .into_iter()
                .map(|message| Warning {
                    code: run.warning_code.into(),
                    message,
                    path: None,
                })
                .collect(),
        });
    }
    Ok(candidates)
}
