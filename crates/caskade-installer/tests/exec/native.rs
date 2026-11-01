//! Native installer runs: fallback, effects journal, compensation, arguments.
use super::*;

#[tokio::test]
async fn native_failures_timeout_and_missing_expected_outputs_fall_back_in_order() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([
        FakeScript {
            actions: vec![
                FakeAction::WriteFile {
                    path: "failed.txt".into(),
                    contents: b"bad".to_vec(),
                },
                FakeAction::Exit { code: 42 },
            ],
        },
        FakeScript {
            actions: vec![FakeAction::Hang],
        },
        FakeScript {
            actions: vec![FakeAction::Exit { code: 0 }],
        },
    ]));
    let mut job = request(
        &root,
        vec![
            candidate(vec![run(vec![])]),
            candidate(vec![run(vec![])]),
            candidate(vec![run(vec!["missing.exe".into()])]),
            candidate(vec![Step::WriteFile {
                path: "good.bin".into(),
                contents: b"ok".to_vec(),
            }]),
        ],
    );
    job.sources
        .insert(ArtifactId("setup".into()), PathBuf::from("/fake/setup.exe"));
    let report = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .execute(job, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.candidate_index, 3);
    assert_eq!(report.failures.len(), 3);
    assert!(!report.dir.join("failed.txt").exists());
    assert_eq!(fs::read(report.dir.join("good.bin")).unwrap(), b"ok");
    for process in runtime.processes() {
        assert_eq!(
            process.command.cwd, report.dir,
            "native installers use final folder"
        );
        assert_eq!(
            process.command.args,
            vec![format!("/DIR={}", report.dir.display()), "/LANG=fr".into()]
        );
        assert!(process.finished && process.killed);
    }
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn native_out_of_root_effects_are_journalled_and_rolled_back() {
    for existing in [false, true] {
        let root = TestRoot::new().unwrap();
        let external = root.parent_path().join("external-file");
        if existing {
            fs::write(&external, b"original").unwrap();
        }
        let runtime = Arc::new(FakeRuntime::new([FakeScript {
            actions: vec![
                FakeAction::WriteFile {
                    path: external.clone(),
                    contents: b"changed".to_vec(),
                },
                FakeAction::Exit { code: 42 },
            ],
        }]));
        let observer = Arc::new(ExternalFileObserver {
            path: external.clone(),
            original: existing.then(|| b"original".to_vec()),
        });
        let worker = executor(runtime, Arc::new(RecordingSink::default())).with_observer(observer);
        let mut job = request(&root, vec![candidate(vec![run(vec![])])]);
        job.sources
            .insert(ArtifactId("setup".into()), "/fake/setup.exe".into());
        assert!(worker.execute(job, CancellationToken::new()).await.is_err());
        if existing {
            assert_eq!(fs::read(&external).unwrap(), b"original");
            fs::remove_file(external).unwrap();
        } else {
            assert!(!external.exists());
        }
        root.assert_clean();
    }
}

fn delegate(name: &str) -> Step {
    Step::Delegate {
        tool: "apt".into(),
        args: vec!["install".into(), name.into()],
        uninstall: vec![DelegateCommand {
            tool: "apt".into(),
            args: vec!["remove".into(), name.into()],
            only_if: None,
        }],
        only_if: None,
    }
}

#[tokio::test]
async fn delegate_uninstall_compensation_runs_only_after_the_delegate_succeeded() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([
        FakeScript {
            actions: vec![FakeAction::Exit { code: 0 }],
        },
        FakeScript {
            actions: vec![FakeAction::Exit { code: 42 }],
        },
        FakeScript {
            actions: vec![FakeAction::Exit { code: 0 }],
        },
    ]));
    let result = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .execute(
            request(
                &root,
                vec![candidate(vec![delegate("first"), delegate("second")])],
            ),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    // The failed install may have left an older "second" in place; only
    // "first", which installed, is removed.
    let args: Vec<_> = runtime
        .processes()
        .into_iter()
        .map(|process| process.command.args)
        .collect();
    assert_eq!(
        args,
        [
            vec!["install", "first"],
            vec!["install", "second"],
            vec!["remove", "first"]
        ]
    );
    root.assert_clean();
}

#[tokio::test]
async fn delegate_compensation_is_skipped_once_its_check_finds_nothing_to_undo() {
    for (installed, expected) in [(1, None), (0, Some(vec!["remove", "first"]))] {
        let root = TestRoot::new().unwrap();
        let runtime = Arc::new(FakeRuntime::new([0, 42, installed, 0].map(|code| {
            FakeScript {
                actions: vec![FakeAction::Exit { code }],
            }
        })));
        let checked = |name: &str| {
            let Step::Delegate {
                tool,
                args,
                mut uninstall,
                only_if,
            } = delegate(name)
            else {
                unreachable!()
            };
            uninstall[0].only_if = Some(DelegateCheck {
                tool: "dpkg-query".into(),
                args: vec!["--status".into(), "--".into(), name.into()],
            });
            Step::Delegate {
                tool,
                args,
                uninstall,
                only_if,
            }
        };
        let result = executor(runtime.clone(), Arc::new(RecordingSink::default()))
            .execute(
                request(
                    &root,
                    vec![candidate(vec![checked("first"), checked("second")])],
                ),
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_err());
        let args: Vec<_> = runtime
            .processes()
            .into_iter()
            .map(|process| process.command.args)
            .collect();
        let mut want = vec![
            vec!["install", "first"],
            vec!["install", "second"],
            vec!["--status", "--", "first"],
        ];
        want.extend(expected);
        assert_eq!(args, want);
        root.assert_clean();
    }
}

struct WatchLimitObserver;

impl ProcessObserver for WatchLimitObserver {
    fn start(
        &self,
        _: &ProcessCommand,
        _: &JournalHandle,
        _: &StepContext,
    ) -> exec::Result<Box<dyn ProcessObservation>> {
        Err(ExecError::Io(std::io::Error::other("watch limit reached")))
    }
}

#[tokio::test]
async fn delegates_are_not_watched_outside_the_root() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 0 }],
    }]));
    let report = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .with_observer(Arc::new(WatchLimitObserver))
        .execute(
            request(&root, vec![candidate(vec![delegate("game")])]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(runtime.processes().len(), 1);
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn cancellation_at_every_native_event_rolls_back_effects_and_kills_descendants() {
    let root = TestRoot::new().unwrap();
    let external = root.parent_path().join("external-file");
    let steps = vec![
        Step::WriteFile {
            path: "config.ini".into(),
            contents: b"config".to_vec(),
        },
        run(vec!["game.exe".into()]),
    ];
    let script = FakeScript {
        actions: vec![
            FakeAction::WriteFile {
                path: external.clone(),
                contents: b"external".to_vec(),
            },
            FakeAction::WriteFile {
                path: "game.exe".into(),
                contents: b"game".to_vec(),
            },
            FakeAction::SpawnChild {
                command: ProcessCommand {
                    program: "child".into(),
                    args: vec![],
                    env: BTreeMap::new(),
                    cwd: root.path().join("Fixture Game"),
                },
                script: FakeScript {
                    actions: vec![FakeAction::Hang],
                },
            },
            FakeAction::Exit { code: 0 },
        ],
    };
    let job = || {
        let mut request = request(&root, vec![candidate(steps.clone())]);
        request
            .sources
            .insert(ArtifactId("setup".into()), "/fake/setup.exe".into());
        request
    };
    let observer = Arc::new(ExternalFileObserver {
        path: external.clone(),
        original: None,
    });
    let baseline = Arc::new(RecordingSink::default());
    let report = executor(
        Arc::new(FakeRuntime::new([script.clone()])),
        baseline.clone(),
    )
    .with_observer(observer.clone())
    .execute(job(), CancellationToken::new())
    .await
    .unwrap();
    assert_eq!(
        report.journal.len(),
        1,
        "successful side effects reach the manifest boundary"
    );
    fs::remove_dir_all(report.dir).unwrap();
    fs::remove_file(&external).unwrap();
    for index in 0..baseline.events().len() {
        let token = CancellationToken::new();
        let sink = Arc::new(CancelAt {
            events: RecordingSink::default(),
            token: token.clone(),
            index,
            count: AtomicUsize::new(0),
        });
        let runtime = Arc::new(FakeRuntime::new([script.clone()]));
        let result = executor(runtime.clone(), sink)
            .with_observer(observer.clone())
            .execute(job(), token)
            .await;
        assert!(
            matches!(result, Err(ExecError::Cancelled)),
            "native event {index}: {result:?}"
        );
        assert!(
            runtime
                .processes()
                .iter()
                .all(|process| process.finished && process.killed)
        );
        root.assert_clean();
    }
}

struct WaitUntilKilled {
    entered: tokio::sync::Notify,
    killed: (Mutex<bool>, std::sync::Condvar),
}

impl runtime::Runtime for WaitUntilKilled {
    fn spawn(
        &self,
        command: &ProcessCommand,
    ) -> std::result::Result<runtime::ProcessId, runtime::RuntimeError> {
        fs::write(command.cwd.join("partial.exe"), b"partial").unwrap();
        Ok(runtime::ProcessId(1))
    }
    fn wait(
        &self,
        _: runtime::ProcessId,
        _: Duration,
    ) -> std::result::Result<runtime::ProcessExit, runtime::RuntimeError> {
        self.entered.notify_one();
        let (lock, condvar) = &self.killed;
        let mut killed = lock.lock().unwrap();
        while !*killed {
            killed = condvar.wait(killed).unwrap();
        }
        Err(runtime::RuntimeError::Killed {
            id: runtime::ProcessId(1),
        })
    }
    fn kill(&self, _: runtime::ProcessId) -> std::result::Result<(), runtime::RuntimeError> {
        let (lock, condvar) = &self.killed;
        *lock.lock().unwrap() = true;
        condvar.notify_all();
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_interrupts_a_running_process_wait() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(WaitUntilKilled {
        entered: tokio::sync::Notify::new(),
        killed: (Mutex::new(false), std::sync::Condvar::new()),
    });
    let worker = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    );
    let mut job = request(&root, vec![candidate(vec![run(vec![])])]);
    job.sources
        .insert(ArtifactId("setup".into()), "/fake/setup.exe".into());
    let token = CancellationToken::new();
    let cancel = token.clone();
    let task = tokio::spawn(async move { worker.execute(job, token).await });
    runtime.entered.notified().await;
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(ExecError::Cancelled)));
    assert!(*runtime.killed.0.lock().unwrap());
    root.assert_clean();
}

struct UninstallerObserver;

impl ProcessObserver for UninstallerObserver {
    fn start(
        &self,
        command: &ProcessCommand,
        journal: &JournalHandle,
        _: &StepContext,
    ) -> exec::Result<Box<dyn ProcessObservation>> {
        journal.record(JournalEntry::Compensation {
            runtime: RuntimeSpec::Native,
            command: ProcessCommand {
                program: command.cwd.join("unins000.exe"),
                args: vec!["/VERYSILENT".into()],
                env: BTreeMap::new(),
                cwd: command.cwd.clone(),
            },
            only_if: None,
        })?;
        Ok(Box::new(NoopObservation))
    }
}

#[tokio::test]
async fn native_rollback_invokes_the_registered_uninstaller_before_removing_the_folder() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([
        FakeScript {
            actions: vec![
                FakeAction::WriteFile {
                    path: "unins000.exe".into(),
                    contents: vec![],
                },
                FakeAction::Exit { code: 42 },
            ],
        },
        FakeScript {
            actions: vec![FakeAction::Exit { code: 0 }],
        },
    ]));
    let mut job = request(&root, vec![candidate(vec![run(vec![])])]);
    job.sources
        .insert(ArtifactId("setup".into()), "/fake/setup.exe".into());
    let result = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .with_observer(Arc::new(UninstallerObserver))
        .execute(job, CancellationToken::new())
        .await;
    assert!(result.is_err());
    let processes = runtime.processes();
    assert_eq!(processes.len(), 2);
    assert_eq!(
        processes[1].command.program,
        root.path().join("Fixture Game/unins000.exe")
    );
    assert_eq!(processes[1].command.args, vec!["/VERYSILENT"]);
    root.assert_clean();
}

#[tokio::test]
async fn msi_host_tool_installs_in_the_final_folder() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::WriteFile {
            path: "game.exe".into(),
            contents: b"game".to_vec(),
        }],
    }]));
    let mut proposal = candidate(vec![Step::Run {
        program: Program::Tool("msiexec".into()),
        args: vec![
            "/i".into(),
            "setup.msi".into(),
            format!("INSTALLDIR={INSTALL_DIR}"),
        ],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 30,
        expect: vec!["game.exe".into()],
    }]);
    proposal.strategy = Strategy::RunSilent;
    let report = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .execute(request(&root, vec![proposal]), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(runtime.processes()[0].command.cwd, report.dir);
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[tokio::test]
async fn source_arguments_resolve_whole_tokens_without_shell_or_substring_expansion() {
    for delegate in [false, true] {
        let root = TestRoot::new().unwrap();
        let id = ArtifactId("source:0".into());
        let token = exec::source_argument(&id);
        let args = vec![
            token.clone(),
            format!("prefix={token}"),
            "$(echo untouched)".into(),
        ];
        let runtime = Arc::new(FakeRuntime::new([FakeScript {
            actions: vec![FakeAction::Exit { code: 0 }],
        }]));
        let step = if delegate {
            Step::Delegate {
                tool: "package-tool".into(),
                args,
                uninstall: vec![DelegateCommand {
                    tool: "undo".into(),
                    args: vec![token.clone()],
                    only_if: None,
                }],
                only_if: None,
            }
        } else {
            Step::Run {
                program: Program::Tool("msiexec".into()),
                args,
                env: BTreeMap::new(),
                runtime: RuntimeSpec::Native,
                timeout_secs: 30,
                expect: vec![],
            }
        };
        let mut job = request(&root, vec![candidate(vec![step])]);
        let picked = root.path().join("picked 空白 setup $(literal).msi");
        job.sources.insert(id, picked.clone());
        let worker = executor(runtime.clone(), Arc::new(RecordingSink::default()));
        let report = worker
            .execute(job.clone(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            runtime.processes()[0].command.args,
            vec![
                picked.to_str().unwrap(),
                &format!("prefix={token}"),
                "$(echo untouched)"
            ]
        );
        fs::remove_dir_all(report.dir).unwrap();
        job.sources.clear();
        assert!(worker.execute(job, CancellationToken::new()).await.is_err());
        assert_eq!(runtime.processes().len(), 1);
        root.assert_clean();
    }
}

#[tokio::test]
async fn relative_source_map_arguments_are_absolute_even_when_the_process_cwd_differs() {
    let root = TestRoot::new().unwrap();
    let id = ArtifactId("relative-package".into());
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::Exit { code: 0 }],
    }]));
    let mut job = request(
        &root,
        vec![candidate(vec![Step::Run {
            program: Program::Tool("dnf".into()),
            args: vec!["install".into(), exec::source_argument(&id)],
            env: BTreeMap::new(),
            runtime: RuntimeSpec::Native,
            timeout_secs: 30,
            expect: vec![],
        }])],
    );
    job.sources.insert(id, "fixture packages/game.rpm".into());
    let report = executor(runtime.clone(), Arc::new(RecordingSink::default()))
        .execute(job, CancellationToken::new())
        .await
        .unwrap();
    let processes = runtime.processes();
    let arg = Path::new(&processes[0].command.args[1]);
    assert!(arg.is_absolute());
    assert!(arg.starts_with(std::env::current_dir().unwrap()));
    assert!(arg.ends_with("fixture packages/game.rpm"));
    assert_ne!(arg.parent().unwrap(), processes[0].command.cwd);
    fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}
