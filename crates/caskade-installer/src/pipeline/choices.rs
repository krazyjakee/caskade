//! Server-owned choices for a rerun. Answers bind the full original question,
//! so changing the source cannot apply a stale index to a different decision.
use crate::{Candidate, PlanSet, Question};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct RunControl {
    pub candidate: Option<Candidate>,
    pub answers: Vec<(Question, u32)>,
    plans: Arc<Mutex<Option<PlanSet>>>,
}
impl RunControl {
    pub fn answer(&self, question: &Question) -> Option<usize> {
        self.answers
            .iter()
            .rev()
            .find(|(q, _)| q == question)
            .map(|(_, index)| *index as usize)
            .or_else(|| self.only_option(question))
    }
    /// A question with one option has no choice in it, so it takes that
    /// option. A lone strategy that runs installer code still asks, unless
    /// the user picked the download it came from: that pick was the consent.
    fn only_option(&self, question: &Question) -> Option<usize> {
        let only = match question {
            Question::ChooseAsset { assets } => assets.len() == 1,
            Question::ChooseInstaller { artifacts } => artifacts.len() == 1,
            Question::ChooseLaunchTarget { targets } => targets.len() == 1,
            Question::ChooseDestination { options, .. } => options.len() == 1,
            Question::LowConfidence { candidates } | Question::ConfirmExecution { candidates } => {
                matches!(candidates.as_slice(), [candidate]
                    if !crate::plan::executes(candidate) || self.picked_download())
            }
            Question::ChooseInstallDirectory { .. } | Question::ReinstallAfterMove { .. } => false,
        };
        only.then_some(0)
    }
    /// The latest answer to an engine fallback question given after
    /// `question` was answered, if its strategy is still in the plan.
    fn fallback_after<'a>(&'a self, question: &Question, plans: &PlanSet) -> Option<&'a Candidate> {
        let start = self
            .answers
            .iter()
            .rposition(|(q, _)| q == question)
            .map_or(0, |at| at + 1);
        self.answers[start..]
            .iter()
            .rev()
            .find_map(|(q, index)| match q {
                Question::ConfirmExecution { candidates } => candidates
                    .get(*index as usize)
                    .filter(|candidate| plans.candidates.contains(candidate)),
                _ => None,
            })
    }
    fn picked_download(&self) -> bool {
        self.answers
            .iter()
            .any(|(question, _)| matches!(question, Question::ChooseAsset { .. }))
    }
    pub fn record_plans(&self, plans: &PlanSet) {
        *self.plans.lock().unwrap_or_else(|e| e.into_inner()) = Some(plans.clone());
    }
    pub fn plans(&self) -> Option<PlanSet> {
        self.plans.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn apply(&self, plans: &mut PlanSet) -> super::PipelineResult<()> {
        self.record_plans(plans);
        if let Some(candidate) = &self.candidate {
            let index = plans
                .candidates
                .iter()
                .position(|c| c == candidate)
                .ok_or("selected alternative changed; inspect the new plan before retrying")?;
            plans.selected = Some(index as u32);
            plans.question = None;
        } else if let Some(question) = &plans.question
            && let Some(index) = self.answer(question)
        {
            match question {
                Question::LowConfidence { candidates }
                | Question::ConfirmExecution { candidates } => {
                    let selected = candidates
                        .get(index)
                        .ok_or("candidate choice is out of range")?;
                    // When the chosen strategy failed, the engine asked
                    // before falling back to installer code (CSK-01). That
                    // later answer decides the rerun; the first one would
                    // retry the failed strategy and ask again forever.
                    let selected = self.fallback_after(question, plans).unwrap_or(selected);
                    let index = plans
                        .candidates
                        .iter()
                        .position(|c| c == selected)
                        .ok_or("candidate is no longer available")?;
                    plans.selected = Some(index as u32);
                    plans.question = None;
                }
                _ => return Err("question requires a different stage of the pipeline".into()),
            }
        } else if plans.question.is_none()
            && let Some(chosen) =
                self.answers
                    .iter()
                    .rev()
                    .find_map(|(question, index)| match question {
                        Question::ConfirmExecution { candidates } => {
                            candidates.get(*index as usize)
                        }
                        _ => None,
                    })
            && let Some(index) = plans.candidates.iter().position(|c| c == chosen)
        {
            // The engine asked before falling back to installer code (CSK-01).
            plans.selected = Some(index as u32);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fidelity, HandlerId, Requirements, Strategy};
    fn candidate(handler: &str) -> Candidate {
        Candidate {
            handler: HandlerId(handler.into()),
            strategy: Strategy::Extract,
            confidence: 0.5,
            fidelity: Fidelity::Exact,
            requires: Requirements::default(),
            steps: vec![],
            warnings: vec![],
        }
    }
    #[test]
    fn answer_requires_the_original_question_and_a_current_backend_candidate() {
        let c = candidate("fixture");
        let q = Question::LowConfidence {
            candidates: vec![c.clone()],
        };
        let mut control = RunControl::default();
        control.answers.push((q.clone(), 0));
        let mut plans = PlanSet {
            candidates: vec![c],
            selected: None,
            question: Some(q),
        };
        control.apply(&mut plans).unwrap();
        assert_eq!(plans.selected, Some(0));
        assert!(plans.question.is_none());
        let changed = Question::LowConfidence {
            candidates: vec![candidate("changed"), candidate("other")],
        };
        assert_eq!(control.answer(&changed), None);
        control.candidate = Some(candidate("gone"));
        assert!(control.apply(&mut plans).is_err());
    }
    fn asset(name: &str) -> crate::DownloadAsset {
        crate::DownloadAsset {
            name: name.into(),
            url: format!("https://example.com/{name}").parse().unwrap(),
            size: None,
            sha256: None,
        }
    }
    #[test]
    fn a_lone_option_answers_itself() {
        let control = RunControl::default();
        let one = Question::ChooseAsset {
            assets: vec![asset("game.tar.gz")],
        };
        assert_eq!(control.answer(&one), Some(0));
        let two = Question::ChooseAsset {
            assets: vec![asset("game.tar.gz"), asset("game.deb")],
        };
        assert_eq!(control.answer(&two), None);
        let strategy = Question::LowConfidence {
            candidates: vec![candidate("extract")],
        };
        assert_eq!(control.answer(&strategy), Some(0));
        let observed = Question::ChooseInstallDirectory {
            paths: vec!["/games/a".into()],
            reason: String::new(),
        };
        assert_eq!(control.answer(&observed), None);
    }
    #[test]
    fn a_lone_installer_program_runs_once_the_user_picked_its_download() {
        let mut runs = candidate("apt");
        runs.requires.executes_code = true;
        let confirm = Question::ConfirmExecution {
            candidates: vec![runs.clone()],
        };
        let mut control = RunControl::default();
        assert_eq!(control.answer(&confirm), None);
        let low = Question::LowConfidence {
            candidates: vec![runs.clone()],
        };
        assert_eq!(control.answer(&low), None);
        control.answers.push((
            Question::ChooseAsset {
                assets: vec![asset("game.tar.gz"), asset("game.deb")],
            },
            1,
        ));
        assert_eq!(control.answer(&confirm), Some(0));
        assert_eq!(control.answer(&low), Some(0));
        let several = Question::ConfirmExecution {
            candidates: vec![runs, candidate("extract")],
        };
        assert_eq!(control.answer(&several), None);
    }
    #[test]
    fn the_fallback_answer_replaces_a_failed_choice_on_the_rerun() {
        let mut silent = candidate("silent");
        silent.requires.executes_code = true;
        let mut interactive = candidate("interactive");
        interactive.requires.executes_code = true;
        let extract = candidate("extract");
        let all = vec![silent.clone(), extract.clone(), interactive.clone()];
        let asked = Question::ConfirmExecution {
            candidates: all.clone(),
        };
        let mut control = RunControl::default();
        // Extract first; it failed, and the engine asked before running code.
        control.answers.push((asked.clone(), 1));
        control.answers.push((
            Question::ConfirmExecution {
                candidates: vec![silent.clone(), interactive.clone()],
            },
            0,
        ));
        let mut plans = PlanSet {
            candidates: all.clone(),
            selected: None,
            question: Some(asked.clone()),
        };
        control.apply(&mut plans).unwrap();
        assert_eq!(plans.selected, Some(0));
        assert!(plans.question.is_none());
        // An answer to the plan's question given after a fallback still wins.
        control.answers.push((asked.clone(), 2));
        let mut plans = PlanSet {
            candidates: all,
            selected: None,
            question: Some(asked),
        };
        control.apply(&mut plans).unwrap();
        assert_eq!(plans.selected, Some(2));
    }
}
