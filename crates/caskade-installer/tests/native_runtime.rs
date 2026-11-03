//! #25: the native runtime (argv/env/cwd, exit codes, timeout, process-tree
//! kill) and the out-of-root watcher, through real processes on Unix. The
//! Windows job-object path is compiled by the windows-gnu gate; native
//! Windows runs are #22's runner.

#![cfg(any(unix, windows))]
// Most cases are Unix process tests; the Windows build keeps only the native AC.
#![cfg_attr(windows, allow(unused_imports))]

use caskade_installer::{
    exec::{self, CancellationToken, ExecError, ExecReport, ExecRequest, Executor, JournalEntry},
    runtime::{NativeRuntime, OutOfRootWatcher, ProcessCommand, Runtime, RuntimeError},
    testing::{FakeDiskSpace, RecordingSink, TestRoot},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(unix)]
fn sh(script: &str, cwd: &Path) -> ProcessCommand {
    ProcessCommand {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            script.into(),
            "installer".into(),
            "two words".into(),
        ],
        env: BTreeMap::from([("CASKADE_TEST".into(), "value; $(not expanded)".into())]),
        cwd: cwd.into(),
    }
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    // Signal 0 probes existence; zombies of other parents count as gone
    // once reaped by init, which happens promptly for orphaned sleeps.
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn wait_gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[cfg(unix)]
#[test]
fn argv_env_cwd_and_exit_codes_are_passed_without_a_shell_layer() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = NativeRuntime::new();
    let process = runtime
        .spawn(&sh(
            r#"printf '%s|%s|%s' "$1" "$CASKADE_TEST" "$PWD" > out; exit 7"#,
            dir.path(),
        ))
        .unwrap();
    assert_eq!(
        runtime.wait(process, Duration::from_secs(10)).unwrap().code,
        7
    );
    runtime.kill(process).unwrap();
    let out = fs::read_to_string(dir.path().join("out")).unwrap();
    let cwd = dunce::canonicalize(dir.path()).unwrap();
    assert_eq!(
        out,
        format!("two words|value; $(not expanded)|{}", cwd.display())
    );
    // Signals map to 128 + signal, like a shell.
    let process = runtime.spawn(&sh("kill -TERM $$", dir.path())).unwrap();
    assert_eq!(
        runtime.wait(process, Duration::from_secs(10)).unwrap().code,
        143
    );
    runtime.kill(process).unwrap();
    assert!(matches!(
        runtime.spawn(&ProcessCommand {
            program: dir.path().join("missing"),
            args: vec![],
            env: BTreeMap::new(),
            cwd: dir.path().into(),
        }),
        Err(RuntimeError::Io { .. })
    ));
}

#[cfg(unix)]
#[test]
fn timeout_and_kill_end_the_whole_process_tree() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = NativeRuntime::new();
    let started = Instant::now();
    let process = runtime
        .spawn(&sh("sleep 30 & echo $! > child; wait", dir.path()))
        .unwrap();
    assert!(matches!(
        runtime.wait(process, Duration::from_millis(300)),
        Err(RuntimeError::TimedOut { .. })
    ));
    assert!(started.elapsed() < Duration::from_secs(10));
    let child: i32 = fs::read_to_string(dir.path().join("child"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(wait_gone(child), "descendant survived the timeout");
    // Unknown after kill; a second kill is a no-op.
    runtime.kill(process).unwrap();
    assert!(matches!(
        runtime.wait(process, Duration::ZERO),
        Err(RuntimeError::UnknownProcess { .. })
    ));
}

#[cfg(unix)]
#[test]
fn kill_after_parent_exit_still_ends_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = NativeRuntime::new();
    // The installer exits 0 but leaves a background helper behind.
    let process = runtime
        .spawn(&sh("sleep 30 & echo $! > child; exit 0", dir.path()))
        .unwrap();
    assert_eq!(
        runtime.wait(process, Duration::from_secs(10)).unwrap().code,
        0
    );
    let child: i32 = fs::read_to_string(dir.path().join("child"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(child));
    runtime.kill(process).unwrap();
    assert!(wait_gone(child), "orphaned descendant survived kill");
}

#[cfg(unix)]
#[test]
fn bounded_stdin_is_delivered_and_ignored_input_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = NativeRuntime::new();
    let process = runtime
        .spawn_with_input(&sh("head -c 3 > answer", dir.path()), b"qy\n")
        .unwrap();
    assert!(runtime.system_pid(process).unwrap() > 0);
    assert_eq!(
        runtime.wait(process, Duration::from_secs(10)).unwrap().code,
        0
    );
    runtime.kill(process).unwrap();
    assert_eq!(fs::read(dir.path().join("answer")).unwrap(), b"qy\n");
    // A child that closes stdin immediately (broken pipe) is not an error.
    let process = runtime
        .spawn_with_input(&sh("exec 0<&-; exit 0", dir.path()), &[b'y'; 4096])
        .unwrap();
    assert_eq!(
        runtime.wait(process, Duration::from_secs(10)).unwrap().code,
        0
    );
    runtime.kill(process).unwrap();
    assert!(matches!(
        runtime.spawn_with_input(&sh("true", dir.path()), &[0; 4097]),
        Err(RuntimeError::Io { .. })
    ));
}

#[cfg(unix)]
#[test]
fn concurrent_kill_reports_killed_to_the_waiter() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(NativeRuntime::new());
    let process = runtime.spawn(&sh("sleep 30", dir.path())).unwrap();
    let waiter = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.wait(process, Duration::from_secs(30)))
    };
    std::thread::sleep(Duration::from_millis(100));
    runtime.kill(process).unwrap();
    assert!(matches!(
        waiter.join().unwrap(),
        Err(RuntimeError::Killed { .. })
    ));
}

/// A scripted native installer: writes the game into the install directory
/// and leaves shortcuts/settings in watched per-user locations.
#[cfg(unix)]
fn installer(home: &Path) -> Step {
    let script = format!(
        r#"set -e
mkdir -p "$1/bin"
printf game > "$1/bin/game"
mkdir -p '{home}/Desktop' '{home}/AppData/Indie Game'
printf shortcut > '{home}/Desktop/Indie Game.desktop'
printf settings > '{home}/AppData/Indie Game/settings.ini'
{extra}"#,
        home = home.display(),
        extra = "exit \"$2\"",
    );
    Step::Run {
        program: Program::Tool("/bin/sh".into()),
        args: vec![
            "-c".into(),
            script,
            "installer".into(),
            exec::INSTALL_DIR.into(),
        ],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 30,
        expect: vec!["bin/game".into()],
    }
}

#[cfg(unix)]
fn plan(step: Step, exit: u32) -> PlanSet {
    let mut step = step;
    if let Step::Run { args, .. } = &mut step {
        args.push(exit.to_string());
    }
    let candidate = Candidate {
        handler: HandlerId("native-test".into()),
        strategy: Strategy::RunSilent,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: true,
            runtime: Some(RuntimeSpec::Native),
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: Some(1024),
        },
        steps: vec![step],
        warnings: vec![],
    };
    PlanSet {
        candidates: vec![candidate],
        selected: Some(0),
        question: None,
    }
}

#[cfg(unix)]
fn watcher(home: &Path) -> OutOfRootWatcher {
    OutOfRootWatcher::new(
        [("user.home".to_owned(), home.to_path_buf())]
            .into_iter()
            .collect(),
    )
}

#[cfg(unix)]
async fn run(
    root: &TestRoot,
    home: &Path,
    exit: u32,
) -> std::result::Result<ExecReport, ExecError> {
    Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_observer(Arc::new(watcher(home)))
    .execute(
        ExecRequest {
            job_id: format!("native-{exit}"),
            root: root.path().into(),
            relative_dir: "Indie Game".into(),
            plans: plan(installer(home), exit),
            sources: BTreeMap::new(),
        },
        CancellationToken::new(),
    )
    .await
}

#[cfg(unix)]
fn external(report: &[JournalEntry]) -> Vec<(PathBuf, bool)> {
    report
        .iter()
        .filter_map(|entry| match entry {
            JournalEntry::ExternalFile { path, original } => {
                assert!(original.is_none());
                Some((path.clone(), false))
            }
            JournalEntry::ExternalDirectory { path } => Some((path.clone(), true)),
            // Compensation and informational entries are not file effects.
            _ => None,
        })
        .collect()
}

/// For a failed assertion: when each path was born, against a time taken
/// just before the run (the watch began after it).
#[cfg(unix)]
fn births(home: &Path, before: std::time::SystemTime, paths: &[&str]) -> String {
    let mut lines = vec![format!("taken before the run: {before:?}")];
    for path in paths {
        let born = fs::symlink_metadata(home.join(path)).and_then(|meta| meta.created());
        let after = born.as_ref().ok().map(|born| {
            born.duration_since(before)
                .map_or_else(|e| format!("-{:?}", e.duration()), |d| format!("+{d:?}"))
        });
        lines.push(format!("{path}: born {born:?} ({after:?} after)"));
    }
    lines.join("\n")
}

#[cfg(unix)]
#[tokio::test]
async fn native_install_lands_in_the_root_and_reports_out_of_root_files() {
    let root = TestRoot::new().unwrap();
    let home = tempfile::tempdir().unwrap();
    let home_path = dunce::canonicalize(home.path()).unwrap();
    fs::create_dir(home_path.join("Documents")).unwrap();
    fs::write(home_path.join("Documents/existing.txt"), b"keep").unwrap();
    let before = std::time::SystemTime::now();
    let report = run(&root, &home_path, 0).await.unwrap();
    assert_eq!(
        fs::read(root.path().join("Indie Game/bin/game")).unwrap(),
        b"game"
    );
    let recorded = external(&report.journal);
    for (path, directory) in [
        ("Desktop", true),
        ("Desktop/Indie Game.desktop", false),
        ("AppData", true),
        ("AppData/Indie Game", true),
        ("AppData/Indie Game/settings.ini", false),
    ] {
        assert!(
            recorded.contains(&(home_path.join(path), directory)),
            "{path} missing from {recorded:?}\n{}",
            births(
                &home_path,
                before,
                &[
                    "Desktop",
                    "Desktop/Indie Game.desktop",
                    "AppData",
                    "AppData/Indie Game",
                    "AppData/Indie Game/settings.ini"
                ]
            )
        );
    }
    // The journal converts to portable, scope-relative manifest effects.
    let effects =
        manifest::effects_from_journal(&report.journal, watcher(&home_path).external_roots())
            .unwrap();
    assert!(effects.iter().any(|effect| {
        effect.location().scope == "user.home"
            && effect.location().path.as_str() == "Desktop/Indie Game.desktop"
    }));
    // Nothing inside the storage root, and no pre-existing file, is external.
    assert!(
        recorded
            .iter()
            .all(|(path, _)| path.starts_with(&home_path))
    );
    assert!(
        !recorded
            .iter()
            .any(|(path, _)| path.ends_with("existing.txt"))
    );
    root.assert_sentinel_untouched();
}

#[cfg(unix)]
#[tokio::test]
async fn failed_native_install_rolls_back_its_out_of_root_files() {
    let root = TestRoot::new().unwrap();
    let home = tempfile::tempdir().unwrap();
    let home_path = dunce::canonicalize(home.path()).unwrap();
    fs::create_dir(home_path.join("Desktop")).unwrap();
    fs::write(home_path.join("Desktop/existing.desktop"), b"keep").unwrap();
    let before = std::time::SystemTime::now();
    let error = run(&root, &home_path, 3).await.unwrap_err();
    assert!(
        matches!(&error, ExecError::AllCandidatesFailed { .. }),
        "{error:?}"
    );
    let left = || {
        births(
            &home_path,
            before,
            &[
                "Desktop",
                "Desktop/Indie Game.desktop",
                "AppData",
                "AppData/Indie Game",
                "AppData/Indie Game/settings.ini",
            ],
        )
    };
    assert!(
        !home_path.join("Desktop/Indie Game.desktop").exists(),
        "{}",
        left()
    );
    assert!(!home_path.join("AppData").exists(), "{}", left());
    assert_eq!(
        fs::read(home_path.join("Desktop/existing.desktop")).unwrap(),
        b"keep"
    );
    assert!(!root.path().join("Indie Game").exists());
    root.assert_sentinel_untouched();
}

/// #25 AC on the Windows runner (#22): a genuine Inno setup with shortcuts
/// (`native-inno-shortcuts`) runs silently
/// through NativeRuntime into the storage root while the host's standard
/// locations are watched. Every journalled effect is outside the root and
/// maps to a manifest scope. Opt in with CASKADE_TEST_NATIVE=1.
#[cfg(windows)]
#[tokio::test]
async fn inno_fixture_installs_silently_into_the_root_with_watched_effects() {
    if std::env::var_os("CASKADE_TEST_NATIVE").is_none() {
        eprintln!("skipping: set CASKADE_TEST_NATIVE=1 on the Windows runner");
        return;
    }
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bin/native-uninstall/native-inno-shortcuts/setup.exe");
    let root = TestRoot::new().unwrap();
    // The fixture registers itself, writes HKCU\Software\CaskadeNativeFixture\inno
    // and makes shortcuts; leaving them would fail uninstall_native's
    // clean-runner check later in the same job.
    struct Uninstall(PathBuf);
    impl Drop for Uninstall {
        fn drop(&mut self) {
            let uninstaller = self.0.join("unins000.exe");
            if uninstaller.is_file() {
                let _ = std::process::Command::new(uninstaller)
                    .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
                    .status();
            }
            use winreg::{RegKey, enums::*};
            let _ = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_with_flags(
                    r"Software\CaskadeNativeFixture",
                    KEY_READ | KEY_WRITE | KEY_WOW64_32KEY,
                )
                .and_then(|key| key.delete_subkey_all("inno"));
        }
    }
    let _uninstall = Uninstall(root.path().join("Fixture Game"));
    let watcher = OutOfRootWatcher::for_host();
    let step = Step::Run {
        program: Program::Source(SourcePath::Artifact {
            artifact: ArtifactId("setup".into()),
            path: ".".into(),
        }),
        args: [
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/SP-",
            "/CURRENTUSER",
        ]
        .into_iter()
        .map(String::from)
        .chain([format!("/DIR={}", exec::INSTALL_DIR)])
        .collect(),
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 300,
        expect: vec!["FixtureGame.exe".into()],
    };
    let candidate = Candidate {
        handler: HandlerId("inno".into()),
        strategy: Strategy::RunSilent,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: true,
            runtime: Some(RuntimeSpec::Native),
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: None,
        },
        steps: vec![step],
        warnings: vec![],
    };
    let report = Executor::new(
        Arc::new(NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_observer(Arc::new(watcher.clone()))
    .execute(
        ExecRequest {
            job_id: "native-inno".into(),
            root: root.path().into(),
            relative_dir: "Fixture Game".into(),
            plans: PlanSet {
                candidates: vec![candidate],
                selected: Some(0),
                question: None,
            },
            sources: BTreeMap::from([(ArtifactId("setup".into()), setup)]),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(root.path().join("Fixture Game/FixtureGame.exe").is_file());
    let storage = dunce::canonicalize(root.path()).unwrap();
    for entry in &report.journal {
        if let JournalEntry::ExternalFile { path, .. } | JournalEntry::ExternalDirectory { path } =
            entry
        {
            assert!(!path.starts_with(&storage), "{path:?}");
        }
    }
    let effects =
        manifest::effects_from_journal(&report.journal, watcher.external_roots()).unwrap();
    eprintln!("out-of-root effects: {effects:#?}");
    // The fixture's [Icons] create Desktop and Start-menu shortcuts.
    assert!(
        effects
            .iter()
            .any(|effect| effect.location().path.as_str().ends_with(".lnk")),
        "no shortcut reported"
    );
    root.assert_sentinel_untouched();
}
