//! #125: backend plugins on the hook bus, with the test guest
//! (`tests/guest`): events, interceptors, lazy activation, capability checks
//! and the circuit breaker.

use crate::guest;
use caskade_core::{
    bus::{Bus, Event, GameSession, Outcome},
    db::LibraryStore,
    launch::{LaunchBefore, LaunchCommand, LaunchService},
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
};
use caskade_plugin_host::{HostOptions, Limits, PluginHooks, PluginHost, PluginServices};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

struct Setup {
    temp: tempfile::TempDir,
    store: LibraryStore,
    bus: Bus,
    host: PluginHost,
    hooks: Arc<PluginHooks>,
}

/// Waits, up to a bound generous enough for a loaded CI runner, until `done`
/// holds: re-attaching a plugin instantiates its component again.
async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Plugins as `(id, manifest additions)`, all running the test guest.
async fn setup(plugins: &[(&str, Value)]) -> Setup {
    setup_with(plugins, Bus::default()).await
}

async fn setup_with(plugins: &[(&str, Value)], bus: Bus) -> Setup {
    let temp = tempfile::tempdir().unwrap();
    for (id, extra) in plugins {
        let dir = temp.path().join("plugins").join(id);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::copy(guest(), dir.join("dist/backend.wasm")).unwrap();
        let mut manifest = json!({
            "id": id,
            "name": id,
            "version": "0.1.0",
            "api": "^0.1",
            "backend": "dist/backend.wasm",
        });
        for (key, value) in extra.as_object().unwrap() {
            manifest[key] = value.clone();
        }
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    }
    let store = LibraryStore::in_memory().await.unwrap();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
            settings: None,
        },
        HostOptions {
            folders: vec![temp.path().join("plugins")],
            data: temp.path().join("plugin-data"),
            limits: Limits {
                memory: 32 << 20,
                call_timeout: Duration::from_millis(300),
                ..Limits::default()
            },
        },
    )
    .unwrap();
    host.discover().await.unwrap();
    for (id, _) in plugins {
        host.set_enabled(id, true).await.unwrap();
    }
    let hooks = PluginHooks::new(host.clone(), bus.clone());
    hooks.sync().await;
    Setup {
        temp,
        store,
        bus,
        host,
        hooks,
    }
}

fn wrapper(process: bool, activation: &[&str]) -> Value {
    json!({
        "permissions": {"process": process},
        "activation": activation,
        "contributes": {"hooks": ["launch.before"]},
    })
}

fn command(args: &[&str]) -> LaunchCommand {
    let mut command = LaunchCommand::executable("/games/celeste/Celeste");
    command.args = args.iter().map(|arg| (*arg).to_owned()).collect();
    command
}

#[tokio::test]
async fn a_process_plugin_rewrites_launch_before_and_can_cancel() {
    let s = setup(&[("dev.test.gamemode", wrapper(true, &[]))]).await;
    let report = s.bus.run::<LaunchBefore>(command(&[])).await;
    let Outcome::Continue(rewritten) = report.outcome else {
        panic!("cancelled: {:?}", report.outcome);
    };
    assert_eq!(
        rewritten.program,
        PathBuf::from("gamemoderun /games/celeste/Celeste")
    );
    assert!(report.skipped.is_empty());
    assert!(matches!(
        s.bus.run::<LaunchBefore>(command(&["cancel-me"])).await.outcome,
        Outcome::Cancelled { by, reason } if by == "plugin:dev.test.gamemode" && reason.contains("not today")
    ));
}

#[tokio::test]
async fn a_rewrite_without_the_process_permission_is_refused() {
    let s = setup(&[("dev.test.sneaky", wrapper(false, &[]))]).await;
    let report = s.bus.run::<LaunchBefore>(command(&[])).await;
    assert_eq!(report.outcome, Outcome::Continue(command(&[])));
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0, "plugin:dev.test.sneaky");
    assert!(
        report.skipped[0].1.contains("process permission"),
        "{:?}",
        report.skipped
    );
    // Cancelling doesn't change what runs, so it needs no permission.
    assert!(matches!(
        s.bus
            .run::<LaunchBefore>(command(&["cancel-me"]))
            .await
            .outcome,
        Outcome::Cancelled { .. }
    ));
}

#[tokio::test]
async fn events_start_a_plugin_only_on_its_activation_event() {
    let s = setup(&[
        (
            "dev.test.lazy",
            json!({"contributes": {"hooks": ["game.exited"]}}),
        ),
        (
            "dev.test.listener",
            json!({
                "activation": ["onEvent:game.exited"],
                "contributes": {"hooks": ["game.exited"]}
            }),
        ),
    ])
    .await;
    let session = GameSession {
        game_id: "g1".into(),
        entry_id: "e1".into(),
        session_id: "s1".into(),
        started_at_ms: 1,
        ended_at_ms: Some(2),
        duration_ms: 1,
    };
    assert!(!s.host.is_active("dev.test.listener"));
    let report = s.bus.emit_and_wait(Event::GameExited(session)).await;
    assert!(report.failed.is_empty(), "{:?}", report.failed);

    assert!(s.host.is_active("dev.test.listener"));
    let stored = s
        .store
        .plugin_kv_get("dev.test.listener", "event:game.exited")
        .await
        .unwrap()
        .expect("delivered");
    let payload: Value = serde_json::from_slice(&stored).unwrap();
    assert_eq!(payload["version"], 1);
    assert_eq!(payload["data"]["gameId"], "g1");
    assert_eq!(payload["data"]["durationMs"], 1);

    // Without an activation event it isn't started for this.
    assert!(!s.host.is_active("dev.test.lazy"));
    assert_eq!(
        s.store
            .plugin_kv_get("dev.test.lazy", "event:game.exited")
            .await
            .unwrap(),
        None
    );
    // Undeclared events never reach a plugin.
    s.bus
        .emit_and_wait(Event::LibraryGameAdded {
            game_id: "g2".into(),
        })
        .await;
    assert_eq!(
        s.store
            .plugin_kv_get("dev.test.listener", "event:library.game.added")
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn crashing_steps_are_skipped_then_the_plugin_is_disabled() {
    let s = setup(&[("dev.test.crashy", wrapper(true, &[]))]).await;
    let mut stream = s.bus.stream();
    // A trap, a timeout, then a success resets the count.
    for args in [&["trap-me"][..], &["spin-me"]] {
        let report = s.bus.run::<LaunchBefore>(command(args)).await;
        assert_eq!(report.outcome, Outcome::Continue(command(args)));
        assert_eq!(report.skipped[0].0, "plugin:dev.test.crashy");
    }
    assert!(matches!(
        s.bus.run::<LaunchBefore>(command(&[])).await.outcome,
        Outcome::Continue(next) if next.program.to_string_lossy().starts_with("gamemoderun")
    ));
    assert!(s.hooks.attached("dev.test.crashy"));
    for args in [&["trap-me"][..], &["spin-me"], &["trap-me"]] {
        let report = s.bus.run::<LaunchBefore>(command(args)).await;
        // Each failed step is skipped; the launch carries on unchanged.
        assert_eq!(report.outcome, Outcome::Continue(command(args)));
    }
    assert!(!s.hooks.attached("dev.test.crashy"));
    assert!(!s.host.enabled("dev.test.crashy"));
    let mut disabled = Vec::new();
    while let Ok(event) = stream.try_recv() {
        if let Event::PluginDisabled { plugin, reason } = event {
            assert!(reason.contains("3 failures"), "{reason}");
            disabled.push(plugin);
        }
    }
    assert_eq!(disabled, ["dev.test.crashy"]);
    // Off the bus: launches go through untouched.
    assert_eq!(
        s.bus.run::<LaunchBefore>(command(&[])).await.outcome,
        Outcome::Continue(command(&[]))
    );
    // Enabling again puts it back (the host's plugins.changed event
    // re-syncs the adapter) with a fresh count.
    s.host.set_enabled("dev.test.crashy", true).await.unwrap();
    eventually("the plugin back on the bus", || {
        s.hooks.attached("dev.test.crashy")
    })
    .await;
}

#[tokio::test]
async fn disabling_takes_a_plugin_off_the_bus() {
    let s = setup(&[("dev.test.gamemode", wrapper(true, &["onStartup"]))]).await;
    // onStartup starts it when attached.
    assert!(s.host.is_active("dev.test.gamemode"));
    s.host
        .set_enabled("dev.test.gamemode", false)
        .await
        .unwrap();
    s.hooks.sync().await;
    assert!(!s.hooks.attached("dev.test.gamemode"));
    assert_eq!(
        s.bus.run::<LaunchBefore>(command(&[])).await.outcome,
        Outcome::Continue(command(&[]))
    );
}

#[tokio::test]
async fn library_import_needs_library_write() {
    let s = setup(&[(
        "dev.test.importer",
        json!({"permissions": {"library": "read"}, "contributes": {"hooks": ["library.import"]}}),
    )])
    .await;
    // Attached for events, but its library.import step was not registered.
    assert!(s.hooks.attached("dev.test.importer"));
    let game = caskade_discovery::DetectedGame::new(SourceId::new("fixture"), "key", "cancel-me");
    let report = s
        .bus
        .run::<caskade_core::bus::LibraryImport>(game.clone())
        .await;
    assert_eq!(report.outcome, Outcome::Continue(game));
}

#[tokio::test]
async fn a_trapping_plugin_does_not_stop_a_real_launch() {
    let s = setup(&[("dev.test.crashy", wrapper(true, &[]))]).await;
    let game_dir = s.temp.path().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::copy("/bin/true", game_dir.join("game")).unwrap();
    let game = Game::new("Fixture");
    s.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: game_dir.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: game_dir.join("game"),
        args: vec!["trap-me".into()],
        cwd: Some(game_dir),
        env: BTreeMap::new(),
    });
    s.store.upsert_entry(&entry).await.unwrap();
    let launch = LaunchService::new(s.store.clone())
        .with_bus(s.bus.clone())
        .with_aliases(None);
    let session = launch
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    assert_eq!(
        s.store.sessions_for_game(&game.id, 10).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn opening_a_view_starts_a_plugin_with_that_activation_event() {
    let s = setup(&[(
        "dev.test.board",
        json!({"activation": ["onView:board"], "contributes": {"views": [{"id": "board", "title": "Board"}]}}),
    )])
    .await;
    s.hooks.view_opened("dev.test.board", "other").await;
    assert!(!s.host.is_active("dev.test.board"));
    s.hooks.view_opened("dev.test.board", "board").await;
    assert!(s.host.is_active("dev.test.board"));
}

/// #55: the installer's points reach plugins; `install.after` needs
/// `process` or an `fs` grant.
#[tokio::test]
async fn installer_points_reach_plugins_and_install_after_needs_process_or_fs() {
    use caskade_core::install::hooks::{InstallAfter, InstallMoved, InstalledGame, MovedGame};
    let installed = InstalledGame {
        job_id: "job".into(),
        game_id: "game".into(),
        title: "cancel-me".into(),
        dir: "/games/x".into(),
    };
    let s = setup(&[(
        "dev.test.tweaker",
        json!({"permissions": {"process": true}, "contributes": {"hooks": ["install.after", "install.moved"]}}),
    )])
    .await;
    let report = s.bus.run::<InstallAfter>(installed.clone()).await;
    assert!(
        matches!(&report.outcome, Outcome::Cancelled { reason, .. } if reason == "install.after: not today"),
        "{report:?}"
    );
    let moved = MovedGame {
        job_id: "cancel-me".into(),
        game_id: "game".into(),
        from: "/games/x".into(),
        to: "/other/x".into(),
    };
    let report = s.bus.run::<InstallMoved>(moved).await;
    assert!(
        matches!(report.outcome, Outcome::Cancelled { .. }),
        "{report:?}"
    );

    let s = setup(&[(
        "dev.test.idle",
        json!({"contributes": {"hooks": ["install.after"]}}),
    )])
    .await;
    assert!(s.hooks.attached("dev.test.idle"));
    let report = s.bus.run::<InstallAfter>(installed.clone()).await;
    assert_eq!(report.outcome, Outcome::Continue(installed));
}

#[tokio::test]
async fn settings_changes_are_not_forwarded_from_the_bus() {
    let s = setup(&[(
        "dev.test.listener",
        json!({"activation": ["onStartup"], "contributes": {"hooks": ["settings.changed"]}}),
    )])
    .await;
    s.bus
        .emit_and_wait(Event::SettingsChanged {
            namespace: "plugins.dev.other".into(),
            version: 1,
        })
        .await;
    assert_eq!(
        s.store
            .plugin_kv_get("dev.test.listener", "event:settings.changed")
            .await
            .unwrap(),
        None
    );
}

/// The bus's time box can end a step before the host's own limit does; the
/// call must still be counted and the plugin turned off, not cancelled
/// midway by the bus (seen live in #134 with both limits at 2 s).
#[tokio::test]
async fn the_circuit_breaker_trips_when_the_bus_gives_up_first() {
    let bus = Bus::new(caskade_core::bus::BusOptions {
        interceptor_timeout: Duration::from_millis(100),
        ..Default::default()
    });
    let s = setup_with(&[("dev.test.crashy", wrapper(true, &[]))], bus).await;
    let mut stream = s.bus.stream();
    for _ in 0..3 {
        let report = s.bus.run::<LaunchBefore>(command(&["spin-me"])).await;
        assert_eq!(report.outcome, Outcome::Continue(command(&["spin-me"])));
    }
    let mut disabled = None;
    eventually("the plugin disabled", || {
        while let Ok(event) = stream.try_recv() {
            if let Event::PluginDisabled { plugin, .. } = event {
                disabled = Some(plugin);
            }
        }
        disabled.is_some() && !s.host.enabled("dev.test.crashy")
    })
    .await;
    assert_eq!(disabled.as_deref(), Some("dev.test.crashy"));
    assert!(!s.host.enabled("dev.test.crashy"));
    let row = s
        .store
        .plugin_state("dev.test.crashy")
        .await
        .unwrap()
        .unwrap();
    assert!(!row.enabled, "the disable is persisted");
}
