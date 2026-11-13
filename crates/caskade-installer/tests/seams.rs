use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Seek, SeekFrom},
    time::Duration,
};

use caskade_installer::{
    disk::DiskSpace,
    events::{EventSink, JobEvent, Phase},
    host::{HostCaps, RunnerKind},
    probe::CountingReader,
    runtime::{ProcessCommand, ProcessId, Runtime, RuntimeError},
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};

fn profile(name: &str) -> HostCaps {
    caskade_installer::pipeline::input::host(Some(name)).unwrap()
}

fn command(root: &TestRoot) -> ProcessCommand {
    ProcessCommand {
        program: "setup.exe".into(),
        args: vec!["/S".into(), "/D=C:\\Games\\游戏 with spaces".into()],
        env: BTreeMap::from([("LANG".into(), "en_US.UTF-8".into())]),
        cwd: root.path().into(),
    }
}

#[test]
fn injected_windows_profile_constructs_plan_on_any_host() {
    // Honest construction against injected capabilities, not the #11 planner.
    let host = profile("windows");
    assert_eq!(host.os, OperatingSystem::Windows);
    assert!(host.has_runner(RunnerKind::Native));
    let runtime = RuntimeSpec::Native;
    let plan = InstallPlan {
        schema_version: 1,
        input: Input::Path("setup.exe".into()),
        artifacts: vec![],
        root_id: "test-root".into(),
        relative_dir: "Fixture Game".into(),
        candidate: Candidate {
            handler: HandlerId("test.windows".into()),
            strategy: Strategy::RunSilent,
            confidence: 1.0,
            fidelity: Fidelity::Native,
            requires: Requirements {
                executes_code: true,
                runtime: Some(runtime.clone()),
                may_prompt_elevation: true,
                interactive: false,
                est_disk_bytes: Some(1024),
            },
            steps: vec![Step::Run {
                program: Program::Source(SourcePath::Artifact {
                    artifact: ArtifactId("setup".into()),
                    path: ".".into(),
                }),
                args: vec!["/S".into()],
                env: BTreeMap::new(),
                runtime,
                timeout_secs: 60,
                expect: vec!["Game.exe".into()],
            }],
            warnings: vec![],
        },
    };
    assert_eq!(plan.candidate.strategy, Strategy::RunSilent);
    let encoded = serde_json::to_string(&plan).unwrap();
    assert_eq!(serde_json::from_str::<InstallPlan>(&encoded).unwrap(), plan);
    assert_eq!(
        serde_json::from_str::<HostCaps>(&serde_json::to_string(&host).unwrap()).unwrap(),
        host
    );
}

#[test]
fn six_host_profiles_model_capability_differences() {
    for name in [
        "windows",
        "linux",
        "linux-no-umu",
        "linux-exfat-root",
        "macos",
        "macos-no-wine",
    ] {
        let host = profile(name);
        assert!(host.has_runner(RunnerKind::Native));
        assert!(host.seven_zip.is_some());
    }
    assert!(profile("linux").has_runner(RunnerKind::Proton));
    assert!(!profile("linux-no-umu").has_runner(RunnerKind::Proton));
    assert!(!profile("linux-exfat-root").filesystem.symlinks);
    assert!(!profile("linux-exfat-root").filesystem.exec_bits);
    assert!(profile("macos").has_runner(RunnerKind::Wine));
    assert!(!profile("macos-no-wine").has_runner(RunnerKind::Wine));
}

#[test]
fn fake_runtime_success_failure_and_exact_invocation() {
    let root = TestRoot::new().unwrap();
    let runtime = FakeRuntime::new([
        FakeScript {
            actions: vec![
                FakeAction::WriteFile {
                    path: "游戏/data.bin".into(),
                    contents: vec![0, 0xff],
                },
                FakeAction::Exit { code: 0 },
            ],
        },
        FakeScript {
            actions: vec![FakeAction::Exit { code: 42 }],
        },
    ]);
    let invocation = command(&root);
    let success = runtime.spawn(&invocation).unwrap();
    assert_eq!(
        runtime.wait(success, Duration::from_secs(2)).unwrap().code,
        0
    );
    let failure = runtime.spawn(&invocation).unwrap();
    assert_eq!(
        runtime.wait(failure, Duration::from_secs(2)).unwrap().code,
        42
    );
    let calls = runtime.processes();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].command, invocation);
    assert_eq!(
        fs::read(root.path().join("游戏/data.bin")).unwrap(),
        [0, 0xff]
    );
    root.assert_sentinel_untouched();
    fs::remove_dir_all(root.path().join("游戏")).unwrap();
    root.assert_clean();
    assert!(matches!(
        runtime.spawn(&invocation),
        Err(RuntimeError::NoScript { .. })
    ));
}

#[test]
fn fake_timeout_kills_parent_and_child_without_wall_clock_wait() {
    let root = TestRoot::new().unwrap();
    let runtime = FakeRuntime::new([FakeScript {
        actions: vec![
            FakeAction::SpawnChild {
                command: command(&root),
                script: FakeScript {
                    actions: vec![FakeAction::Hang],
                },
            },
            FakeAction::Hang,
        ],
    }]);
    let process = runtime.spawn(&command(&root)).unwrap();
    assert_eq!(
        runtime.wait(process, Duration::from_secs(3600)),
        Err(RuntimeError::TimedOut { id: process })
    );
    let records = runtime.processes();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].parent, Some(process));
    assert!(
        records
            .iter()
            .all(|record| record.killed && record.finished)
    );
    assert_eq!(
        runtime.wait(records[1].id, Duration::ZERO),
        Err(RuntimeError::Killed { id: records[1].id })
    );
    runtime.kill(process).unwrap(); // Idempotent termination.
    root.assert_clean();
}

#[test]
fn explicit_kill_reaches_children_after_parent_exit() {
    let root = TestRoot::new().unwrap();
    let runtime = FakeRuntime::new([FakeScript {
        actions: vec![
            FakeAction::SpawnChild {
                command: command(&root),
                script: FakeScript {
                    actions: vec![FakeAction::Hang],
                },
            },
            FakeAction::Exit { code: 0 },
        ],
    }]);
    let process = runtime.spawn(&command(&root)).unwrap();
    runtime.wait(process, Duration::from_secs(1)).unwrap();
    runtime.kill(process).unwrap();
    assert!(runtime.processes().iter().all(|record| record.killed));
    assert!(matches!(
        runtime.kill(ProcessId(999)),
        Err(RuntimeError::UnknownProcess { .. })
    ));
    assert!(matches!(
        runtime.wait(ProcessId(999), Duration::ZERO),
        Err(RuntimeError::UnknownProcess { .. })
    ));
}

#[test]
fn fake_runtime_models_outside_writes_and_testroot_detects_them() {
    let root = TestRoot::new().unwrap();
    let runtime = FakeRuntime::new([FakeScript {
        actions: vec![
            FakeAction::WriteFile {
                path: root.sentinel_path().into(),
                contents: b"tampered".to_vec(),
            },
            FakeAction::WriteFile {
                path: "../outside.txt".into(),
                contents: b"outside".to_vec(),
            },
        ],
    }]);
    let process = runtime.spawn(&command(&root)).unwrap();
    assert_eq!(
        runtime.wait(process, Duration::from_secs(1)).unwrap().code,
        0
    );
    assert!(std::panic::catch_unwind(|| root.assert_sentinel_untouched()).is_err());
    fs::write(root.sentinel_path(), b"caskade-test-sentinel\n").unwrap();
    assert!(std::panic::catch_unwind(|| root.assert_clean()).is_err());
    fs::remove_file(root.parent_path().join("outside.txt")).unwrap();
    root.assert_clean();
}

#[test]
fn failed_script_io_leaves_no_live_fake_processes() {
    let root = TestRoot::new().unwrap();
    fs::write(root.path().join("not-a-directory"), b"file").unwrap();
    let runtime = FakeRuntime::new([FakeScript {
        actions: vec![
            FakeAction::SpawnChild {
                command: command(&root),
                script: FakeScript {
                    actions: vec![FakeAction::Hang],
                },
            },
            FakeAction::WriteFile {
                path: "not-a-directory/child".into(),
                contents: vec![],
            },
        ],
    }]);
    assert!(matches!(
        runtime.spawn(&command(&root)),
        Err(RuntimeError::Io { .. })
    ));
    assert!(runtime.processes().iter().all(|process| process.killed));
    fs::remove_file(root.path().join("not-a-directory")).unwrap();
    root.assert_clean();
}

#[test]
fn fixed_and_decreasing_disk_space_reach_zero_without_underflow() {
    let root = TestRoot::new().unwrap();
    let fixed = FakeDiskSpace::fixed(12);
    assert_eq!(fixed.available_bytes(root.path()).unwrap(), 12);
    assert_eq!(fixed.available_bytes(root.path()).unwrap(), 12);
    let disk = FakeDiskSpace::decreasing(12, 5);
    let readings: Vec<_> = (0..5)
        .map(|_| disk.available_bytes(root.path()).unwrap())
        .collect();
    assert_eq!(readings, [12, 7, 2, 0, 0]);
}

#[test]
fn recording_sink_preserves_order_and_drains() {
    let sink = RecordingSink::default();
    for (step, phase) in [Phase::Download, Phase::Install, Phase::Move]
        .into_iter()
        .enumerate()
    {
        sink.emit(JobEvent {
            job_id: "job-1".into(),
            phase,
            step: Some(step as u32),
            bytes: 10,
            total_bytes: Some(20),
            files: 1,
            eta_secs: Some(2),
            message: "progress".into(),
        });
    }
    insta::assert_json_snapshot!("recorded_events", sink.events());
    assert_eq!(sink.take().len(), 3);
    assert!(sink.events().is_empty());
}

#[test]
fn counting_reader_counts_reads_seeks_and_eof() {
    let mut reader = CountingReader::new(Cursor::new(b"abcdef"));
    let mut buffer = [0; 2];
    reader.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"ab");
    reader.seek(SeekFrom::Start(4)).unwrap();
    reader.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"ef");
    assert_eq!(reader.read(&mut buffer).unwrap(), 0);
    assert_eq!(reader.stats().bytes_read, 4);
    assert_eq!(reader.stats().read_calls, 3);
    assert_eq!(reader.stats().seek_calls, 1);
}
