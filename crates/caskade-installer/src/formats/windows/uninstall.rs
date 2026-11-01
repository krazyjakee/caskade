//! Record only the uninstaller for the strategy that actually executed code.
use crate::{ArtifactNode, Candidate, Format, Step, Strategy};

pub fn uninstall_steps(artifacts: &[ArtifactNode], candidate: &Candidate) -> Vec<Step> {
    if !matches!(
        candidate.strategy,
        Strategy::RunSilent | Strategy::RunInteractive
    ) {
        return vec![];
    }
    let Some(runtime) = candidate.steps.iter().find_map(|step| match step {
        Step::Run { runtime, .. } | Step::RunInteractive { runtime, .. } => Some(runtime),
        _ => None,
    }) else {
        return vec![];
    };
    // The executed artifact is authoritative; sibling installers in a bundle
    // must never acquire uninstall records merely because they were probed.
    let source = candidate.steps.iter().find_map(|step| match step {
        Step::Run {
            program: crate::Program::Source(crate::SourcePath::Artifact { artifact, .. }),
            ..
        }
        | Step::RunInteractive {
            program: crate::Program::Source(crate::SourcePath::Artifact { artifact, .. }),
            ..
        } => Some(artifact.clone()),
        Step::Run { args, .. } | Step::RunInteractive { args, .. } => args.iter().find_map(|arg| {
            arg.strip_prefix("{caskade.source:")
                .and_then(|s| s.strip_suffix('}'))
                .map(|id| crate::ArtifactId(id.into()))
        }),
        _ => None,
    });
    fn find<'a>(nodes: &'a [ArtifactNode], id: &crate::ArtifactId) -> Option<&'a ArtifactNode> {
        for node in nodes {
            if node.id == *id {
                return Some(node);
            }
            if let Some(node) = find(&node.children, id) {
                return Some(node);
            }
        }
        None
    }
    let Some(node) = source.as_ref().and_then(|id| find(artifacts, id)) else {
        return vec![];
    };
    let mut steps = match (candidate.handler.0.as_str(), node.format) {
        ("msi", Format::Msi) => super::msi::uninstall_steps(node),
        ("inno", Format::Inno) => super::inno::uninstall_steps(node),
        ("nsis", Format::Nsis) => super::nsis::uninstall_steps(node),
        _ => vec![],
    };
    for step in &mut steps {
        match step {
            Step::Run { runtime: spec, .. } | Step::RunInteractive { runtime: spec, .. } => {
                *spec = runtime.clone()
            }
            _ => (),
        }
    }
    steps
}
