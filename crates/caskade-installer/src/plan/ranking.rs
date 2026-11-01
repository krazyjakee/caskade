use std::cmp::Ordering;

use crate::{
    ArtifactNode, Fidelity, OperatingSystem, PayloadPlatform, Program, Question, Role, RuntimeSpec,
    Step, Strategy, host::RunnerKind,
};

use super::{Candidate, ExecutionPolicy, PlanContext, PlanSet, WindowsMethod};

pub(super) struct Ranked<'a> {
    pub candidate: Candidate,
    pub artifact: &'a ArtifactNode,
    pub question: Option<Question>,
}

pub(crate) fn executes(candidate: &Candidate) -> bool {
    candidate.requires.executes_code
        || matches!(
            candidate.strategy,
            Strategy::RunSilent | Strategy::RunInteractive | Strategy::Delegate
        )
        || candidate.steps.iter().any(|step| {
            matches!(
                step,
                Step::Run { .. } | Step::RunInteractive { .. } | Step::Delegate { .. }
            )
        })
}

fn delegates(candidate: &Candidate) -> bool {
    candidate.strategy == Strategy::Delegate
        || candidate
            .steps
            .iter()
            .any(|step| matches!(step, Step::Delegate { .. }))
}

fn native_payload_supported(artifact: &ArtifactNode, context: &PlanContext<'_>) -> bool {
    match artifact.platform {
        PayloadPlatform::Windows => context.host.os == OperatingSystem::Windows,
        PayloadPlatform::Linux => context.host.os == OperatingSystem::Linux,
        PayloadPlatform::Macos => context.host.os == OperatingSystem::Macos,
        PayloadPlatform::Unknown => true,
        PayloadPlatform::ConsoleRom { .. } => false,
    }
}

fn runtime_available(
    runtime: &RuntimeSpec,
    artifact: &ArtifactNode,
    context: &PlanContext<'_>,
) -> bool {
    match runtime {
        RuntimeSpec::Native => {
            context.host.has_runner(RunnerKind::Native)
                && native_payload_supported(artifact, context)
        }
        RuntimeSpec::ProtonPrefix { .. } => {
            context.host.os == OperatingSystem::Linux
                && context.host.has_runner(RunnerKind::Proton)
                && matches!(
                    artifact.platform,
                    PayloadPlatform::Windows | PayloadPlatform::Unknown
                )
        }
        RuntimeSpec::WinePrefix { .. } => {
            context.host.os != OperatingSystem::Windows
                && context.host.has_runner(RunnerKind::Wine)
                && matches!(
                    artifact.platform,
                    PayloadPlatform::Windows | PayloadPlatform::Unknown
                )
        }
    }
}

pub(super) fn available(
    candidate: &Candidate,
    artifact: &ArtifactNode,
    context: &PlanContext<'_>,
) -> bool {
    if context.policy.execution == ExecutionPolicy::Never && executes(candidate) {
        return false;
    }
    if candidate.strategy != Strategy::HandOff {
        if matches!(
            artifact.platform,
            PayloadPlatform::Linux | PayloadPlatform::Macos
        ) && !native_payload_supported(artifact, context)
        {
            return false;
        }
        if !delegates(candidate) && !context.host.filesystem.writable {
            return false;
        }
    }
    if let Some(runtime) = &candidate.requires.runtime
        && !runtime_available(runtime, artifact, context)
    {
        return false;
    }
    // Validate the steps as well as declarations: plugins must not bypass a
    // capability or execution filter with an incomplete Requirements value.
    candidate.steps.iter().all(|step| match step {
        Step::Run {
            runtime, program, ..
        }
        | Step::RunInteractive {
            runtime, program, ..
        } => {
            // Host tools execute on the host, independent of the payload OS.
            let available =
                if matches!(program, Program::Tool(_)) && *runtime == RuntimeSpec::Native {
                    context.host.has_runner(RunnerKind::Native)
                } else {
                    runtime_available(runtime, artifact, context)
                };
            available
                && match program {
                    Program::Tool(tool) if matches!(tool.as_str(), "7z" | "7zz" | "7za") => {
                        context.host.seven_zip.is_some()
                    }
                    _ => true,
                }
        }
        Step::RegistryImport { runtime, .. } => {
            *runtime != RuntimeSpec::Native && runtime_available(runtime, artifact, context)
        }
        Step::Symlink { .. } => context.host.filesystem.symlinks,
        Step::SetExecutable { .. } => {
            context.host.os == OperatingSystem::Windows || context.host.filesystem.exec_bits
        }
        Step::Mount { .. } | Step::Unmount { .. } => context.host.os == OperatingSystem::Macos,
        _ => true,
    })
}

fn strategy_rank(strategy: Strategy, context: &PlanContext<'_>) -> u8 {
    let extract_first = context.host.os != OperatingSystem::Windows
        || context.policy.windows_method == WindowsMethod::Extract;
    // Installers run silently first; extraction is the fallback when that
    // fails, ahead of an installer that needs someone to click through it.
    match strategy {
        Strategy::Copy | Strategy::HandOff => 0,
        Strategy::Extract if extract_first => 1,
        Strategy::RunSilent => 2,
        Strategy::Extract => 3,
        Strategy::RunInteractive => 4,
        Strategy::Delegate => 5,
    }
}

fn fidelity_rank(fidelity: Fidelity) -> u8 {
    match fidelity {
        Fidelity::Native => 0,
        Fidelity::Exact => 1,
        Fidelity::Equivalent => 2,
        Fidelity::Partial => 3,
    }
}

fn compare(a: &Ranked<'_>, b: &Ranked<'_>, context: &PlanContext<'_>) -> Ordering {
    delegates(&a.candidate)
        .cmp(&delegates(&b.candidate))
        .then_with(|| {
            strategy_rank(a.candidate.strategy, context)
                .cmp(&strategy_rank(b.candidate.strategy, context))
        })
        .then_with(|| b.candidate.confidence.total_cmp(&a.candidate.confidence))
        .then_with(|| fidelity_rank(a.candidate.fidelity).cmp(&fidelity_rank(b.candidate.fidelity)))
}

pub(super) fn select(
    mut ranked: Vec<Ranked<'_>>,
    questions: Vec<Question>,
    context: &PlanContext<'_>,
) -> PlanSet {
    ranked.sort_by(|a, b| {
        compare(a, b, context)
            .then_with(|| a.artifact.id.cmp(&b.artifact.id))
            .then_with(|| a.candidate.handler.cmp(&b.candidate.handler))
    });
    let mut question = ranked
        .first()
        .and_then(|top| top.question.clone())
        .or_else(|| questions.into_iter().next());
    if let Some(top) = ranked.first() {
        if question.is_none() && top.artifact.role == Role::Installer {
            let mut artifacts = vec![top.artifact.id.clone()];
            for alternative in ranked.iter().skip(1) {
                if compare(top, alternative, context) != Ordering::Equal {
                    break;
                }
                if alternative.artifact.role == Role::Installer
                    && !artifacts.contains(&alternative.artifact.id)
                {
                    artifacts.push(alternative.artifact.id.clone());
                }
            }
            if artifacts.len() > 1 {
                question = Some(Question::ChooseInstaller { artifacts });
            }
        }
        if question.is_none() && top.candidate.confidence < context.policy.ask_below_confidence {
            question = Some(Question::LowConfidence {
                candidates: ranked.iter().map(|p| p.candidate.clone()).collect(),
            });
        }
        if question.is_none()
            && context.policy.execution == ExecutionPolicy::Ask
            && executes(&top.candidate)
        {
            question = Some(Question::ConfirmExecution {
                candidates: ranked.iter().map(|p| p.candidate.clone()).collect(),
            });
        }
    }
    let selected = if !ranked.is_empty() && question.is_none() {
        Some(0)
    } else {
        None
    };
    PlanSet {
        candidates: ranked.into_iter().map(|p| p.candidate).collect(),
        selected,
        question,
    }
}
