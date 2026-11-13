use super::*;

fn script(scope: ScriptScope, phase: ScriptPhase, name: &str) -> LifecycleScript {
    let mut script = LifecycleScript::new(
        scope,
        phase,
        ScriptCommand::Direct {
            program: "/bin/true".into(),
            args: vec![],
        },
    );
    script.name = name.into();
    script
}
fn game() -> ScriptScope {
    ScriptScope::Game {
        game_id: "g".into(),
    }
}
fn profile() -> ScriptScope {
    ScriptScope::Profile {
        profile_id: "p".into(),
    }
}
fn names(scripts: &[&LifecycleScript]) -> Vec<String> {
    scripts.iter().map(|script| script.name.clone()).collect()
}

#[test]
fn scopes_run_outside_in_and_cleanup_inside_out() {
    let mut off = script(game(), ScriptPhase::BeforeLaunch, "off");
    off.enabled = false;
    let set = ScriptSet {
        global: vec![
            script(ScriptScope::Global, ScriptPhase::BeforeLaunch, "global 1"),
            script(ScriptScope::Global, ScriptPhase::AfterExit, "global exit"),
            script(ScriptScope::Global, ScriptPhase::BeforeLaunch, "global 2"),
        ],
        profile: vec![
            script(profile(), ScriptPhase::BeforeLaunch, "profile"),
            script(profile(), ScriptPhase::AfterExit, "profile exit"),
        ],
        game: vec![
            off,
            script(game(), ScriptPhase::BeforeLaunch, "game"),
            script(game(), ScriptPhase::AfterStart, "game start"),
            script(game(), ScriptPhase::AfterExit, "game exit"),
        ],
    };
    assert_eq!(
        names(&set.ordered(ScriptPhase::BeforeLaunch)),
        ["global 1", "global 2", "profile", "game"]
    );
    assert_eq!(names(&set.ordered(ScriptPhase::AfterStart)), ["game start"]);
    assert_eq!(
        names(&set.ordered(ScriptPhase::AfterExit)),
        ["game exit", "profile exit", "global exit"]
    );
}

#[test]
fn placeholders_expand_as_data_inside_one_argument() {
    let context = ScriptContext {
        game_title: "Café {GameId} $(rm -rf ~); `x` \"q\" 'q'".into(),
        game_id: "id-1".into(),
        install_dir: Some("/games/with space/&;|".into()),
        ..Default::default()
    };
    let values = context.values(ScriptPhase::BeforeLaunch);
    assert_eq!(
        expand("--title={GameTitle}", &values),
        "--title=Café {GameId} $(rm -rf ~); `x` \"q\" 'q'"
    );
    assert_eq!(
        expand("{InstallDir}/save {Unknown} {", &values),
        "/games/with space/&;|/save {Unknown} {"
    );
    assert_eq!(expand("{Pid}{RomPath}", &values), "");
    let variables: Vec<_> = values.iter().map(|value| value.variable.as_str()).collect();
    assert!(variables.contains(&"CASKADE_INSTALL_DIR"));
    assert!(variables.contains(&"CASKADE_PHASE"));
}

#[test]
fn scripts_are_checked_before_they_are_saved() {
    let ok = script(ScriptScope::Global, ScriptPhase::BeforeLaunch, " Mount ");
    assert_eq!(ok.normalized().unwrap().name, "Mount");
    let mut nameless = ok.clone();
    nameless.name = " ".into();
    assert!(nameless.normalized().is_err());
    let mut forever = ok.clone();
    forever.timeout_secs = 0;
    assert!(forever.normalized().is_err());
    forever.timeout_secs = MAX_TIMEOUT_SECS + 1;
    assert!(forever.normalized().is_err());
    let mut empty = ok.clone();
    empty.command = ScriptCommand::Shell {
        shell: Shell::available()[0],
        script: "  ".into(),
    };
    assert!(empty.normalized().is_err());
    let mut elsewhere = ok.clone();
    elsewhere.command = ScriptCommand::Shell {
        shell: if cfg!(windows) { Shell::Sh } else { Shell::Cmd },
        script: "echo".into(),
    };
    assert_eq!(elsewhere.normalized().is_ok(), cfg!(windows));
}

/// #271: cmd gets its script between the quotes /s strips, on a command
/// line written as is; other shells keep one argument per value.
#[test]
fn cmd_scripts_reach_cmd_as_typed() {
    let script = r#""C:\Games\quoted script.cmd" "an arg" & echo "two words""#;
    assert_eq!(
        Shell::Cmd.args(script),
        (
            vec![
                "/d".to_owned(),
                "/s".into(),
                "/c".into(),
                format!("\"{script}\""),
            ],
            true
        )
    );
    assert_eq!(
        Shell::Powershell.args(script),
        (
            vec![
                "-NoProfile".to_owned(),
                "-NonInteractive".into(),
                "-Command".into(),
                script.into(),
            ],
            false
        )
    );
    assert_eq!(
        Shell::Sh.args(script),
        (vec!["-c".to_owned(), script.into()], false)
    );
}

/// #271 on Windows: quoted paths with spaces, quotes inside the text,
/// quoted arguments and several statements all run as typed.
#[cfg(windows)]
#[tokio::test]
async fn cmd_runs_quoted_paths_arguments_and_several_statements() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("quoted script.cmd");
    std::fs::write(&file, "@echo off\r\necho quoted-ok [%~1]\r\n").unwrap();
    let mut script = LifecycleScript::new(
        ScriptScope::Global,
        ScriptPhase::BeforeLaunch,
        ScriptCommand::Shell {
            shell: Shell::Cmd,
            script: format!(
                r#""{}" "an arg" && echo "hello world" & echo %CASKADE_GAME_TITLE%"#,
                file.display()
            ),
        },
    );
    script.name = "Test".into();
    let context = ScriptContext {
        game_title: "Indie Game".into(),
        ..Default::default()
    };
    let run = ScriptRunner::default()
        .run(&script, &context, &CancellationToken::new())
        .await;
    assert_eq!(run.status, ScriptStatus::Succeeded, "{run:?}");
    assert_eq!(
        run.stdout.replace("\r\n", "\n"),
        "quoted-ok [an arg]\n\"hello world\" \nIndie Game\n"
    );
}

#[cfg(unix)]
mod processes {
    use super::*;
    use std::time::Instant;

    fn direct(program: &str, args: &[&str]) -> LifecycleScript {
        let mut script = LifecycleScript::new(
            ScriptScope::Global,
            ScriptPhase::BeforeLaunch,
            ScriptCommand::Direct {
                program: program.into(),
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            },
        );
        script.name = "Test".into();
        script
    }
    fn shell(text: &str) -> LifecycleScript {
        let mut script = direct("sh", &[]);
        script.command = ScriptCommand::Shell {
            shell: Shell::Sh,
            script: text.into(),
        };
        script
    }
    async fn run(script: &LifecycleScript, context: &ScriptContext) -> ScriptRun {
        ScriptRunner::default()
            .run(script, context, &CancellationToken::new())
            .await
    }

    #[tokio::test]
    async fn a_direct_command_gets_its_arguments_verbatim_and_its_context() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("dir with space;&");
        std::fs::create_dir(&dir).unwrap();
        let context = ScriptContext {
            game_title: "$(touch pwned)".into(),
            install_dir: Some(dir.clone()),
            ..Default::default()
        };
        let run = run(
            &direct(
                "printf",
                &["[%s]\\n", "{GameTitle}", "{InstallDir}", "a b", "*"],
            ),
            &context,
        )
        .await;
        assert_eq!(run.status, ScriptStatus::Succeeded, "{run:?}");
        assert_eq!(
            run.stdout,
            format!("[$(touch pwned)]\n[{}]\n[a b]\n[*]\n", dir.display())
        );
        assert!(!dir.join("pwned").exists());
        // It runs in the install folder.
        let run =
            super::processes::run(&shell("pwd; printf %s \"$CASKADE_GAME_TITLE\""), &context).await;
        assert_eq!(
            run.stdout,
            format!("{}\n$(touch pwned)", dir.canonicalize().unwrap().display())
        );
        assert!(!dir.join("pwned").exists());
    }

    #[tokio::test]
    async fn failures_say_what_happened() {
        let context = ScriptContext::default();
        let failed = run(&shell("echo oops >&2; exit 3"), &context).await;
        assert_eq!(failed.status, ScriptStatus::Failed);
        assert_eq!(failed.exit_code, Some(3));
        assert_eq!(failed.stderr, "oops\n");
        assert!(
            failed.describe().contains("exit code 3"),
            "{}",
            failed.describe()
        );
        let missing = run(&direct("/no/such/program", &[]), &context).await;
        assert_eq!(missing.status, ScriptStatus::NotStarted);
        assert!(missing.describe().contains("/no/such/program"));
        let mut nowhere = shell("true");
        nowhere.cwd = Some("/no/such/folder".into());
        let run = run(&nowhere, &context).await;
        assert_eq!(run.status, ScriptStatus::NotStarted);
        assert!(run.error.unwrap().contains("/no/such/folder"));
    }

    #[tokio::test]
    async fn output_keeps_its_end_within_the_limit_without_control_characters() {
        let run = run(
            &shell("head -c 200000 /dev/zero | tr '\\0' x; printf '\\033[31mend\\n'"),
            &ScriptContext::default(),
        )
        .await;
        assert!(run.truncated);
        assert!(run.stdout.len() <= exec::OUTPUT_LIMIT);
        assert!(
            run.stdout.ends_with("[31mend\n"),
            "{:?}",
            &run.stdout[run.stdout.len() - 20..]
        );
        assert!(!run.stdout.contains('\u{1b}'));
    }

    #[tokio::test]
    async fn a_time_limit_or_a_cancel_stops_the_script_and_what_it_started() {
        let temp = tempfile::tempdir().unwrap();
        let background = |marker: &str, delay: u32| {
            shell(&format!(
                "(sleep {delay}; touch '{}') & sleep 30",
                temp.path().join(marker).display()
            ))
        };
        let mut slow = background("timed-out", 2);
        slow.timeout_secs = 1;
        let started = Instant::now();
        let timed_out = run(&slow, &ScriptContext::default()).await;
        assert_eq!(timed_out.status, ScriptStatus::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(timed_out.describe().contains("ran out of time"));

        let slow = background("cancelled", 1);
        let cancel = CancellationToken::new();
        let runner = ScriptRunner::default();
        let task = {
            let cancel = cancel.clone();
            tokio::spawn(async move { runner.run(&slow, &ScriptContext::default(), &cancel).await })
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.cancel();
        let cancelled = task.await.unwrap();
        assert_eq!(cancelled.status, ScriptStatus::Cancelled);
        tokio::time::sleep(Duration::from_millis(2500)).await;
        for marker in ["timed-out", "cancelled"] {
            assert!(
                !temp.path().join(marker).exists(),
                "the {marker} script's background job was stopped too"
            );
        }
    }

    #[tokio::test]
    async fn before_launch_stops_at_the_first_failure_and_cleanup_runs_every_script() {
        let mut failing = shell("exit 1");
        failing.name = "fails".into();
        let mut after = shell("true");
        after.name = "after".into();
        let set = ScriptSet {
            global: vec![failing.clone(), after.clone()],
            ..Default::default()
        };
        let runner = ScriptRunner::default();
        let context = ScriptContext::default();
        let cancel = CancellationToken::new();
        let report = runner
            .run_phase(&set, ScriptPhase::BeforeLaunch, &context, &cancel)
            .await;
        assert_eq!(report.runs.len(), 1);
        assert_eq!(report.failed().unwrap().name, "fails");
        for script in [&mut failing, &mut after] {
            script.phase = ScriptPhase::AfterExit;
        }
        let set = ScriptSet {
            global: vec![failing, after],
            ..Default::default()
        };
        let report = runner
            .run_phase(&set, ScriptPhase::AfterExit, &context, &cancel)
            .await;
        assert_eq!(report.runs.len(), 2);
    }

    #[test]
    fn programs_are_found_on_the_path() {
        assert!(find_program(Path::new("sh"), None).is_some());
        assert!(find_program(Path::new("no-such-interpreter-xyz"), None).is_none());
        assert!(find_program(Path::new("/bin/sh"), None).is_some());
    }
    use std::path::Path;
}
