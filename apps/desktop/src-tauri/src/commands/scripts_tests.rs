//! Lifecycle scripts through IPC; who may change each scope is
//! `caskade_app::permissions::script_scope`.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::model::Game;
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn scripts_are_saved_ordered_and_deleted_within_their_scope() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Scripted"))).unwrap();
    let app = mock_builder()
        .manage(store.clone())
        .invoke_handler(tauri::generate_handler![
            lifecycle_scripts,
            lifecycle_script_save,
            lifecycle_script_delete,
            lifecycle_scripts_order
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let scope = json!({"type": "game", "gameId": game.id});
    let save = |name: &str, scope: &serde_json::Value| {
        invoke(
            &window,
            "lifecycle_script_save",
            json!({"scope": scope, "script": {
                "id": "", "scope": scope, "name": name, "enabled": true,
                "phase": "before_launch",
                "command": {"type": "shell", "shell": "sh", "script": "echo \"$CASKADE_GAME_TITLE\""},
                "cwd": null, "timeoutSecs": 30,
            }}),
        )
    };
    let first = save("Mount", &scope).unwrap();
    let second = save("VPN", &scope).unwrap();
    assert!(!first["id"].as_str().unwrap().is_empty());
    // A script names the scope it is saved in.
    let other = json!({"type": "global"});
    let refused = invoke(
        &window,
        "lifecycle_script_save",
        json!({"scope": other, "script": first}),
    )
    .unwrap_err();
    assert_eq!(refused["code"], "invalid");
    invoke(
        &window,
        "lifecycle_scripts_order",
        json!({"scope": scope, "ids": [second["id"], first["id"]]}),
    )
    .unwrap();
    let listed = invoke(&window, "lifecycle_scripts", json!({"scope": scope})).unwrap();
    assert_eq!(listed[0]["name"], "VPN");
    assert_eq!(listed[1]["name"], "Mount");
    assert!(
        invoke(
            &window,
            "lifecycle_script_delete",
            json!({"scope": other, "id": first["id"]}),
        )
        .is_err()
    );
    assert_eq!(
        invoke(
            &window,
            "lifecycle_script_delete",
            json!({"scope": scope, "id": first["id"]}),
        )
        .unwrap(),
        true
    );
}

#[cfg(unix)]
#[test]
fn a_test_runs_with_the_game_s_context_shows_its_output_and_can_be_cancelled() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Tested"))).unwrap();
    let app = mock_builder()
        .manage(store.clone())
        .invoke_handler(tauri::generate_handler![
            lifecycle_script_save,
            lifecycle_scripts_effective,
            lifecycle_script_test,
            lifecycle_script_test_cancel
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let scope = json!({"type": "game", "gameId": game.id});
    let script = |name: &str, text: &str| {
        json!({
            "id": "", "scope": scope, "name": name, "enabled": true, "phase": "before_launch",
            "command": {"type": "shell", "shell": "sh", "script": text},
            "cwd": null, "timeoutSecs": 30,
        })
    };
    // Tested unsaved: the game's values, its output, and how it ended.
    let tested = invoke(
        &window,
        "lifecycle_script_test",
        json!({
            "scope": scope,
            "script": script("Echo", "printf '%s' \"$CASKADE_GAME_TITLE\"; echo oops >&2; exit 3"),
            "gameId": null,
            "testId": "one",
        }),
    )
    .unwrap();
    assert_eq!(tested["run"]["status"], "failed");
    assert_eq!(tested["run"]["exitCode"], 3);
    assert_eq!(tested["run"]["stdout"], "Tested");
    assert_eq!(tested["run"]["stderr"], "oops\n");
    assert!(
        tested["context"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["variable"] == "CASKADE_TEST" && value["value"] == "1")
    );
    // A slow one is cancelled from another call.
    let slow = std::thread::spawn({
        let window = window.clone();
        let scope = scope.clone();
        let script = script("Slow", "sleep 30");
        move || {
            invoke(
                &window,
                "lifecycle_script_test",
                json!({"scope": scope, "script": script, "gameId": null, "testId": "slow"}),
            )
        }
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while invoke(
        &window,
        "lifecycle_script_test_cancel",
        json!({"testId": "slow"}),
    )
    .unwrap()
        != json!(true)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the test never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(slow.join().unwrap().unwrap()["run"]["status"], "cancelled");
    // The order a launch would take, including a disabled step.
    let mut off = script("Off", "true");
    off["enabled"] = json!(false);
    invoke(
        &window,
        "lifecycle_script_save",
        json!({"scope": scope, "script": off}),
    )
    .unwrap();
    let steps = invoke(
        &window,
        "lifecycle_scripts_effective",
        json!({"gameId": game.id, "profileId": null}),
    )
    .unwrap();
    assert_eq!(steps[0]["script"]["name"], "Off");
    assert_eq!(steps[0]["script"]["enabled"], false);
}
