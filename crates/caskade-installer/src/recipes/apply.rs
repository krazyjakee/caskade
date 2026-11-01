use super::{Recipe, RecipeError, RecipeSet, ResolvedRecipe, Result};
use crate::{
    ArtifactNode, Candidate, Step,
    plan::{CandidateTransform, PlanError, PresetArgs},
};
fn merge<T: PartialEq + Clone>(into: &mut Option<T>, other: &Option<T>, name: &str) -> Result<()> {
    if let Some(value) = other {
        if into.as_ref().is_some_and(|previous| previous != value) {
            return Err(RecipeError::Conflict(name.into()));
        }
        *into = Some(value.clone());
    }
    Ok(())
}
pub(super) fn resolve<'a>(recipes: impl Iterator<Item = &'a Recipe>) -> Result<ResolvedRecipe> {
    let mut resolved = ResolvedRecipe::default();
    let mut alias = None;
    let mut total = 0_usize;
    for recipe in recipes {
        merge(&mut resolved.strategy, &recipe.strategy, "strategy")?;
        merge(
            &mut resolved.preset_args.language,
            &recipe.preset_args.language,
            "language",
        )?;
        merge(
            &mut resolved.preset_args.no_shortcuts,
            &recipe.preset_args.no_shortcuts,
            "no_shortcuts",
        )?;
        merge(&mut alias, &recipe.disable_alias, "disable_alias")?;
        total = total
            .checked_add(
                serde_json::to_vec(&recipe.add_steps)
                    .map_err(|_| RecipeError::Limit)?
                    .len(),
            )
            .ok_or(RecipeError::Limit)?;
        if total > 1024 * 1024 || resolved.add_steps.len() + recipe.add_steps.len() > 256 {
            return Err(RecipeError::Limit);
        }
        resolved.recipe_ids.push(recipe.id.clone());
        resolved.add_steps.extend(recipe.add_steps.iter().cloned());
    }
    resolved.disable_alias = alias.unwrap_or(false);
    Ok(resolved)
}
fn plan_error(error: RecipeError) -> PlanError {
    PlanError::Override {
        message: error.to_string(),
    }
}
impl CandidateTransform for RecipeSet {
    fn preset_args(
        &self,
        node: &ArtifactNode,
        base: &PresetArgs,
    ) -> std::result::Result<PresetArgs, PlanError> {
        let resolved = self.resolve_artifact(node).map_err(plan_error)?;
        let mut args = base.clone();
        if let Some(language) = resolved.preset_args.language {
            args.language = Some(language);
        }
        if let Some(no_shortcuts) = resolved.preset_args.no_shortcuts {
            args.no_shortcuts = no_shortcuts;
        }
        Ok(args)
    }
    fn transform(
        &self,
        node: &ArtifactNode,
        mut candidate: Candidate,
    ) -> std::result::Result<Option<Candidate>, PlanError> {
        let resolved = self.resolve_artifact(node).map_err(plan_error)?;
        if resolved
            .strategy
            .is_some_and(|strategy| candidate.strategy != strategy)
        {
            return Ok(None);
        }
        if candidate.steps.len() + resolved.add_steps.len() > 1024 {
            return Err(plan_error(RecipeError::Limit));
        }
        if resolved.add_steps.iter().any(|step| {
            matches!(
                step,
                Step::Run { .. } | Step::RunInteractive { .. } | Step::Delegate { .. }
            )
        }) {
            candidate.requires.executes_code = true;
        }
        if resolved
            .add_steps
            .iter()
            .any(|step| matches!(step, Step::RunInteractive { .. }))
        {
            candidate.requires.interactive = true;
        }
        candidate.steps.extend(resolved.add_steps);
        Ok(Some(candidate))
    }
}
