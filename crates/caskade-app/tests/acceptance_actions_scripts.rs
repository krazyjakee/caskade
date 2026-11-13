//! Milestone acceptance (#194) for launch actions and lifecycle scripts,
//! through the app's own commands, on the real core: a named mod-manager
//! play action and a tool, global and per-game lifecycle commands, then
//! tracking, phase order, a time limit, cancellation and cleanup, with fake
//! processes (shell scripts and a copied `sleep`).
#![cfg(target_os = "linux")]

use caskade_app::commands::{actions, launch, scripts};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    launch::{LaunchConfig, LaunchService},
    model::{
        EntryActionSettings, Game, GameAction, GameEntry, InstallLocation, InstallState,
        LaunchSpec, SourceId,
    },
    scripts::{LifecycleScript, ScriptScope},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

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
async fn wait(what: &str, done: impl AsyncFn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done().await {
        assert!(Instant::now() < deadline, "waited too long for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `value` as the type a command takes, as the UI would send it.
fn from<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

/// A command's answer as the UI reads it.
fn to<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}

/// A game in `dir` whose own launch is `game.sh`.
async fn game(store: &LibraryStore, dir: &Path, title: &str) -> (String, String) {
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
}

fn service(store: &LibraryStore, bus: &Bus) -> LaunchService {
    LaunchService::new(store.clone())
        .with_bus(bus.clone())
        .with_aliases(None)
        .with_config(LaunchConfig {
            poll_interval: Duration::from_millis(20),
            launch_timeout: Duration::from_secs(5),
            exit_grace: Duration::from_millis(100),
            heartbeat_interval: Duration::from_millis(50),
        })
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

async fn save_script(store: &LibraryStore, script: Value) {
    let scope: ScriptScope = from(script["scope"].clone());
    let script: LifecycleScript = from(script);
    scripts::lifecycle_script_save(store, &scope, script)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mod_manager_action_with_lifecycle_scripts_tracks_the_game_in_phase_order() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let (game_id, entry_id) = game(&store, temp.path(), "Modded Game").await;
    let bus = Bus::default();
    let service = service(&store, &bus);
    let log = temp.path().join("phases.log");

    // The mod manager starts the game (a copied `sleep` named `modgame`)
    // and exits at once; tracking follows the game by its process name.
    let modgame = temp.path().join("modgame");
    fs::copy("/bin/sleep", &modgame).unwrap();
    script(
        &temp.path().join("manager.sh"),
        &format!("'{}' 0.8 &\nexit 0", modgame.display()),
    );
    let modded: GameAction = from(json!({
        "id": "", "entryId": entry_id, "name": "Modded (MO2)", "kind": "play",
        "target": {"type": "executable", "path": temp.path().join("manager.sh"), "args": [], "cwd": null, "env": {}},
        "tracking": {"mode": "process_name", "folder": null, "names": ["modgame"]},
    }));
    let modded = to(actions::game_action_save(&store, &bus, &game_id, modded)
        .await
        .unwrap());
    let configured = temp.path().join("configured");
    script(
        &temp.path().join("configure.sh"),
        &format!("touch '{}'", configured.display()),
    );
    let tool: GameAction = from(json!({
        "id": "", "entryId": entry_id, "name": "Configure", "kind": "utility",
        "target": {"type": "executable", "path": temp.path().join("configure.sh"), "args": [], "cwd": null, "env": {}},
        "tracking": {"mode": "automatic", "folder": null, "names": []},
    }));
    let tool = to(actions::game_action_save(&store, &bus, &game_id, tool)
        .await
        .unwrap());
    let settings: EntryActionSettings = from(json!({
        "defaultAction": modded["id"], "ask": false, "builtIn": {}
    }));
    actions::game_action_settings_save(&store, &bus, &game_id, &entry_id, settings)
        .await
        .unwrap();

    // Lifecycle commands: global and per game, every phase.
    let global = json!({"type": "global"});
    let per_game = json!({"type": "game", "gameId": game_id});
    save_script(
        &store,
        logging(&global, "before_launch", "global", &log, ""),
    )
    .await;
    save_script(
        &store,
        logging(&per_game, "before_launch", "game", &log, ""),
    )
    .await;
    save_script(&store, logging(&per_game, "after_start", "game", &log, "")).await;
    save_script(&store, logging(&per_game, "after_exit", "game", &log, "")).await;
    save_script(&store, logging(&global, "after_exit", "global", &log, "")).await;

    let started = to(launch::launch_game_with(&service, &entry_id, None)
        .await
        .unwrap());
    assert_eq!(started["type"], "started", "{started}");
    let sessions = async || store.sessions_for_game(&game_id, 10).await.unwrap();
    wait("the session to end", async || {
        sessions()
            .await
            .first()
            .is_some_and(|s| s.ended_at_ms.is_some())
    })
    .await;
    wait("the cleanup scripts", async || lines(&log).len() >= 5).await;
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
    let session = sessions().await.remove(0);
    assert_eq!(session.entry_id, entry_id);
    assert!(
        session.duration_ms >= 500,
        "the game's time counts, not the manager's: {} ms",
        session.duration_ms
    );

    // The tool runs without a session, playtime or lifecycle scripts.
    let ran = to(
        actions::game_tool_run(&service, &entry_id, tool["id"].as_str().unwrap())
            .await
            .unwrap(),
    );
    assert_eq!(ran["type"], "started");
    wait("the tool", async || configured.exists()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(sessions().await.len(), 1);
    assert_eq!(lines(&log).len(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_time_limit_stops_the_launch_and_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let service = service(&store, &Bus::default());

    // A before-launch step that runs out of time refuses the launch.
    let slow_dir = temp.path().join("slow");
    fs::create_dir(&slow_dir).unwrap();
    let (slow_game, slow_entry) = game(&store, &slow_dir, "Slow Prep").await;
    let slow_log = temp.path().join("slow.log");
    let scope = json!({"type": "game", "gameId": slow_game});
    let mut prepare = logging(&scope, "before_launch", "prep", &slow_log, "sleep 30; ");
    prepare["timeoutSecs"] = json!(1);
    save_script(&store, prepare).await;
    save_script(
        &store,
        logging(&scope, "after_exit", "cleanup", &slow_log, ""),
    )
    .await;
    let refused = to(launch::launch_game_with(&service, &slow_entry, None)
        .await
        .unwrap_err());
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
        store
            .sessions_for_game(&slow_game, 5)
            .await
            .unwrap()
            .is_empty(),
        "no session for a launch that never started"
    );
}
