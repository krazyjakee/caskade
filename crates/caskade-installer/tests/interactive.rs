use caskade_installer::{
    exec::{interactive::InteractiveConfig, *},
    plan::presets::{InstallerFamily, InstallerMode, SwitchSettings, build_args},
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
fn candidate(spec: RuntimeSpec, args: Vec<String>) -> Candidate {
    Candidate {
        handler: HandlerId("interactive-test".into()),
        strategy: Strategy::RunInteractive,
        confidence: 1.0,
        fidelity: Fidelity::Equivalent,
        requires: Requirements {
            executes_code: true,
            runtime: Some(spec.clone()),
            may_prompt_elevation: false,
            interactive: true,
            est_disk_bytes: None,
        },
        steps: vec![Step::RunInteractive {
            program: Program::Tool("setup.exe".into()),
            args,
            env: BTreeMap::new(),
            runtime: spec,
            target_dir: ".".into(),
        }],
        warnings: vec![],
    }
}
fn request(root: &TestRoot, candidates: Vec<Candidate>) -> ExecRequest {
    ExecRequest {
        job_id: "interactive-job".into(),
        root: root.path().into(),
        relative_dir: "Game".into(),
        plans: PlanSet {
            candidates,
            selected: Some(0),
            question: None,
        },
        sources: BTreeMap::new(),
    }
}
fn config() -> ExecConfig {
    ExecConfig {
        disk_margin_bytes: 0,
        interactive: InteractiveConfig {
            expect: BTreeMap::from([(
                HandlerId("interactive-test".into()),
                vec!["game.exe".into()],
            )]),
            settle: Duration::ZERO,
            ..InteractiveConfig::default()
        },
        ..ExecConfig::default()
    }
}
fn executor(runtime: Arc<FakeRuntime>, config: ExecConfig) -> Executor {
    Executor::new(
        runtime,
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_config(config)
}
fn script(path: impl Into<PathBuf>) -> FakeScript {
    FakeScript {
        actions: vec![FakeAction::WriteFile {
            path: path.into(),
            contents: b"fixture payload".to_vec(),
        }],
    }
}
#[tokio::test]
async fn every_family_keeps_accepted_interactive_presets_on_native_proton_and_wine() {
    use InstallerFamily::*;
    for family in [
        Inno,
        Nsis,
        Msi,
        Burn,
        InstallShield,
        Wise,
        SetupFactory,
        Custom,
    ] {
        for spec in [
            RuntimeSpec::Native,
            RuntimeSpec::ProtonPrefix { id: "main".into() },
            RuntimeSpec::WinePrefix { id: "main".into() },
        ] {
            let root = TestRoot::new().unwrap();
            let args = build_args(
                family,
                InstallerMode::Interactive,
                &SwitchSettings {
                    language: Some("english".into()),
                    ..SwitchSettings::default()
                },
            )
            .unwrap()
            .args;
            let runtime = Arc::new(FakeRuntime::new([script("game.exe")]));
            let mut exec = executor(runtime.clone(), config());
            exec.register_runtime(spec.clone(), runtime.clone());
            let report = exec
                .execute(
                    request(&root, vec![candidate(spec.clone(), args.clone())]),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            let directory = if spec == RuntimeSpec::Native {
                report.dir.to_str().unwrap()
            } else {
                caskade_installer::runtime::proton::GAME_DIR
            };
            assert_eq!(
                runtime.processes()[0].command.args,
                args.iter()
                    .map(|arg| arg.replace(INSTALL_DIR, directory))
                    .collect::<Vec<_>>()
            );
            assert!(runtime.processes().iter().all(|p| p.killed));
            std::fs::remove_dir_all(report.dir).unwrap();
            root.assert_clean();
        }
    }
}
#[tokio::test]
async fn expected_paths_come_from_matching_silent_candidate_and_are_checked() {
    let root = TestRoot::new().unwrap();
    let mut silent = candidate(RuntimeSpec::Native, vec![]);
    silent.strategy = Strategy::RunSilent;
    silent.steps = vec![Step::Run {
        program: Program::Tool("setup.exe".into()),
        args: vec![],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 30,
        expect: vec!["game.exe".into()],
    }];
    let runtime = Arc::new(FakeRuntime::new([script("game.exe")]));
    let mut cfg = config();
    cfg.interactive.expect.clear();
    let report = executor(runtime, cfg)
        .execute(
            request(&root, vec![candidate(RuntimeSpec::Native, vec![]), silent]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    std::fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}
#[tokio::test]
async fn newly_observed_root_folder_is_reported_and_never_triggers_fallback() {
    let root = TestRoot::new().unwrap();
    let elsewhere = root.path().join("ChosenByInstaller");
    let runtime = Arc::new(FakeRuntime::new([
        script(elsewhere.join("game.exe")),
        script("fallback.exe"),
    ]));
    let result = executor(runtime.clone(), config())
        .execute(
            request(
                &root,
                vec![
                    candidate(RuntimeSpec::Native, vec![]),
                    candidate(RuntimeSpec::Native, vec![]),
                ],
            ),
            CancellationToken::new(),
        )
        .await;
    let Err(ExecError::NeedsUser {
        question: Question::ChooseInstallDirectory { paths, .. },
    }) = result
    else {
        panic!("{result:?}")
    };
    assert_eq!(paths, vec![elsewhere.clone()]);
    assert_eq!(runtime.processes().len(), 1);
    assert!(!root.path().join("Game").exists());
    std::fs::remove_dir_all(elsewhere).unwrap();
    root.assert_clean();
}
#[tokio::test]
async fn outside_explicit_scope_is_preserved_but_preexisting_folder_is_not_claimed() {
    for preexisting in [false, true] {
        let root = TestRoot::new().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let chosen = outside.path().join("Chosen");
        if preexisting {
            std::fs::create_dir(&chosen).unwrap();
        }
        let runtime = Arc::new(FakeRuntime::new([script(chosen.join("game.exe"))]));
        let mut cfg = config();
        cfg.interactive
            .outside_roots
            .push(std::fs::canonicalize(outside.path()).unwrap());
        let result = executor(runtime, cfg)
            .execute(
                request(&root, vec![candidate(RuntimeSpec::Native, vec![])]),
                CancellationToken::new(),
            )
            .await;
        let Err(ExecError::NeedsUser {
            question: Question::ChooseInstallDirectory { paths, .. },
        }) = result
        else {
            panic!("{result:?}")
        };
        assert_eq!(paths, if preexisting { vec![] } else { vec![chosen] });
        root.assert_clean();
    }
}
#[tokio::test]
async fn prefix_outside_game_payload_is_retained_under_confined_recovery_before_rollback() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([script(
        ".caskade/prefix/drive_c/Program Files/Chosen/game.exe",
    )]));
    let spec = RuntimeSpec::ProtonPrefix { id: "main".into() };
    let mut exec = executor(runtime.clone(), config());
    exec.register_runtime(spec.clone(), runtime);
    let result = exec
        .execute(
            request(&root, vec![candidate(spec, vec![])]),
            CancellationToken::new(),
        )
        .await;
    let Err(ExecError::NeedsUser {
        question: Question::ChooseInstallDirectory { paths, .. },
    }) = result
    else {
        panic!("{result:?}")
    };
    assert_eq!(
        paths,
        vec![
            root.path()
                .join(".caskade/interactive/interactive-job/payload-0")
        ]
    );
    assert!(paths[0].join("game.exe").is_file());
    assert!(!root.path().join("Game").exists());
    std::fs::remove_dir_all(root.path().join(".caskade")).unwrap();
    root.assert_clean();
}
#[tokio::test]
async fn success_without_expected_payload_yields_a_decision_and_cleans_owned_staging() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([script("readme.txt")]));
    assert!(matches!(
        executor(runtime, config())
            .execute(
                request(&root, vec![candidate(RuntimeSpec::Native, vec![])]),
                CancellationToken::new()
            )
            .await,
        Err(ExecError::NeedsUser { .. })
    ));
    root.assert_clean();
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_expectations_cannot_claim_an_outside_payload() {
    let root = TestRoot::new().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("game.exe"), b"sentinel").unwrap();
    let selected = root.path().join("Selected");
    std::os::unix::fs::symlink(outside.path(), &selected).unwrap();
    let runtime = Arc::new(FakeRuntime::new([script("readme.txt")]));
    let result = executor(runtime, config())
        .execute(
            request(&root, vec![candidate(RuntimeSpec::Native, vec![])]),
            CancellationToken::new(),
        )
        .await;
    assert!(
        matches!(result,Err(ExecError::NeedsUser{question:Question::ChooseInstallDirectory{paths,..}}) if paths.is_empty())
    );
    assert_eq!(
        std::fs::read(outside.path().join("game.exe")).unwrap(),
        b"sentinel"
    );
    std::fs::remove_file(selected).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn cancellation_kills_interactive_descendants_and_rolls_back_without_a_decision() {
    let root = TestRoot::new().unwrap();
    let token = CancellationToken::new();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![
            FakeAction::WriteFile {
                path: "partial".into(),
                contents: vec![1],
            },
            FakeAction::Hang,
        ],
    }]));
    struct CancelOnStart(CancellationToken);
    impl caskade_installer::events::EventSink for CancelOnStart {
        fn emit(&self, event: caskade_installer::events::JobEvent) {
            if event.message == "process_started" {
                self.0.cancel();
            }
        }
    }
    let exec = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(CancelOnStart(token.clone())),
    )
    .with_config(config());
    let result = exec
        .execute(
            request(&root, vec![candidate(RuntimeSpec::Native, vec![])]),
            token,
        )
        .await;
    assert!(matches!(result, Err(ExecError::Cancelled)), "{result:?}");
    assert!(runtime.processes().iter().all(|process| process.killed));
    root.assert_clean();
}
#[tokio::test]
async fn inventory_limits_reject_before_launching_an_installer() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([script("game.exe")]));
    let mut cfg = config();
    cfg.interactive.max_entries = 1;
    assert!(
        executor(runtime.clone(), cfg)
            .execute(
                request(&root, vec![candidate(RuntimeSpec::Native, vec![])]),
                CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(runtime.processes().is_empty());
    root.assert_clean();
}
