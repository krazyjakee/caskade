use super::*;
use crate::commands::test_ipc::invoke;
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn a_game_is_added_by_hand_through_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("game.sh");
    std::fs::write(&program, b"#!/bin/sh\n").unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let app = mock_builder()
        .manage(store.clone())
        .manage(bus)
        .invoke_handler(tauri::generate_handler![
            manual_game_target,
            manual_game_duplicates,
            manual_game_add
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    assert!(invoke(&window, "manual_game_target", json!({"input": temp.path()})).is_err());
    let target = invoke(&window, "manual_game_target", json!({"input": program})).unwrap();
    assert_eq!(target["title"], "game");
    assert_eq!(target["launch"]["type"], "executable");
    let none = invoke(
        &window,
        "manual_game_duplicates",
        json!({"title": "game", "target": target}),
    )
    .unwrap();
    assert_eq!(none, json!([]));
    let id = invoke(
        &window,
        "manual_game_add",
        json!({"game": {"title": "My Game", "target": target, "linkTo": null, "keepApart": false}}),
    )
    .unwrap();
    assert!(matches!(
        events.try_recv(),
        Ok(Event::LibraryGameAdded { game_id }) if json!(game_id) == id
    ));
    let found = invoke(
        &window,
        "manual_game_duplicates",
        json!({"title": "my game", "target": null}),
    )
    .unwrap();
    assert_eq!(found[0]["gameId"], id);
    tauri::async_runtime::block_on(store.close());
}
