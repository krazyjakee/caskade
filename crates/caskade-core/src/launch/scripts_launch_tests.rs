//! Lifecycle scripts (#179) around real launches: phase order, a failing
//! or cancelled before-launch script, cleanup when a start fails, utility
//! actions, restart recovery and a slow script beside the game.
use super::{test_support::fast_config, *};
use crate::{
    db::LibraryStore,
    model::{
        ActionKind, ActionTracking, Game, GameAction, GameEntry, InstallLocation, LaunchSpec,
        new_id,
    },
    scripts::{
        ExecOutcome, LifecycleScript, ScriptCommand, ScriptExecutor, ScriptPhase, ScriptReport,
        ScriptRequest, ScriptRunner, ScriptScope, ScriptStatus, Shell,
    },
    tracking::TrackingMode,
};
use async_trait::async_trait;
use caskade_discovery::{InstallState, SourceId};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Records each script's phase and name instead of running it.
#[derive(Default)]
struct Recorder {
    ran: Mutex<Vec<String>>,
}
#[async_trait]
impl ScriptExecutor for Recorder {
    async fn run(&self, request: ScriptRequest, _: CancellationToken) -> ExecOutcome {
        self.ran.lock().unwrap().push(format!(
            "{} {}",
            request.env["CASKADE_PHASE"],
            request.args.last().cloned().unwrap_or_default()
        ));
        ExecOutcome {
            status: ScriptStatus::Succeeded,
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            truncated: false,
            error: None,
        }
    }
}

/// Keeps the scripts' reports.
#[derive(Default)]
struct Reports(Mutex<Vec<ScriptReport>>);
#[async_trait]
impl LaunchHooks for Reports {
    async fn scripts(&self, report: &ScriptReport) {
        self.0.lock().unwrap().push(report.clone());
    }
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
/// A game in `dir` that runs `body`.
async fn release(store: &LibraryStore, dir: &Path, body: &str) -> GameEntry {
    let game = store.upsert_game(&Game::new("Scripted")).await.unwrap();
    write_script(&dir.join("game.sh"), body);
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: dir.into() });
    entry.launch = Some(LaunchSpec::Executable {
        path: dir.join("game.sh"),
        args: vec![],
        cwd: None,
        env: BTreeMap::new(),
    });
    store.upsert_entry(&entry).await.unwrap()
}
fn shell(scope: ScriptScope, phase: ScriptPhase, name: &str, text: &str) -> LifecycleScript {
    let mut script = LifecycleScript::new(
        scope,
        phase,
        ScriptCommand::Shell {
            shell: Shell::Sh,
            script: text.into(),
        },
    );
    script.name = name.into();
    script
}
/// A script that appends `<name> <phase> <status>` to `log`.
fn logging(scope: ScriptScope, phase: ScriptPhase, name: &str, log: &Path) -> LifecycleScript {
    shell(
        scope,
        phase,
        name,
        &format!(
            "echo \"{name} $CASKADE_PHASE $CASKADE_STATUS\" >> '{}'",
            log.display()
        ),
    )
}
fn lines(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
async fn wait_for(log: &Path, count: usize) -> Vec<String> {
    for _ in 0..300 {
        let lines = lines(log);
        if lines.len() >= count {
            return lines;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    lines(log)
}
fn game_scope(entry: &GameEntry) -> ScriptScope {
    ScriptScope::Game {
        game_id: entry.game_id.clone(),
    }
}
fn service(store: &LibraryStore, reports: Arc<Reports>) -> LaunchService {
    LaunchService::new(store.clone())
        .with_hooks(reports)
        .with_config(fast_config())
        .with_aliases(None)
}

#[tokio::test]
async fn each_phase_runs_once_in_scope_order_around_the_game() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path(), "sleep 0.2").await;
    for (scope, name) in [
        (ScriptScope::Global, "global"),
        (game_scope(&entry), "game"),
    ] {
        for phase in [
            ScriptPhase::BeforeLaunch,
            ScriptPhase::AfterStart,
            ScriptPhase::AfterExit,
        ] {
            let mut script = shell(scope.clone(), phase, name, name);
            script.name = format!("{name} {}", phase.as_str());
            store.save_lifecycle_script(&script).await.unwrap();
        }
    }
    let recorder = Arc::new(Recorder::default());
    let session = LaunchService::new(store.clone())
        .with_config(fast_config())
        .with_aliases(None)
        .with_script_runner(ScriptRunner::new(recorder.clone()))
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    // After-start runs beside the game; give it a moment.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        *recorder.ran.lock().unwrap(),
        [
            "before_launch global",
            "before_launch game",
            "after_start global",
            "after_start game",
            "after_exit game",
            "after_exit global",
        ]
    );
}

#[tokio::test]
async fn a_failing_before_launch_script_stops_the_launch_and_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let played = temp.path().join("played");
    let entry = release(
        &store,
        temp.path(),
        &format!("touch '{}'", played.display()),
    )
    .await;
    let log = temp.path().join("log");
    store
        .save_lifecycle_script(&shell(
            ScriptScope::Global,
            ScriptPhase::BeforeLaunch,
            "Mount",
            "echo 'no disc' >&2; exit 4",
        ))
        .await
        .unwrap();
    store
        .save_lifecycle_script(&logging(
            game_scope(&entry),
            ScriptPhase::BeforeLaunch,
            "never",
            &log,
        ))
        .await
        .unwrap();
    store
        .save_lifecycle_script(&logging(
            game_scope(&entry),
            ScriptPhase::AfterExit,
            "cleanup",
            &log,
        ))
        .await
        .unwrap();
    let reports = Arc::new(Reports::default());
    let Err(LaunchError::Refused(why)) = service(&store, reports.clone()).launch(&entry.id).await
    else {
        panic!("the launch is refused");
    };
    assert!(why.contains("“Mount” failed (exit code 4)"), "{why}");
    assert!(!played.exists());
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
    assert!(
        store
            .sessions_for_game(&entry.game_id, 5)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(lines(&log), ["cleanup after_exit failed"]);
    let reports = reports.0.lock().unwrap();
    assert_eq!(reports[0].phase, Some(ScriptPhase::BeforeLaunch));
    assert_eq!(reports[0].runs[0].stderr, "no disc\n");
    assert_eq!(reports[1].phase, Some(ScriptPhase::AfterExit));
}

#[tokio::test]
async fn a_slow_before_launch_script_can_be_cancelled() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let played = temp.path().join("played");
    let entry = release(
        &store,
        temp.path(),
        &format!("touch '{}'", played.display()),
    )
    .await;
    let log = temp.path().join("log");
    store
        .save_lifecycle_script(&shell(
            game_scope(&entry),
            ScriptPhase::BeforeLaunch,
            "Slow",
            "sleep 30",
        ))
        .await
        .unwrap();
    store
        .save_lifecycle_script(&logging(
            game_scope(&entry),
            ScriptPhase::AfterExit,
            "cleanup",
            &log,
        ))
        .await
        .unwrap();
    let service = service(&store, Arc::default());
    assert!(!service.cancel_start(&entry.game_id));
    let launching = {
        let service = service.clone();
        let entry = entry.id.clone();
        tokio::spawn(async move { service.launch(&entry).await.map(|_| ()) })
    };
    let mut cancelled = false;
    for _ in 0..200 {
        if service.cancel_start(&entry.game_id) {
            cancelled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(cancelled);
    assert!(matches!(
        launching.await.unwrap(),
        Err(LaunchError::Cancelled)
    ));
    assert!(!played.exists());
    assert_eq!(lines(&log), ["cleanup after_exit cancelled"]);
    // The game is free to launch again.
    assert!(!service.cancel_start(&entry.game_id));
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_launch_that_fails_after_its_scripts_cleans_up_once() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path(), "true").await;
    fs::remove_file(temp.path().join("game.sh")).unwrap();
    let log = temp.path().join("log");
    for phase in [ScriptPhase::BeforeLaunch, ScriptPhase::AfterExit] {
        store
            .save_lifecycle_script(&logging(ScriptScope::Global, phase, "step", &log))
            .await
            .unwrap();
    }
    assert!(
        service(&store, Arc::default())
            .launch(&entry.id)
            .await
            .is_err()
    );
    assert_eq!(
        lines(&log),
        ["step before_launch ", "step after_exit not_started"]
    );
}

#[tokio::test]
async fn tools_run_without_lifecycle_scripts() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path(), "true").await;
    let log = temp.path().join("log");
    store
        .save_lifecycle_script(&logging(
            ScriptScope::Global,
            ScriptPhase::BeforeLaunch,
            "step",
            &log,
        ))
        .await
        .unwrap();
    let ran = temp.path().join("tool-ran");
    write_script(
        &temp.path().join("tool.sh"),
        &format!("touch '{}'", ran.display()),
    );
    let tool = store
        .save_game_action(&GameAction {
            id: new_id(),
            entry_id: entry.id.clone(),
            name: "Tool".into(),
            kind: ActionKind::Utility,
            target: LaunchSpec::Executable {
                path: temp.path().join("tool.sh"),
                args: vec![],
                cwd: None,
                env: BTreeMap::new(),
            },
            tracking: ActionTracking::default(),
        })
        .await
        .unwrap();
    service(&store, Arc::default())
        .run_tool(&entry.id, &tool.id)
        .await
        .unwrap();
    for _ in 0..200 {
        if ran.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ran.exists());
    assert!(lines(&log).is_empty());
}

#[tokio::test]
async fn a_slow_after_start_script_never_holds_the_game_s_session() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path(), "sleep 0.2").await;
    // A direct program, not a shell: other tests track new `sh` processes.
    let mut slow = shell(game_scope(&entry), ScriptPhase::AfterStart, "Slow", "");
    slow.command = ScriptCommand::Direct {
        program: "sleep".into(),
        args: vec!["20".into()],
    };
    store.save_lifecycle_script(&slow).await.unwrap();
    let started = std::time::Instant::now();
    service(&store, Arc::default())
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn after_a_restart_each_phase_still_runs_once() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path(), "true").await;
    let game = temp.path().join("longgame");
    fs::copy("/bin/sleep", &game).unwrap();
    let play = store
        .save_game_action(&GameAction {
            id: new_id(),
            entry_id: entry.id.clone(),
            name: "Long".into(),
            kind: ActionKind::Play,
            target: LaunchSpec::Executable {
                path: game.clone(),
                args: vec!["30".into()],
                cwd: None,
                env: BTreeMap::new(),
            },
            tracking: ActionTracking {
                mode: TrackingMode::ProcessTree,
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let log = temp.path().join("log");
    for phase in [ScriptPhase::AfterStart, ScriptPhase::AfterExit] {
        store
            .save_lifecycle_script(&logging(game_scope(&entry), phase, "step", &log))
            .await
            .unwrap();
    }
    let choice = LaunchChoice {
        action: Some(play.id.clone()),
        ..Default::default()
    };
    // The first run: the game starts and its after-start script runs.
    let first = service(&store, Arc::default());
    let handle = first.launch_with(&entry.id, &choice).await.unwrap();
    assert_eq!(wait_for(&log, 1).await, ["step after_start "]);
    first.shutdown();
    assert!(matches!(handle.wait().await, Err(LaunchError::Cancelled)));
    // Caskade starts again and finds the game: no second after-start.
    let second = service(&store, Arc::default());
    let handles = second.recover().await.unwrap();
    assert_eq!(handles.len(), 1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(lines(&log), ["step after_start "]);
    // The game is stopped; its session ends and cleans up once.
    let pids: Vec<u32> = store
        .in_flight_sessions()
        .await
        .unwrap()
        .iter()
        .flat_map(|session| {
            serde_json::from_value::<crate::sessions::RecoveryState>(
                session.tracking.clone().unwrap(),
            )
            .unwrap()
            .processes
        })
        .map(|identity| identity.pid)
        .collect();
    for pid in pids {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap();
    }
    for handle in handles {
        handle.wait().await.unwrap();
    }
    assert_eq!(lines(&log), ["step after_start ", "step after_exit exited"]);
}

#[tokio::test]
async fn a_game_that_ended_while_caskade_was_closed_cleans_up_in_the_background() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path(), "true").await;
    let log = temp.path().join("log");
    store
        .save_lifecycle_script(&shell(
            game_scope(&entry),
            ScriptPhase::AfterExit,
            "step",
            &format!(
                "sleep 0.3; echo \"step $CASKADE_PHASE $CASKADE_STATUS\" >> '{}'",
                log.display()
            ),
        ))
        .await
        .unwrap();
    // A session left open by a crash, whose game is gone.
    let mut session = crate::model::PlaySession::new(&entry.game_id, &entry.id, 10);
    let mut state = crate::sessions::RecoveryState::pending(Default::default());
    state.phase = crate::sessions::SessionPhase::Playing;
    state.processes = vec![crate::tracking::ProcessIdentity {
        pid: u32::MAX,
        started_at: 1,
    }];
    state.scripts = Some(crate::sessions::ScriptState {
        context: crate::scripts::ScriptContext {
            game_id: entry.game_id.clone(),
            entry_id: entry.id.clone(),
            ..Default::default()
        },
        started: true,
    });
    session.tracking = Some(serde_json::to_value(&state).unwrap());
    store.start_session(&session).await.unwrap();
    let started = std::time::Instant::now();
    assert!(
        service(&store, Arc::default())
            .recover()
            .await
            .unwrap()
            .is_empty()
    );
    // Startup didn't wait for the script.
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(wait_for(&log, 1).await, ["step after_exit interrupted"]);
    let _: PathBuf = log;
}
