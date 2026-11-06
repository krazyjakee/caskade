use super::*;
fn process(pid: u32, parent: Option<u32>, exe: &str) -> ProcessInfo {
    ProcessInfo {
        identity: ProcessIdentity {
            pid,
            started_at: 100,
            boot: 0,
        },
        parent,
        name: None,
        executable: Some(exe.into()),
        cwd: None,
        arguments: vec![],
        steam_app_id: None,
    }
}
fn target() -> TrackingTarget {
    TrackingTarget {
        real_dir: Some("/games/Fixture".into()),
        ..Default::default()
    }
}
#[test]
fn launcher_exit_does_not_end_child_or_grandchild() {
    let launcher = process(1, None, "/usr/bin/launcher");
    let child = process(2, Some(1), "/games/Fixture/game");
    let grandchild = process(3, Some(2), "/usr/bin/wine-worker");
    let mut t = ProcessTracker::new(target(), &ProcessSnapshot::default());
    t.launcher(launcher.identity);
    assert_eq!(
        t.observe(&ProcessSnapshot {
            processes: vec![child.clone(), launcher]
        }),
        vec![child.identity]
    );
    assert_eq!(
        t.observe(&ProcessSnapshot {
            processes: vec![grandchild.clone(), child]
        })
        .len(),
        2
    );
    assert_eq!(
        t.observe(&ProcessSnapshot {
            processes: vec![grandchild.clone()]
        }),
        vec![grandchild.identity]
    );
    assert!(t.observe(&ProcessSnapshot::default()).is_empty());
}
#[test]
fn baseline_and_component_boundary_prevent_other_games() {
    let old = process(1, None, "/games/Fixture/game");
    let mut t = ProcessTracker::new(
        target(),
        &ProcessSnapshot {
            processes: vec![old.clone()],
        },
    );
    assert!(
        t.observe(&ProcessSnapshot {
            processes: vec![old, process(2, None, "/games/Fixture-other/game")]
        })
        .is_empty()
    );
}
#[test]
fn pid_reuse_does_not_inherit_a_finished_game() {
    let game = process(1, None, "/games/Fixture/game");
    let mut t = ProcessTracker::new(target(), &ProcessSnapshot::default());
    assert_eq!(
        t.observe(&ProcessSnapshot {
            processes: vec![game]
        })
        .len(),
        1
    );
    let mut reused = process(1, None, "/usr/bin/other");
    reused.identity.started_at = 101;
    let mut child = process(2, Some(1), "/usr/bin/other-child");
    child.identity.started_at = 102;
    assert!(
        t.observe(&ProcessSnapshot {
            processes: vec![reused, child]
        })
        .is_empty()
    );
}
#[test]
fn steam_hint_works_when_game_is_reparented() {
    let target = TrackingTarget {
        steam_app_id: Some(620),
        ..Default::default()
    };
    let mut p = process(99, Some(1), "/runner/proton");
    p.steam_app_id = Some(620);
    let mut t = ProcessTracker::new(target, &ProcessSnapshot::default());
    assert_eq!(
        t.observe(&ProcessSnapshot {
            processes: vec![p.clone()]
        }),
        vec![p.identity]
    );
}
#[test]
fn recovery_never_attaches_to_reused_pid() {
    let id = ProcessIdentity {
        pid: 1,
        started_at: 90,
        boot: 0,
    };
    let mut t = ProcessTracker::new(target(), &ProcessSnapshot::default());
    t.recover(&[id]);
    assert!(
        t.observe(&ProcessSnapshot {
            processes: vec![process(1, None, "/other")]
        })
        .is_empty()
    );
}
#[test]
fn alias_and_real_executable_match() {
    let target = TrackingTarget {
        real_dir: Some("/new/game".into()),
        alias_dir: Some("/stable/id".into()),
        ..Default::default()
    };
    let mut t = ProcessTracker::new(target, &ProcessSnapshot::default());
    assert_eq!(
        t.observe(&ProcessSnapshot {
            processes: vec![
                process(1, None, "/stable/id/game"),
                process(2, None, "/new/game/game")
            ]
        })
        .len(),
        2
    );
}
#[test]
fn names_require_cwd_or_descendant_evidence() {
    let mut target = target();
    target.exe_names.push("game".into());
    let mut t = ProcessTracker::new(target, &ProcessSnapshot::default());
    assert!(
        t.observe(&ProcessSnapshot {
            processes: vec![process(1, None, "/elsewhere/game")]
        })
        .is_empty()
    );
    let mut p = process(2, None, "/elsewhere/game");
    p.cwd = Some("/games/Fixture".into());
    assert_eq!(t.observe(&ProcessSnapshot { processes: vec![p] }).len(), 1);
}

#[test]
fn known_launcher_and_background_daemon_never_count_as_the_game() {
    let mut target = target();
    target.steam_app_id = Some(620);
    let mut steam = process(1, None, "/usr/bin/steam");
    steam.steam_app_id = Some(620);
    steam.arguments.push("/games/Fixture/game".into());
    let mut tracker = ProcessTracker::new(target, &ProcessSnapshot::default());
    tracker.launcher(steam.identity);
    assert!(
        tracker
            .observe(&ProcessSnapshot {
                processes: vec![steam.clone()]
            })
            .is_empty()
    );
    let game = process(2, Some(1), "/games/Fixture/game");
    assert_eq!(
        tracker
            .observe(&ProcessSnapshot {
                processes: vec![steam, game]
            })
            .len(),
        1
    );
    assert!(
        tracker
            .observe(&ProcessSnapshot {
                processes: vec![process(3, Some(2), "/usr/bin/wineserver")]
            })
            .is_empty()
    );
}

#[test]
fn wine_process_name_or_windows_argv_needs_game_cwd_or_parent() {
    let mut target = target();
    target.exe_names = vec!["fixture.exe".into()];
    let mut tracker = ProcessTracker::new(target, &Default::default());
    let mut process = process(10, None, "/usr/lib/wine/wine64-preloader");
    process.name = Some("fixture.exe".into());
    assert!(
        tracker
            .observe(&ProcessSnapshot {
                processes: vec![process.clone()]
            })
            .is_empty()
    );
    process.cwd = Some("/games/Fixture".into());
    assert_eq!(
        tracker.observe(&ProcessSnapshot {
            processes: vec![process.clone()]
        }),
        vec![process.identity]
    );
    let mut other = process;
    other.identity.pid = 11;
    other.name = None;
    other.arguments = vec![r"G:\fixture.exe".into()];
    assert_eq!(
        tracker
            .observe(&ProcessSnapshot {
                processes: vec![other]
            })
            .len(),
        1
    );
}
