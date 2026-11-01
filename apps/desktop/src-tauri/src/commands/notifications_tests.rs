//! Typed IPC -> the notification feed, on Tauri's mock runtime.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    db::LibraryStore,
    model::{Game, GameEntry, SourceId},
};
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn the_feed_is_listed_paged_and_marked_over_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    for (title, source) in [("Celeste", "steam"), ("Metroid", "romm:home")] {
        let game = Game::new(title);
        let entry = GameEntry::new(&game.id, SourceId::new(source), title);
        tauri::async_runtime::block_on(store.import_group(&game, &[entry], None)).unwrap();
    }
    let notifications = Notifications::new(store.clone());
    assert!(tauri::async_runtime::block_on(notifications.ingest_additions()).unwrap());
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![
            notifications_list,
            notifications_unread,
            notification_members,
            notification_mark_read,
            notification_mark_unread,
            notifications_mark_all_read
        ],
    );
    let app = mock_builder()
        .manage(notifications)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let page = invoke(
        &window,
        "notifications_list",
        json!({"unreadOnly": false, "offset": 0, "limit": 20}),
    )
    .unwrap();
    assert_eq!(
        (page["total"].clone(), page["unread"].clone()),
        (json!(1), json!(1))
    );
    let item = &page["items"][0];
    assert_eq!(item["category"], "gamesAdded");
    assert_eq!(item["title"], "2 games added");
    assert_eq!(item["target"], json!({"type": "members"}));
    assert_eq!(item["unread"], true);
    let id = item["id"].as_str().unwrap().to_owned();

    let members = invoke(
        &window,
        "notification_members",
        json!({"id": id, "offset": 0, "limit": 1}),
    )
    .unwrap();
    assert_eq!(members["total"], 2);
    assert_eq!(members["items"].as_array().unwrap().len(), 1);
    assert_eq!(members["items"][0]["state"], "present");

    assert_eq!(
        invoke(
            &window,
            "notification_mark_read",
            json!({"id": id, "version": item["version"]}),
        )
        .unwrap(),
        json!(true)
    );
    assert_eq!(
        invoke(&window, "notifications_unread", json!({})).unwrap(),
        json!(0)
    );
    let unread = invoke(
        &window,
        "notifications_list",
        json!({"unreadOnly": true, "offset": 0, "limit": 20}),
    )
    .unwrap();
    assert_eq!(unread["total"], 0);

    invoke(&window, "notification_mark_unread", json!({"id": id})).unwrap();
    assert_eq!(
        invoke(&window, "notifications_unread", json!({})).unwrap(),
        json!(1)
    );
    assert_eq!(
        invoke(
            &window,
            "notifications_mark_all_read",
            json!({"revision": page["revision"]}),
        )
        .unwrap(),
        json!(1)
    );
    assert_eq!(
        invoke(&window, "notifications_unread", json!({})).unwrap(),
        json!(0)
    );
}
