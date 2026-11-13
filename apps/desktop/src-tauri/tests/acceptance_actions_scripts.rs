//! Milestone acceptance (#194) for launch actions and lifecycle scripts,
//! through the app's own commands over IPC, on the real core: a named
//! mod-manager play action and a tool, global and per-game lifecycle
//! commands, then tracking, phase order, a time limit, cancellation and
//! cleanup, with fake processes (shell scripts and a copied `sleep`).
#![cfg(target_os = "linux")]

use crate::ipc::invoke;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    launch::{LaunchConfig, LaunchService},
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
};
use caskade_desktop::commands::{actions, launch, scripts};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn wait(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "waited too long for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A game in `dir` whose own launch is `game.sh`.
fn game(store: &LibraryStore, dir: &Path, title: &str) -> (String, String) {
    tauri::async_runtime::block_on(async {
        let game = store.upsert_game(&Game::new(title)).await.unwrap();
        script(&dir.join("game.sh"), "sleep 0.2");
        let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), title);
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External { path: dir.into() });
        entry.launch = Some(LaunchSpec::Executable {
            path: dir.join("game.sh"),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
        });
        let entry = store.upsert_entry(&entry).await.unwrap();
        (game.id, entry.id)
    })
}

fn app(store: &LibraryStore) -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
    let bus = Bus::default();
    let service = LaunchService::new(store.clone())
        .with_bus(bus.clone())
        .with_aliases(None)
        .with_config(LaunchConfig {
            poll_interval: Duration::from_millis(20),
            launch_timeout: Duration::from_secs(5),
            exit_grace: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(50),
        });
    let app = mock_builder()
        .manage(store.clone())
        .manage(service)
        .manage(bus)
        .invoke_handler(tauri::generate_handler![
            actions::game_action_save,
            actions::game_action_settings_save,
            actions::game_actions,
            actions::game_tool_run,
            scripts::lifecycle_script_save,
            launch::launch_game_with,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, window)
}

/// A shell lifecycle script that appends `<name> <phase> <status>` to `log`.
fn logging(scope: &Value, phase: &str, name: &str, log: &Path, extra: &str) -> Value {
    json!({
        "id": "", "scope": scope, "name": name, "enabled": true, "phase": phase,
        "command": {"type": "shell", "shell": "sh", "script": format!(
            "{extra}echo \"{name} $CASKADE_PHASE ${{CASKADE_STATUS}} ${{CASKADE_ACTION_NAME}} pid=${{CASKADE_PID:+yes}}\" >> '{}'",
            log.display()
        )},
        "cwd": null, "timeoutSecs": 30,
    })
}

fn save_script(window: &tauri::WebviewWindow<MockRuntime>, script: Value) {
    let scope = script["scope"].clone();
    invoke(
        window,
        "lifecycle_script_save",
        json!({"scope": scope, "script": script}),
    )
    .unwrap();
}

#[test]
fn a_mod_manager_action_with_lifecycle_scripts_tracks_the_game_in_phase_order() {
    let temp = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let (game_id, entry_id) = game(&store, temp.path(), "Modded Game");
    let (_app, window) = app(&store);
    let log = temp.path().join("phases.log");

    // The mod manager starts the game (a copied `sleep` named `modgame`)
    // and exits at once; tracking follows the game by its process name.
    let modgame = temp.path().join("modgame");
    fs::copy("/bin/sleep", &modgame).unwrap();
    script(
        &temp.path().join("manager.sh"),
        &format!("'{}' 0.8 &\nexit 0", modgame.display()),
    );
    let modded = invoke(
        &window,
        "game_action_save",
        json!({"gameId": game_id, "action": {
            "id": "", "entryId": entry_id, "name": "Modded (MO2)", "kind": "play",
            "target": {"type": "executable", "path": temp.path().join("manager.sh"), "args": [], "cwd": null, "env": {}},
            "tracking": {"mode": "process_name", "folder": null, "names": ["modgame"]},
        }}),
    )
    .unwrap();
    let configured = temp.path().join("configured");
    script(
        &temp.path().join("configure.sh"),
        &format!("touch '{}'", configured.display()),
    );
    let tool = invoke(
        &window,
        "game_action_save",
        json!({"gameId": game_id, "action": {
            "id": "", "entryId": entry_id, "name": "Configure", "kind": "utility",
            "target": {"type": "executable", "path": temp.path().join("configure.sh"), "args": [], "cwd": null, "env": {}},
            "tracking": {"mode": "automatic", "folder": null, "names": []},
        }}),
    )
    .unwrap();
    invoke(
        &window,
        "game_action_settings_save",
        json!({"gameId": game_id, "entryId": entry_id, "settings": {
            "defaultAction": modded["id"], "ask": false, "builtIn": {}
        }}),
    )
    .unwrap();

    // Lifecycle commands: global and per game, every phase.
    let global = json!({"type": "global"});
    let per_game = json!({"type": "game", "gameId": game_id});
    save_script(
        &window,
        logging(&global, "before_launch", "global", &log, ""),
    );
    save_script(
        &window,
        logging(&per_game, "before_launch", "game", &log, ""),
    );
    save_script(&window, logging(&per_game, "after_start", "game", &log, ""));
    save_script(&window, logging(&per_game, "after_exit", "game", &log, ""));
    save_script(&window, logging(&global, "after_exit", "global", &log, ""));

    let started = invoke(
        &window,
        "launch_game_with",
        json!({"entryId": entry_id, "choice": null}),
    )
    .unwrap();
    assert_eq!(started["type"], "started", "{started}");
    let sessions =
        || tauri::async_runtime::block_on(store.sessions_for_game(&game_id, 10)).unwrap();
    wait("the session to end", || {
        sessions().first().is_some_and(|s| s.ended_at_ms.is_some())
    });
    wait("the cleanup scripts", || lines(&log).len() >= 5);
    assert_eq!(
        lines(&log),
        [
            "global before_launch  Modded (MO2) pid=",
            "game before_launch  Modded (MO2) pid=",
            "game after_start  Modded (MO2) pid=yes",
            "game after_exit exited Modded (MO2) pid=yes",
            "global after_exit exited Modded (MO2) pid=yes",
        ]
    );
    let session = sessions().remove(0);
    assert_eq!(session.entry_id, entry_id);
    assert!(
        session.duration_ms >= 500,
        "the game's time counts, not the manager's: {} ms",
        session.duration_ms
    );

    // The tool runs without a session, playtime or lifecycle scripts.
    let ran = invoke(
        &window,
        "game_tool_run",
        json!({"entryId": entry_id, "actionId": tool["id"]}),
    )
    .unwrap();
    assert_eq!(ran["type"], "started");
    wait("the tool", || configured.exists());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(sessions().len(), 1);
    assert_eq!(lines(&log).len(), 5);
}

#[test]
fn a_time_limit_stops_the_launch_and_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let (_app, window) = app(&store);

    // A before-launch step that runs out of time refuses the launch.
    let slow_dir = temp.path().join("slow");
    fs::create_dir(&slow_dir).unwrap();
    let (slow_game, slow_entry) = game(&store, &slow_dir, "Slow Prep");
    let slow_log = temp.path().join("slow.log");
    let scope = json!({"type": "game", "gameId": slow_game});
    let mut prepare = logging(&scope, "before_launch", "prep", &slow_log, "sleep 30; ");
    prepare["timeoutSecs"] = json!(1);
    save_script(&window, prepare);
    save_script(
        &window,
        logging(&scope, "after_exit", "cleanup", &slow_log, ""),
    );
    let refused = invoke(
        &window,
        "launch_game_with",
        json!({"entryId": slow_entry, "choice": null}),
    )
    .unwrap_err();
    assert_eq!(refused["code"], "refused", "{refused}");
    assert!(
        refused["summary"]
            .as_str()
            .unwrap()
            .contains("ran out of time"),
        "{refused}"
    );
    assert_eq!(lines(&slow_log), ["cleanup after_exit failed Play pid="]);

    assert!(
        tauri::async_runtime::block_on(store.sessions_for_game(&slow_game, 5))
            .unwrap()
            .is_empty(),
        "no session for a launch that never started"
    );
}
