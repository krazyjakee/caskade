use std::collections::BTreeMap;

use crate::{
    ArtifactId, ArtifactNode, Format, OperatingSystem, PayloadPlatform, Program, Question, Role,
    RuntimeSpec, SourcePath, host::HostCaps,
};

use super::*;

fn host(name: &str) -> HostCaps {
    crate::pipeline::input::host(Some(name)).unwrap()
}

fn artifact() -> ArtifactNode {
    ArtifactNode {
        id: ArtifactId("setup".into()),
        path: "setup.exe".into(),
        format: Format::Inno,
        platform: PayloadPlatform::Windows,
        role: Role::Installer,
        size: Some(1024),
        facts: BTreeMap::new(),
        children: vec![],
        children_complete: true,
    }
}

fn candidate(strategy: Strategy) -> Candidate {
    Candidate {
        handler: HandlerId("test".into()),
        strategy,
        confidence: 0.9,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: matches!(
                strategy,
                Strategy::RunSilent | Strategy::RunInteractive | Strategy::Delegate
            ),
            runtime: None,
            may_prompt_elevation: false,
            interactive: strategy == Strategy::RunInteractive,
            est_disk_bytes: None,
        },
        steps: vec![],
        warnings: vec![],
    }
}

#[derive(Clone)]
struct TestHandler(Proposal);

impl Handler for TestHandler {
    fn id(&self) -> HandlerId {
        HandlerId("test".into())
    }
    fn claims(&self, _: &ArtifactNode) -> bool {
        true
    }
    fn propose(&self, _: &ArtifactNode, _: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        Ok(self.0.clone())
    }
}

fn engine(proposal: Proposal) -> PlanEngine {
    let mut engine = PlanEngine::new();
    engine.register(Box::new(TestHandler(proposal))).unwrap();
    engine
}

fn plan(candidates: Vec<Candidate>, profile: &str, policy: Policy) -> PlanSet {
    engine(Proposal {
        candidates,
        question: None,
    })
    .plan(&[artifact()], &host(profile), &policy)
    .unwrap()
}

#[test]
fn host_fallback_order_and_all_alternatives_are_preserved() {
    for profile in [
        "windows",
        "linux",
        "linux-no-umu",
        "linux-exfat-root",
        "macos",
        "macos-no-wine",
    ] {
        let candidates = [
            Strategy::Delegate,
            Strategy::Extract,
            Strategy::RunInteractive,
            Strategy::RunSilent,
        ]
        .map(candidate)
        .to_vec();
        let plans = plan(candidates, profile, Policy::default());
        let expected = if profile == "windows" {
            vec![
                Strategy::RunSilent,
                Strategy::Extract,
                Strategy::RunInteractive,
                Strategy::Delegate,
            ]
        } else {
            vec![
                Strategy::Extract,
                Strategy::RunSilent,
                Strategy::RunInteractive,
                Strategy::Delegate,
            ]
        };
        assert_eq!(
            plans
                .candidates
                .iter()
                .map(|c| c.strategy)
                .collect::<Vec<_>>(),
            expected,
            "{profile}"
        );
        assert_eq!(plans.selected, Some(0));
        assert!(plans.question.is_none());
    }
}

#[test]
fn every_managed_strategy_beats_delegate_even_at_lower_confidence() {
    for strategy in [
        Strategy::Copy,
        Strategy::Extract,
        Strategy::RunSilent,
        Strategy::RunInteractive,
    ] {
        let mut managed = candidate(strategy);
        managed.confidence = 0.1;
        managed.fidelity = Fidelity::Partial;
        let mut delegate = candidate(Strategy::Delegate);
        delegate.confidence = 1.0;
        delegate.fidelity = Fidelity::Native;
        let plans = plan(vec![delegate, managed], "windows", Policy::default());
        assert_eq!(plans.candidates[0].strategy, strategy);
        assert!(matches!(
            plans.question,
            Some(Question::LowConfidence { .. })
        ));
    }
}

#[test]
fn extraction_override_keeps_native_fallbacks() {
    let policy = Policy {
        windows_method: WindowsMethod::Extract,
        ..Policy::default()
    };
    let plans = plan(
        vec![
            candidate(Strategy::RunSilent),
            candidate(Strategy::Extract),
            candidate(Strategy::RunInteractive),
        ],
        "windows",
        policy,
    );
    assert_eq!(
        plans
            .candidates
            .iter()
            .map(|c| c.strategy)
            .collect::<Vec<_>>(),
        vec![
            Strategy::Extract,
            Strategy::RunSilent,
            Strategy::RunInteractive
        ]
    );
}

#[test]
fn default_allow_never_asks_for_execution_or_elevation() {
    let mut proposed = candidate(Strategy::Delegate);
    proposed.requires.may_prompt_elevation = true;
    let plans = plan(vec![proposed], "windows", Policy::default());
    assert_eq!(plans.selected, Some(0));
    assert!(plans.question.is_none());
    let policy: Policy = serde_json::from_str("{}").unwrap();
    assert_eq!(policy.execution, ExecutionPolicy::Allow);
    assert!(policy.preset_args.no_shortcuts);
    assert!(policy.preset_args.language.is_none());
}

#[test]
fn low_confidence_is_strictly_below_threshold() {
    for (confidence, ask) in [(0.0, true), (0.49, true), (0.5, false), (1.0, false)] {
        let mut proposed = candidate(Strategy::Extract);
        proposed.confidence = confidence;
        let plans = plan(vec![proposed], "windows", Policy::default());
        assert_eq!(plans.question.is_some(), ask);
        assert_eq!(plans.selected.is_none(), ask);
    }
}

fn run_step(runtime: RuntimeSpec) -> Step {
    Step::Run {
        program: Program::Source(SourcePath::Artifact {
            artifact: ArtifactId("setup".into()),
            path: ".".into(),
        }),
        args: vec![],
        env: BTreeMap::new(),
        runtime,
        timeout_secs: 30,
        expect: vec![],
    }
}

#[test]
fn execution_never_checks_steps_and_retains_safe_fallbacks() {
    let mut hidden = candidate(Strategy::Extract);
    hidden.steps.push(run_step(RuntimeSpec::Native));
    let policy = Policy {
        execution: ExecutionPolicy::Never,
        ..Policy::default()
    };
    let plans = plan(
        vec![
            candidate(Strategy::RunSilent),
            hidden,
            candidate(Strategy::Extract),
            candidate(Strategy::Delegate),
        ],
        "windows",
        policy,
    );
    assert_eq!(plans.candidates.len(), 1);
    assert_eq!(plans.candidates[0].strategy, Strategy::Extract);
    assert_eq!(plans.selected, Some(0));
}

#[test]
fn execution_ask_only_questions_selected_executable_candidate() {
    let policy = Policy {
        execution: ExecutionPolicy::Ask,
        ..Policy::default()
    };
    let plans = plan(
        vec![candidate(Strategy::RunSilent), candidate(Strategy::Extract)],
        "windows",
        policy.clone(),
    );
    assert!(matches!(
        plans.question,
        Some(Question::ConfirmExecution { .. })
    ));
    assert_eq!(plans.selected, None);
    let plans = plan(
        vec![candidate(Strategy::RunSilent), candidate(Strategy::Extract)],
        "linux",
        policy,
    );
    assert_eq!(plans.selected, Some(0));
    assert!(plans.question.is_none());
}

#[test]
fn runtime_requirements_and_steps_are_filtered_for_every_profile() {
    for profile in [
        "windows",
        "linux",
        "linux-no-umu",
        "linux-exfat-root",
        "macos",
        "macos-no-wine",
    ] {
        let caps = host(profile);
        for (runtime, supported) in [
            (RuntimeSpec::Native, caps.os == OperatingSystem::Windows),
            (
                RuntimeSpec::ProtonPrefix { id: "game".into() },
                caps.has_runner(crate::host::RunnerKind::Proton),
            ),
            (
                RuntimeSpec::WinePrefix { id: "game".into() },
                caps.has_runner(crate::host::RunnerKind::Wine),
            ),
        ] {
            for use_steps in [false, true] {
                let mut proposed = candidate(Strategy::RunSilent);
                if use_steps {
                    proposed.steps.push(run_step(runtime.clone()));
                } else {
                    proposed.requires.runtime = Some(runtime.clone());
                }
                let plans = plan(vec![proposed], profile, Policy::default());
                assert_eq!(
                    !plans.candidates.is_empty(),
                    supported,
                    "{profile}: {runtime:?}"
                );
            }
        }
        assert_eq!(
            plan(
                vec![candidate(Strategy::Extract)],
                profile,
                Policy::default()
            )
            .selected,
            Some(0)
        );
    }
}

#[test]
fn foreign_native_payloads_are_unsupported_but_handoff_is_portable() {
    let handler = engine(Proposal {
        candidates: vec![candidate(Strategy::Extract)],
        question: None,
    });
    for platform in [PayloadPlatform::Linux, PayloadPlatform::Macos] {
        let mut node = artifact();
        node.platform = platform;
        assert!(
            handler
                .plan(&[node], &host("windows"), &Policy::default())
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    let mut caps = host("linux");
    caps.filesystem.writable = false;
    let plans = engine(Proposal {
        candidates: vec![candidate(Strategy::HandOff)],
        question: None,
    })
    .plan(&[artifact()], &caps, &Policy::default())
    .unwrap();
    assert_eq!(plans.selected, Some(0));
}

#[test]
fn filesystem_and_sidecar_capabilities_are_checked() {
    let mut caps = host("linux");
    caps.filesystem.symlinks = false;
    caps.filesystem.exec_bits = false;
    caps.seven_zip = None;
    for step in [
        Step::Symlink {
            path: "link".into(),
            target: "game".into(),
        },
        Step::SetExecutable {
            path: "game".into(),
        },
        Step::Run {
            program: Program::Tool("7zz".into()),
            args: vec![],
            env: BTreeMap::new(),
            runtime: RuntimeSpec::Native,
            timeout_secs: 30,
            expect: vec![],
        },
    ] {
        let mut proposed = candidate(Strategy::Extract);
        proposed.steps.push(step);
        assert!(
            engine(Proposal {
                candidates: vec![proposed],
                question: None
            })
            .plan(&[artifact()], &caps, &Policy::default())
            .unwrap()
            .candidates
            .is_empty()
        );
    }
    caps.filesystem.writable = false;
    let planner = engine(Proposal {
        candidates: vec![candidate(Strategy::Extract), candidate(Strategy::Delegate)],
        question: None,
    });
    let plans = planner
        .plan(&[artifact()], &caps, &Policy::default())
        .unwrap();
    assert_eq!(plans.candidates.len(), 1);
    assert_eq!(plans.candidates[0].strategy, Strategy::Delegate);
}

#[test]
fn hidden_delegation_cannot_outrank_managed_installation() {
    let mut hidden = candidate(Strategy::Copy);
    hidden.steps.push(Step::Delegate {
        tool: "installer".into(),
        args: vec![],
        uninstall: vec![],
        only_if: None,
    });
    let plans = plan(
        vec![hidden, candidate(Strategy::Extract)],
        "windows",
        Policy::default(),
    );
    assert_eq!(plans.candidates[0].strategy, Strategy::Extract);
}

#[test]
fn equal_fallbacks_for_one_artifact_are_not_ambiguous() {
    let plans = plan(
        vec![candidate(Strategy::Extract), candidate(Strategy::Extract)],
        "windows",
        Policy::default(),
    );
    assert_eq!(plans.selected, Some(0));
    assert!(plans.question.is_none());
}

#[test]
fn equally_ranked_distinct_installers_require_a_choice() {
    let planner = engine(Proposal {
        candidates: vec![candidate(Strategy::RunSilent), candidate(Strategy::Extract)],
        question: None,
    });
    let mut other = artifact();
    other.id = ArtifactId("other".into());
    let plans = planner
        .plan(&[artifact(), other], &host("windows"), &Policy::default())
        .unwrap();
    assert_eq!(plans.selected, None);
    assert_eq!(plans.candidates.len(), 4);
    assert!(
        matches!(plans.question, Some(Question::ChooseInstaller { artifacts }) if artifacts.len() == 2)
    );
}

#[test]
fn handler_ambiguity_is_retained_without_inventing_candidates() {
    let question = Question::ChooseInstaller {
        artifacts: vec![ArtifactId("a".into()), ArtifactId("b".into())],
    };
    let plans = engine(Proposal {
        candidates: vec![],
        question: Some(question.clone()),
    })
    .plan(&[artifact()], &host("windows"), &Policy::default())
    .unwrap();
    assert_eq!(plans.question, Some(question));
    assert!(plans.candidates.is_empty());
    assert_eq!(plans.selected, None);
}

#[test]
fn questions_from_unavailable_proposals_do_not_leak() {
    let mut proposed = candidate(Strategy::RunSilent);
    proposed.requires.runtime = Some(RuntimeSpec::Native);
    let plans = engine(Proposal {
        candidates: vec![proposed],
        question: Some(Question::ChooseInstaller { artifacts: vec![] }),
    })
    .plan(&[artifact()], &host("linux"), &Policy::default())
    .unwrap();
    assert!(plans.question.is_none());
    assert!(plans.candidates.is_empty());
}

#[test]
fn malformed_confidence_and_spoofed_handler_are_rejected() {
    for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut proposed = candidate(Strategy::Extract);
        proposed.confidence = value;
        assert!(matches!(
            engine(Proposal {
                candidates: vec![proposed],
                question: None
            })
            .plan(&[artifact()], &host("windows"), &Policy::default()),
            Err(PlanError::InvalidConfidence { .. })
        ));
        let policy = Policy {
            ask_below_confidence: value,
            ..Policy::default()
        };
        assert!(matches!(
            PlanEngine::new().plan(&[], &host("windows"), &policy),
            Err(PlanError::InvalidConfidence { .. })
        ));
    }
    let mut proposed = candidate(Strategy::Extract);
    proposed.handler = HandlerId("spoof".into());
    assert!(matches!(
        engine(Proposal {
            candidates: vec![proposed],
            question: None
        })
        .plan(&[artifact()], &host("windows"), &Policy::default()),
        Err(PlanError::HandlerMismatch { .. })
    ));
}

#[test]
fn duplicate_handlers_and_artifact_ids_are_rejected() {
    let mut planner = engine(Proposal::default());
    assert!(matches!(
        planner.register(Box::new(TestHandler(Proposal::default()))),
        Err(PlanError::DuplicateHandler { .. })
    ));
    assert!(matches!(
        planner.plan(
            &[artifact(), artifact()],
            &host("windows"),
            &Policy::default()
        ),
        Err(PlanError::DuplicateArtifact { .. })
    ));
}

#[test]
fn confidence_then_fidelity_break_ties_within_a_strategy() {
    let mut low = candidate(Strategy::Extract);
    low.confidence = 0.6;
    let mut high_partial = candidate(Strategy::Extract);
    high_partial.fidelity = Fidelity::Partial;
    let exact = candidate(Strategy::Extract);
    let plans = plan(
        vec![low, high_partial, exact.clone()],
        "windows",
        Policy::default(),
    );
    assert_eq!(plans.candidates[0], exact);
    assert_eq!(plans.candidates[1].fidelity, Fidelity::Partial);
    assert_eq!(plans.candidates[2].confidence, 0.6);
}

#[test]
fn registry_visits_claimed_children_and_passes_policy_presets() {
    struct PresetHandler;
    impl Handler for PresetHandler {
        fn id(&self) -> HandlerId {
            HandlerId("test".into())
        }
        fn claims(&self, artifact: &ArtifactNode) -> bool {
            artifact.format == Format::Inno
        }
        fn propose(
            &self,
            artifact: &ArtifactNode,
            context: &PlanContext<'_>,
        ) -> Result<Proposal, PlanError> {
            assert_eq!(artifact.id.0, "setup");
            assert_eq!(context.policy.preset_args.language.as_deref(), Some("fr"));
            assert!(!context.policy.preset_args.no_shortcuts);
            assert_eq!(context.host.os, OperatingSystem::Windows);
            assert_eq!(context.target_dir, Some(std::path::Path::new("G:\\")));
            Ok(Proposal {
                candidates: vec![candidate(Strategy::Extract)],
                question: None,
            })
        }
    }
    let mut parent = artifact();
    parent.id = ArtifactId("archive".into());
    parent.format = Format::Zip;
    parent.children = vec![artifact()];
    let mut planner = PlanEngine::new();
    planner.register(Box::new(PresetHandler)).unwrap();
    let policy = Policy {
        preset_args: PresetArgs {
            language: Some("fr".into()),
            no_shortcuts: false,
        },
        ..Policy::default()
    };
    let caps = host("windows");
    let context = PlanContext::new(&caps, &policy).with_target_dir(std::path::Path::new("G:\\"));
    let plans = planner.plan_with_context(&[parent], &context).unwrap();
    assert_eq!(plans.candidates.len(), 1);
}
