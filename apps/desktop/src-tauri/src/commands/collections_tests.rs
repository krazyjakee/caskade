//! Typed IPC -> LibraryStore collections, on Tauri's mock runtime.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::model::Game;
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn collections_are_made_filled_and_listed_over_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = Game::new("Shelved");
    tauri::async_runtime::block_on(store.upsert_game(&game)).unwrap();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![
            list_collections,
            get_collection,
            collection_games,
            save_collection,
            set_collection_pinned,
            delete_collection,
            add_to_collection,
            remove_from_collection,
            game_collections
        ],
    );
    let accounts = tauri::async_runtime::block_on(AccountService::open(
        store.clone(),
        caskade_core::bus::Bus::default(),
    ))
    .unwrap();
    let app = mock_builder()
        .manage(store)
        .manage(accounts)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let made = invoke(
        &window,
        "save_collection",
        json!({"collection": {"id": null, "name": "Couch", "description": "Co-op",
            "kind": {"type": "manual"}}}),
    )
    .unwrap();
    let id = made["id"].as_str().unwrap().to_owned();
    invoke(
        &window,
        "add_to_collection",
        json!({"id": id, "gameIds": [game.id]}),
    )
    .unwrap();
    assert_eq!(
        invoke(&window, "game_collections", json!({"gameId": game.id})).unwrap(),
        json!([id])
    );
    let stored = invoke(&window, "get_collection", json!({"id": id})).unwrap();
    assert_eq!(stored["gameIds"], json!([game.id]));
    assert_eq!(stored["description"], "Co-op");
    let page = invoke(
        &window,
        "collection_games",
        json!({"id": id, "offset": 0, "limit": 10}),
    )
    .unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["title"], "Shelved");

    let smart = invoke(
        &window,
        "save_collection",
        json!({"collection": {"id": null, "name": "Favourites", "description": "",
            "kind": {"type": "smart", "query": {"filter": "favorites"}}}}),
    )
    .unwrap();
    invoke(
        &window,
        "set_collection_pinned",
        json!({"id": smart["id"], "pinned": true}),
    )
    .unwrap();
    let list = invoke(&window, "list_collections", json!({})).unwrap();
    assert_eq!(list[0]["name"], "Favourites");
    assert_eq!(list[0]["games"], 0);
    assert_eq!(list[1]["games"], 1);
    assert!(
        invoke(
            &window,
            "add_to_collection",
            json!({"id": smart["id"], "gameIds": [game.id]})
        )
        .is_err(),
        "a smart collection chooses its own games"
    );

    invoke(
        &window,
        "remove_from_collection",
        json!({"id": id, "gameIds": [game.id]}),
    )
    .unwrap();
    assert_eq!(
        invoke(&window, "delete_collection", json!({"id": id})).unwrap(),
        json!(true)
    );
    let list = invoke(&window, "list_collections", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
}
