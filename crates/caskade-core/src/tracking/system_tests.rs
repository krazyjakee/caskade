#![cfg(unix)]
use crate::tracking::{ProcessMonitor, ProcessTracker, TrackingTarget};
use caskade_installer::runtime::alias::AliasStore;
use std::{fs, process::Stdio, time::Duration};

#[tokio::test]
async fn real_alias_launcher_forks_game_and_parent_exits_first() {
    let t = tempfile::tempdir().unwrap();
    let game = t.path().join("Café Game");
    fs::create_dir(&game).unwrap();
    fs::copy("/bin/sleep", game.join("fixture-child")).unwrap();
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
    fs::copy("/bin/sleep", &game).unwrap();
    let monitor = ProcessMonitor::default();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .saturating_sub(2);
    let _ = monitor.snapshot().await.unwrap();
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
        let snapshot = monitor.snapshot_since(since).await.unwrap();
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
