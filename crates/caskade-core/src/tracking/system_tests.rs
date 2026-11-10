#![cfg(unix)]
use crate::test_programs::copy_program;
use crate::tracking::{
    ProcessIdentity, ProcessMonitor, ProcessSnapshot, ProcessTracker, TrackingTarget,
};
use caskade_installer::runtime::alias::AliasStore;
use std::{collections::BTreeSet, fs, process::Stdio, sync::Arc, time::Duration};

fn identities(snapshot: &ProcessSnapshot) -> BTreeSet<ProcessIdentity> {
    snapshot.processes.iter().map(|p| p.identity).collect()
}

#[tokio::test]
async fn real_alias_launcher_forks_game_and_parent_exits_first() {
    let t = tempfile::tempdir().unwrap();
    let game = t.path().join("Café Game");
    fs::create_dir(&game).unwrap();
    copy_program("/bin/sleep", game.join("fixture-child"));
    let aliases = AliasStore::new(t.path().join("aliases"));
    let alias = aliases.link("fixture", &game).unwrap();
    let monitor = ProcessMonitor::default();
    let baseline = monitor.snapshot().await.unwrap();
    let mut tracker = ProcessTracker::new(
        TrackingTarget {
            real_dir: Some(game),
            alias_dir: Some(alias.clone()),
            ..Default::default()
        },
        &baseline,
    );
    let mut launcher = tokio::process::Command::new("/bin/sh")
        .args(["-c", r#""$1" 1 &"#, "launcher"])
        .arg(alias.join("fixture-child"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Launcher deliberately exits immediately and reparents the child.
    assert!(launcher.wait().await.unwrap().success());
    let mut seen = false;
    for _ in 0..100 {
        let snapshot = monitor.snapshot().await.unwrap();
        let active = tracker.observe(&snapshot);
        if !active.is_empty() {
            seen = true;
        } else if seen {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(seen, "real sysinfo snapshot never found forked alias game");
    assert!(
        tracker
            .observe(&monitor.snapshot().await.unwrap())
            .is_empty()
    );
}

/// Tracking polls read a fresh process's metadata on every poll: `exec`
/// replaces its executable without changing its identity.
#[tokio::test]
async fn tracking_polls_reread_a_process_that_execs() {
    let t = tempfile::tempdir().unwrap();
    let game = fs::canonicalize(t.path()).unwrap().join("fixture-game");
    copy_program("/bin/sleep", &game);
    let monitor = ProcessMonitor::default();
    let baseline = Arc::new(identities(&monitor.snapshot().await.unwrap()));
    let mut launcher = tokio::process::Command::new("/bin/sh")
        .args(["-c", r#"sleep 0.4; exec "$1" 5"#, "launcher"])
        .arg(&game)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = launcher.id().unwrap();
    let mut before = None;
    let mut after = None;
    for _ in 0..100 {
        let snapshot = monitor.snapshot_past(baseline.clone()).await.unwrap();
        let exe = snapshot
            .processes
            .iter()
            .find(|p| p.identity.pid == pid)
            .and_then(|p| p.executable.clone());
        if exe.as_deref() == Some(game.as_path()) {
            after = exe;
            break;
        }
        before = before.or(exe);
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let _ = launcher.kill().await;
    assert!(before.is_some_and(|exe| exe != game), "never saw the shell");
    assert_eq!(after, Some(game));
}

#[cfg(target_os = "linux")]
mod clock_step {
    use super::*;
    use crate::tracking::{ClockStep, SystemProcesses};

    /// A running copy of `sleep` from `dir`, and that copy's path.
    async fn game(dir: &std::path::Path) -> (tokio::process::Child, std::path::PathBuf) {
        let game = dir.join("fixture-game");
        copy_program("/bin/sleep", &game);
        let running = tokio::process::Command::new(&game)
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        (running, game)
    }

    /// A restart after a clock step reads the game's start anew. The session
    /// it recorded must still find the game, or it ends with the game running.
    #[tokio::test]
    async fn recovery_finds_the_game_after_the_clock_steps() {
        let t = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(t.path()).unwrap();
        let (running, _) = game(&dir).await;
        let pid = running.id().unwrap();
        let recorded = ProcessIdentity::lookup(pid).unwrap();

        let _stepped = ClockStep::by(7200);
        let after = SystemProcesses::default().snapshot();
        let mut tracker = ProcessTracker::new(
            TrackingTarget {
                real_dir: Some(dir),
                ..Default::default()
            },
            &after,
        );
        tracker.recover(&[recorded]);
        assert_eq!(tracker.observe(&after), vec![recorded]);
    }

    /// The same restart with a PID that was reused while Caskade was away.
    #[tokio::test]
    async fn recovery_leaves_a_reused_pid_after_the_clock_steps() {
        let t = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(t.path()).unwrap();
        let (running, _) = game(&dir).await;
        let now = ProcessIdentity::lookup(running.id().unwrap()).unwrap();
        let recorded = ProcessIdentity {
            started_at: now.started_at - 1,
            ..now
        };

        let _stepped = ClockStep::by(7200);
        let after = SystemProcesses::default().snapshot();
        let mut tracker = ProcessTracker::new(TrackingTarget::default(), &after);
        tracker.recover(&[recorded]);
        assert!(tracker.observe(&after).is_empty());
    }

    /// A clock stepped forward after the monitor's first read leaves what
    /// sysinfo reports behind the session's start. A game started then is
    /// still news to a tracking poll.
    #[tokio::test]
    async fn tracking_polls_read_a_game_started_after_the_clock_steps() {
        let t = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(t.path()).unwrap();
        let mut source = SystemProcesses::default();
        let baseline = identities(&source.snapshot());

        let _stepped = ClockStep::by(-7200);
        let (running, game) = game(&dir).await;
        let pid = running.id().unwrap();
        let snapshot = source.snapshot_past(&baseline);
        let seen = snapshot.processes.iter().find(|p| p.identity.pid == pid);
        assert_eq!(seen.and_then(|p| p.executable.clone()), Some(game));
    }
}
