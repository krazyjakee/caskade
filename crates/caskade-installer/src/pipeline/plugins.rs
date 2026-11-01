//! Plugin seams (#55, INSTALLER-PLAN §10): extra handlers and URL resolvers,
//! and the `install.plan` review. The installer knows no plugin runtime;
//! the app adapts its plugins to these traits and puts them in [`Context`].
//!
//! [`Context`]: super::Context

use std::{future::Future, pin::Pin, sync::Arc};

use crate::{
    ArtifactNode,
    plan::{Handler, HandlerId, PlanContext, PlanError, PlanSet, Proposal},
};

use super::{Context, PipelineResult};

pub type ReviewFuture<'a> = Pin<Box<dyn Future<Output = Result<PlanSet, String>> + Send + 'a>>;

/// `install.plan`: sees the ranked plans before anything runs.
pub trait PlanReview: Send + Sync {
    /// The plans to go on with, or `Err(reason)` to stop the install. Only
    /// reordering, dropping or selecting the given candidates is accepted.
    fn review<'a>(&'a self, artifact: &'a ArtifactNode, plans: PlanSet) -> ReviewFuture<'a>;
}

/// A shared handler in the plan engine, which owns its handlers.
pub(super) struct Shared(pub Arc<dyn Handler>);

impl Handler for Shared {
    fn id(&self) -> HandlerId {
        self.0.id()
    }
    fn claims(&self, artifact: &ArtifactNode) -> bool {
        self.0.claims(artifact)
    }
    fn propose(
        &self,
        artifact: &ArtifactNode,
        context: &PlanContext<'_>,
    ) -> Result<Proposal, PlanError> {
        self.0.propose(artifact, context)
    }
}

/// Runs the review, and refuses an answer that adds or changes a step.
pub(super) async fn review(
    ctx: &Context,
    artifact: &ArtifactNode,
    plans: PlanSet,
) -> PipelineResult<PlanSet> {
    let Some(review) = &ctx.review else {
        return Ok(plans);
    };
    let original = plans.candidates.clone();
    let reviewed = review
        .review(artifact, plans)
        .await
        .map_err(|reason| format!("install.plan stopped the install: {reason}"))?;
    reviewed_subset(&original, reviewed)
}

fn reviewed_subset(original: &[crate::Candidate], reviewed: PlanSet) -> PipelineResult<PlanSet> {
    if reviewed.question.is_some() {
        return Err("install.plan may not ask a question".into());
    }
    if reviewed
        .candidates
        .iter()
        .any(|candidate| !original.contains(candidate))
    {
        return Err("install.plan may reorder, drop or select candidates, not change them".into());
    }
    if reviewed.candidates.is_empty() {
        return Err("install.plan dropped every candidate".into());
    }
    if reviewed
        .selected
        .is_some_and(|index| index as usize >= reviewed.candidates.len())
    {
        return Err("install.plan selected a candidate that isn't there".into());
    }
    Ok(reviewed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Candidate, Fidelity, Requirements, Step, Strategy};
    use std::path::PathBuf;

    fn candidate(handler: &str) -> Candidate {
        Candidate {
            handler: HandlerId(handler.into()),
            strategy: Strategy::Extract,
            confidence: 0.5,
            fidelity: Fidelity::Exact,
            requires: Requirements::default(),
            steps: vec![Step::SetExecutable {
                path: PathBuf::from("game"),
            }],
            warnings: vec![],
        }
    }

    fn set(candidates: Vec<Candidate>, selected: Option<u32>) -> PlanSet {
        PlanSet {
            candidates,
            selected,
            question: None,
        }
    }

    #[test]
    fn a_review_may_reorder_drop_and_select_but_not_change() {
        let original = vec![candidate("a"), candidate("b")];
        let reordered = reviewed_subset(&original, set(vec![candidate("b")], Some(0))).unwrap();
        assert_eq!(reordered.candidates, vec![candidate("b")]);

        let mut changed = candidate("a");
        changed.steps.push(Step::WriteFile {
            path: PathBuf::from("x"),
            contents: vec![],
        });
        for bad in [
            set(vec![changed], None),
            set(vec![], None),
            set(vec![candidate("a")], Some(1)),
        ] {
            assert!(reviewed_subset(&original, bad).is_err());
        }
    }
}
