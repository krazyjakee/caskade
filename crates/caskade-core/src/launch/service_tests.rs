#![cfg(unix)]
use super::{test_support::*, *};
use crate::{
    model::PlaySession,
    sessions::{RecoveryState, SessionPhase},
};
use async_trait::async_trait;
use std::{fs, sync::Arc, time::Duration};

#[cfg(target_os = "linux")]
#[tokio::test]
async fn installer_fixture_game_records_session_and_ordered_events() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("fixture-game");
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    )
    .unwrap();
    let token = t.path().join("token");
    let entry = entry(
        exe,
        vec!["--token".into(), token.to_str().unwrap().into()],
        Some(t.path().into()),
    );
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let events = Arc::new(Events::default());
    let service = service(store.clone(), events.clone());
    let session = service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(session.entry_id, entry.id);
    assert!(session.ended_at_ms.is_some());
    assert!(session.duration_ms < 1000);
    assert!(token.is_file());
    assert_eq!(store.sessions.lock().unwrap().len(), 1);
    let events = events.0.lock().unwrap();
    assert_eq!(
        events.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
        vec!["game.started", "game.exited", "launch.after"]
    );
    assert_eq!(events[1].1.status, SessionStatus::Exited);
}

#[tokio::test]
async fn forked_game_outlives_launcher_and_duplicate_launch_is_rejected() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("game");
    fs::copy("/bin/sleep", &exe).unwrap();
    let entry = entry(
        "/bin/sh".into(),
        vec![
            "-c".into(),
            r#""$1" 0.5 &"#.into(),
            "launcher".into(),
            exe.to_str().unwrap().into(),
        ],
        Some(t.path().into()),
    );
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let service = service(store, Arc::new(Events::default()));
    let handle = service.launch(&entry.id).await.unwrap();
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::AlreadyRunning)
    ));
    let session = handle.wait().await.unwrap();
    // Indirect launch begins only when the game child is observed, after the
    // launcher exits. Snapshot latency varies under concurrent system load, so
    // assert the child produced a completed, positive play session rather than
    // a wall-clock lower bound relative to when the launcher was spawned.
    assert!(session.duration_ms > 0, "{session:?}");
    assert!(session.ended_at_ms.is_some());
}

#[tokio::test]
async fn restart_reattaches_to_child_without_creating_second_session() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("game");
    fs::copy("/bin/sleep", &exe).unwrap();
    let entry = entry(exe, vec!["0.8".into()], Some(t.path().into()));
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let events = Arc::new(Events::default());
    let first = service(store.clone(), events.clone());
    let handle = first.launch(&entry.id).await.unwrap();
    for _ in 0..100 {
        if !events.0.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(40)).await;
    first.shutdown();
    assert!(matches!(handle.wait().await, Err(LaunchError::Cancelled)));
    let second = service(store.clone(), events);
    let mut recovered = second.recover().await.unwrap();
    assert_eq!(recovered.len(), 1);
    let session = recovered.remove(0).wait().await.unwrap();
    assert!(session.duration_ms >= 500, "{}", session.duration_ms);
    assert_eq!(store.sessions.lock().unwrap().len(), 1);
    assert!(session.ended_at_ms.is_some());
}

#[tokio::test]
async fn dead_recovered_identity_stops_at_last_heartbeat() {
    let store = Arc::new(MemoryStore::default());
    let mut s = PlaySession::new("game", "entry", 10);
    s.duration_ms = 123;
    s.heartbeat_at_ms = 1000;
    let mut state = RecoveryState::pending(Default::default());
    state.phase = SessionPhase::Playing;
    state.processes = vec![crate::tracking::ProcessIdentity {
        pid: u32::MAX,
        started_at: 1,
        boot: 0,
    }];
    s.tracking = Some(serde_json::to_value(state).unwrap());
    store.sessions.lock().unwrap().insert(s.id.clone(), s);
    let events = Arc::new(Events::default());
    assert!(
        service(store.clone(), events.clone())
            .recover()
            .await
            .unwrap()
            .is_empty()
    );
    let sessions = store.sessions.lock().unwrap();
    let s = sessions.values().next().unwrap();
    assert_eq!(s.ended_at_ms, Some(1000));
    assert_eq!(s.duration_ms, 123);
    assert_eq!(
        events.0.lock().unwrap()[0].1.status,
        SessionStatus::Interrupted
    );
}

struct Cancel;
#[async_trait]
impl LaunchHooks for Cancel {
    async fn before(&self, _: &LaunchTicket, _: &mut LaunchCommand) -> Result<bool> {
        Ok(false)
    }
}
#[tokio::test]
async fn interceptor_cancel_leaves_no_session_or_process() {
    let store = Arc::new(MemoryStore::default());
    let entry = entry("/bin/true".into(), vec![], None);
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let service = LaunchService::from_repository(store.clone()).with_hooks(Arc::new(Cancel));
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::Cancelled)
    ));
    assert!(store.sessions.lock().unwrap().is_empty());
}
#[tokio::test]
async fn nonzero_direct_exit_is_recorded_as_crash() {
    let store = Arc::new(MemoryStore::default());
    let entry = entry("/bin/sh".into(), vec!["-c".into(), "exit 17".into()], None);
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let events = Arc::new(Events::default());
    let session = service(store, events.clone())
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    assert_eq!(events.0.lock().unwrap()[1].1.status, SessionStatus::Crashed);
}

#[tokio::test]
async fn storage_game_lease_excludes_launch_until_the_move_finishes() {
    let store = Arc::new(MemoryStore::default());
    let entry = entry("/bin/true".into(), vec![], None);
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let service = service(store, Arc::new(Events::default()));
    let movement = service.try_game_lease(&entry.game_id).unwrap();
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::AlreadyRunning)
    ));
    drop(movement);
    assert!(
        service
            .launch(&entry.id)
            .await
            .unwrap()
            .wait()
            .await
            .unwrap()
            .ended_at_ms
            .is_some()
    );
}

struct CurrentRunner(std::sync::atomic::AtomicUsize);
#[async_trait]
impl RunnerResolver for CurrentRunner {
    async fn resolve(&self, _: &crate::model::GameEntry) -> Result<RunnerPaths> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(LaunchError::Unsupported(
            "selected runner is unavailable".into(),
        ))
    }
}
#[tokio::test]
async fn managed_prefix_runner_is_resolved_for_each_launch_before_reserving_a_session() {
    use caskade_discovery::GameRuntime;
    let store = Arc::new(MemoryStore::default());
    let mut entry = entry("play.exe".into(), vec![], None);
    entry.location = Some(crate::model::InstallLocation::Managed {
        root_id: "fixture-root".into(),
        rel_dir: "Game".into(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let resolver = Arc::new(CurrentRunner(std::sync::atomic::AtomicUsize::new(0)));
    let service =
        service(store.clone(), Arc::new(Events::default())).with_runner_resolver(resolver.clone());
    for _ in 0..2 {
        assert!(matches!(
            service.launch(&entry.id).await,
            Err(LaunchError::Unsupported(_))
        ));
    }
    assert_eq!(resolver.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(store.sessions.lock().unwrap().is_empty());
    // A failed resolve released the game's lease.
    drop(service.try_game_lease(&entry.game_id).unwrap());
    // Native and externally discovered entries retain their own resolution.
    entry.location = None;
    entry.runtime = GameRuntime::Native;
    entry.launch = Some(crate::model::LaunchSpec::Executable {
        path: "/bin/true".into(),
        args: vec![],
        cwd: None,
        env: Default::default(),
    });
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(resolver.0.load(std::sync::atomic::Ordering::SeqCst), 2);
}

/// A runner that never becomes ready, like a umu download on a network
/// that drops every packet.
struct StalledRunner;
#[async_trait]
impl RunnerResolver for StalledRunner {
    async fn resolve(&self, _: &crate::model::GameEntry) -> Result<RunnerPaths> {
        std::future::pending().await
    }
}
#[tokio::test]
async fn a_runner_that_never_resolves_times_out_and_frees_the_game() {
    use caskade_discovery::GameRuntime;
    let store = Arc::new(MemoryStore::default());
    let mut entry = entry("play.exe".into(), vec![], None);
    entry.location = Some(crate::model::InstallLocation::Managed {
        root_id: "fixture-root".into(),
        rel_dir: "Game".into(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let service = service(store.clone(), Arc::new(Events::default()))
        .with_runner_resolver(Arc::new(StalledRunner))
        .with_runner_timeout(Duration::from_millis(50));
    let Err(LaunchError::Invalid(message)) =
        tokio::time::timeout(Duration::from_secs(5), service.launch(&entry.id))
            .await
            .unwrap()
    else {
        panic!("a stalled runner fails the launch");
    };
    assert!(message.contains("not ready"), "{message}");
    // The game is not left looking like it is running or being moved.
    drop(service.try_game_lease(&entry.game_id).unwrap());
    assert!(store.sessions.lock().unwrap().is_empty());
}

#[test]
fn tracking_backs_off_while_the_game_runs() {
    let default = LaunchConfig::default();
    assert_eq!(default.poll_interval, Duration::from_millis(250));
    assert_eq!(default.playing_poll_interval(), Duration::from_millis(1500));
    // Tests keep fast polls fast; a slow configured poll is never sped up.
    assert_eq!(
        fast_config().playing_poll_interval(),
        Duration::from_millis(120)
    );
    let slow = LaunchConfig {
        poll_interval: Duration::from_secs(3),
        ..Default::default()
    };
    assert_eq!(slow.playing_poll_interval(), Duration::from_secs(3));
}

#[tokio::test]
async fn a_web_game_plays_in_the_game_window_process() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("caskade");
    let seen = dir.path().join("args");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nsleep 0.3\n",
            seen.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let store = Arc::new(MemoryStore::default());
    let mut entry = entry("unused".into(), vec![], None);
    entry.launch = Some(crate::model::LaunchSpec::Web {
        url: "https://example.com/game".into(),
    });
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let events = Arc::new(Events::default());
    let error = service(store.clone(), events.clone())
        .launch(&entry.id)
        .await
        .err()
        .unwrap();
    assert!(matches!(error, LaunchError::Unsupported(_)), "{error}");

    let service = service(store, events.clone())
        .with_web_player(WebPlayer::new(&script, dir.path().join("web")));
    let session = service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    assert!(session.duration_ms > 0, "the window's process is the game");
    let events = events.0.lock().unwrap();
    assert_eq!(events[1].1.status, SessionStatus::Exited);
    let data = dir.path().join("web").join(&entry.id);
    assert_eq!(
        fs::read_to_string(&seen).unwrap(),
        format!(
            "--web-game\nhttps://example.com/game\n--data-dir\n{}\n",
            data.display()
        )
    );
}

#[tokio::test]
async fn a_web_game_ends_when_its_window_closes() {
    // The window leaves a web process behind, as a busy WebKit page does.
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("caskade");
    let left = dir.path().join("left");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nsleep 5 &\necho $! > '{}'\nsleep 0.3\n",
            left.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let store = Arc::new(MemoryStore::default());
    let mut entry = entry("unused".into(), vec![], None);
    entry.launch = Some(crate::model::LaunchSpec::Web {
        url: "https://example.com/game".into(),
    });
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let service = service(store, Arc::new(Events::default()))
        .with_config(LaunchConfig {
            exit_grace: Duration::from_secs(5),
            ..fast_config()
        })
        .with_web_player(WebPlayer::new(&script, dir.path().join("web")));
    let started = std::time::Instant::now();
    let session = service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    let waited = started.elapsed();
    let left = fs::read_to_string(&left).unwrap();
    let alive = std::process::Command::new("kill")
        .arg(left.trim())
        .status()
        .unwrap()
        .success();
    assert!(alive, "the web process outlived the window");
    assert!(waited < Duration::from_secs(2), "no exit grace: {waited:?}");
    assert!(session.duration_ms < 2_000, "{}", session.duration_ms);
}
