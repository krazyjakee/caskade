//! Launch actions through IPC: add, list, default, order, run a tool.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    launch::LaunchService,
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
};
use serde_json::json;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn actions_are_added_chosen_ordered_and_run_through_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let tool = temp.path().join("configure.sh");
    let ran = temp.path().join("ran");
    std::fs::write(&tool, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (store, game, entry) = tauri::async_runtime::block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = store.upsert_game(&Game::new("Actions")).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "ipc");
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: temp.path().into(),
        });
        entry.launch = Some(LaunchSpec::Executable {
            path: temp.path().join("game"),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
        });
        let entry = store.upsert_entry(&entry).await.unwrap();
        (store, game, entry)
    });
    let bus = Bus::default();
    let mut events = bus.stream();
    let app = mock_builder()
        .manage(store.clone())
        .manage(LaunchService::new(store.clone()).with_aliases(None))
        .manage(bus)
        .invoke_handler(tauri::generate_handler![
            game_actions,
            game_action_save,
            game_action_delete,
            game_actions_order,
            game_action_settings_save,
            game_tool_run,
            crate::commands::launch::launch_game_with
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let listed = invoke(&window, "game_actions", json!({"gameId": game.id})).unwrap();
    assert_eq!(listed[0]["entryId"], entry.id);
    assert_eq!(listed[0]["editableBuiltIn"], false);
    assert_eq!(listed[0]["actions"][0]["name"], "Play");
    let add = |name: &str, kind: &str| {
        invoke(
            &window,
            "game_action_save",
            json!({"gameId": game.id, "action": {
                "id": "", "entryId": entry.id, "name": name, "kind": kind,
                "target": {"type": "executable", "path": tool, "args": ["a b"], "cwd": null, "env": {}},
                "tracking": {"mode": "automatic", "folder": null, "names": []},
            }}),
        )
    };
    let configure = add("Configure", "utility").unwrap();
    let modded = add("Modded", "play").unwrap();
    assert!(matches!(
        events.try_recv(),
        Ok(Event::LibraryGameUpdated { game_id }) if game_id == game.id
    ));
    let refused = add("", "play").unwrap_err();
    assert_eq!(refused["code"], "invalid");

    invoke(
        &window,
        "game_actions_order",
        json!({"gameId": game.id, "entryId": entry.id, "ids": [modded["id"], configure["id"]]}),
    )
    .unwrap();
    let settings = invoke(
        &window,
        "game_action_settings_save",
        json!({"gameId": game.id, "entryId": entry.id, "settings": {"defaultAction": modded["id"], "ask": true, "builtIn": {}}}),
    )
    .unwrap();
    assert_eq!(settings["defaultAction"], modded["id"]);
    let listed = invoke(&window, "game_actions", json!({"gameId": game.id})).unwrap();
    let names: Vec<_> = listed[0]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| action["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Play", "Modded", "Configure"]);
    // Another game's release can't be changed through this one.
    let elsewhere = invoke(
        &window,
        "game_action_settings_save",
        json!({"gameId": "other", "entryId": entry.id, "settings": {"ask": false, "builtIn": {}}}),
    )
    .unwrap_err();
    assert_eq!(elsewhere["code"], "invalid");

    let outcome = invoke(
        &window,
        "game_tool_run",
        json!({"entryId": entry.id, "actionId": configure["id"]}),
    )
    .unwrap();
    assert_eq!(outcome["type"], "started");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ran.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ran.exists());
    let sessions = tauri::async_runtime::block_on(store.sessions_for_game(&game.id, 10)).unwrap();
    assert!(sessions.is_empty());

    // An action-only launch: Play starts the chosen action's program, with
    // its arguments as given, in a session of the release.
    let played = temp.path().join("played");
    let game_script = temp.path().join("modded.sh");
    std::fs::write(
        &game_script,
        format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", played.display()),
    )
    .unwrap();
    std::fs::set_permissions(&game_script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut changed = modded.clone();
    changed["target"]["path"] = json!(game_script);
    changed["tracking"]["mode"] = json!("original_process");
    invoke(
        &window,
        "game_action_save",
        json!({"gameId": game.id, "action": changed}),
    )
    .unwrap();
    let started = invoke(
        &window,
        "launch_game_with",
        json!({"entryId": entry.id, "choice": {"action": modded["id"]}}),
    )
    .unwrap();
    assert_eq!(started["type"], "started");
    assert_eq!(started["ticket"]["entryId"], entry.id);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !played.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&played).unwrap(), "a b");

    assert_eq!(
        invoke(
            &window,
            "game_action_delete",
            json!({"gameId": game.id, "id": modded["id"]}),
        )
        .unwrap(),
        true
    );
}
