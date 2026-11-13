use std::{collections::BTreeSet, path::Path};

use crate::{ArtifactNode, Question, host::HostCaps};

use super::{Candidate, HandlerId, PlanSet, Policy, ranking};

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("handler {id:?} is already registered")]
    DuplicateHandler { id: HandlerId },
    #[error("handler identifiers must not be empty")]
    EmptyHandlerId,
    #[error("confidence must be finite and in 0..=1 ({value})")]
    InvalidConfidence { value: f32 },
    #[error("handler {expected:?} proposed a candidate for {actual:?}")]
    HandlerMismatch {
        expected: HandlerId,
        actual: HandlerId,
    },
    #[error("handler {handler:?} failed: {message}")]
    Handler { handler: HandlerId, message: String },
    #[error("artifact identifier {id:?} is duplicated")]
    DuplicateArtifact { id: crate::ArtifactId },
}

/// All environmental input is explicit; handlers must not detect the real host.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    pub host: &'a HostCaps,
    pub policy: &'a Policy,
    /// Final managed path (or stable alias) for installer directory arguments.
    /// None is sufficient for extraction, copy and other destination-free plans.
    /// Executing handlers must not invent a host-specific install directory.
    pub target_dir: Option<&'a Path>,
}

impl<'a> PlanContext<'a> {
    pub fn new(host: &'a HostCaps, policy: &'a Policy) -> Self {
        Self {
            host,
            policy,
            target_dir: None,
        }
    }

    pub fn with_target_dir(mut self, target_dir: &'a Path) -> Self {
        self.target_dir = Some(target_dir);
        self
    }
}

/// Multiple strategies for one artifact, plus an optional genuine ambiguity.
/// Do not ask merely because a proposal runs code or may trigger OS elevation.
#[derive(Debug, Clone, Default)]
pub struct Proposal {
    pub candidates: Vec<Candidate>,
    pub question: Option<Question>,
}

/// Shared contract for built-in handlers and plugin adapters. `claims` only
/// inspects static metadata. `propose` returns data and never executes steps.
pub trait Handler: Send + Sync {
    fn id(&self) -> HandlerId;
    fn claims(&self, artifact: &ArtifactNode) -> bool;
    fn propose(
        &self,
        artifact: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> Result<Proposal, PlanError>;
}

/// Registry and host-aware planner. No format handlers are registered implicitly;
/// callers register the handlers enabled by their build and plugin configuration.
#[derive(Default)]
pub struct PlanEngine {
    handlers: Vec<(HandlerId, Box<dyn Handler>)>,
}

impl PlanEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, handler: Box<dyn Handler>) -> Result<(), PlanError> {
        let id = handler.id();
        if id.0.trim().is_empty() {
            return Err(PlanError::EmptyHandlerId);
        }
        if self.handlers.iter().any(|(existing, _)| *existing == id) {
            return Err(PlanError::DuplicateHandler { id });
        }
        self.handlers.push((id, handler));
        Ok(())
    }

    /// Visit every node, retain all usable strategies, then select the best one.
    /// Questions are returned in `PlanSet::question` (`InstallOutcome::NeedsUser`
    /// at the application boundary); an empty registry returns an empty set.
    pub fn plan(
        &self,
        artifacts: &[ArtifactNode],
        host: &HostCaps,
        policy: &Policy,
    ) -> Result<PlanSet, PlanError> {
        self.plan_with_context(artifacts, &PlanContext::new(host, policy))
    }

    /// Plan with a final destination for native/prefix installer argument builders.
    /// The destination is supplied by the caller, never inspected or created here.
    pub fn plan_with_context(
        &self,
        artifacts: &[ArtifactNode],
        context: &PlanContext<'_>,
    ) -> Result<PlanSet, PlanError> {
        validate_confidence(context.policy.ask_below_confidence)?;
        let mut nodes = vec![];
        let mut ids = BTreeSet::new();
        collect_nodes(artifacts, &mut nodes, &mut ids)?;
        let mut candidates = vec![];
        let mut questions = vec![];
        for artifact in nodes {
            for (id, handler) in &self.handlers {
                if !handler.claims(artifact) {
                    continue;
                }
                let proposal = handler.propose(artifact, context)?;
                let needs_choice_before_proposing = proposal.candidates.is_empty();
                let start = candidates.len();
                for candidate in proposal.candidates {
                    validate_confidence(candidate.confidence)?;
                    if candidate.handler != *id {
                        return Err(PlanError::HandlerMismatch {
                            expected: id.clone(),
                            actual: candidate.handler,
                        });
                    }
                    if ranking::available(&candidate, artifact, context) {
                        candidates.push(ranking::Ranked {
                            candidate,
                            artifact,
                            question: None,
                        });
                    }
                }
                if let Some(question) = proposal.question {
                    // A handler may need a choice before it can propose a plan.
                    // Otherwise attach the question only to its usable proposals.
                    if needs_choice_before_proposing {
                        questions.push(question);
                    } else {
                        for candidate in &mut candidates[start..] {
                            candidate.question = Some(question.clone());
                        }
                    }
                }
            }
        }
        Ok(ranking::select(candidates, questions, context))
    }
}

fn validate_confidence(value: f32) -> Result<(), PlanError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(PlanError::InvalidConfidence { value });
    }
    Ok(())
}

fn collect_nodes<'a>(
    artifacts: &'a [ArtifactNode],
    nodes: &mut Vec<&'a ArtifactNode>,
    ids: &mut BTreeSet<crate::ArtifactId>,
) -> Result<(), PlanError> {
    for artifact in artifacts {
        if !ids.insert(artifact.id.clone()) {
            return Err(PlanError::DuplicateArtifact {
                id: artifact.id.clone(),
            });
        }
        nodes.push(artifact);
        collect_nodes(&artifact.children, nodes, ids)?;
    }
    Ok(())
}
