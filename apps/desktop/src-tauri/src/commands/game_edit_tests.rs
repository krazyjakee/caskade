use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
    model::Game,
};
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn edits_save_reject_and_report_through_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Portal 2"))).unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let app = mock_builder()
        .manage(store.clone())
        .manage(bus)
        .invoke_handler(tauri::generate_handler![game_edit_state, game_edit])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let state = invoke(&window, "game_edit_state", json!({"gameId": game.id})).unwrap();
    assert_eq!(state["manual"], json!([]));
    let saved = invoke(
        &window,
        "game_edit",
        json!({
            "gameId": game.id,
            "edit": {"title": {"op": "set", "value": "Portal Two"}, "rating": {"value": 80}},
            "base": state["game"],
        }),
    )
    .unwrap();
    assert_eq!(saved["status"], "saved");
    assert_eq!(saved["changed"], json!(["title", "rating"]));
    assert_eq!(saved["state"]["game"]["title"], "Portal Two");
    assert_eq!(saved["state"]["automatic"]["title"], "Portal 2");
    assert_eq!(saved["state"]["manual"], json!(["title"]));
    assert!(matches!(
        events.try_recv(),
        Ok(Event::LibraryGameUpdated { game_id }) if game_id == game.id
    ));

    let rejected = invoke(
        &window,
        "game_edit",
        json!({"gameId": game.id, "edit": {"releaseDate": {"op": "set", "value": "1999-13-01"}}, "base": null}),
    )
    .unwrap();
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["errors"][0]["field"], "releaseDate");
    assert_eq!(rejected["errors"][0]["problem"], "invalid");
    assert!(events.try_recv().is_err());
    // An unknown key fails the call; nothing is saved.
    assert!(
        invoke(
            &window,
            "game_edit",
            json!({"gameId": game.id, "edit": {"completion": "done"}, "base": null}),
        )
        .is_err()
    );
    let missing = invoke(&window, "game_edit_state", json!({"gameId": "nope"})).unwrap();
    assert!(missing.is_null());
    tauri::async_runtime::block_on(store.close());
}

/// A 1×1 PNG.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[test]
fn artwork_is_imported_removed_and_reset_through_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Portal 2"))).unwrap();
    let cache = caskade_core::artwork::ArtworkCache::new(temp.path().join("artwork")).unwrap();
    let app = mock_builder()
        .manage(store.clone())
        .manage(Bus::default())
        .manage(EditArtwork(Some(cache)))
        .invoke_handler(tauri::generate_handler![game_artwork, game_artwork_set])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let slots = invoke(&window, "game_artwork", json!({"gameId": game.id})).unwrap();
    assert_eq!(slots[0]["kind"], "cover");
    assert_eq!(slots[0]["choice"], "automatic");

    let image = temp.path().join("cover.png");
    std::fs::write(&image, PIXEL).unwrap();
    let set = |change: serde_json::Value| {
        invoke(
            &window,
            "game_artwork_set",
            json!({"gameId": game.id, "kind": "cover", "change": change}),
        )
    };
    let slots = set(json!({"op": "import", "path": image})).unwrap();
    assert_eq!(slots[0]["choice"], "chosen");
    assert_eq!(slots[0]["shown"]["source"], "user");
    let broken = temp.path().join("broken.png");
    std::fs::write(&broken, b"nope").unwrap();
    assert!(set(json!({"op": "import", "path": broken})).is_err());
    let slots = invoke(&window, "game_artwork", json!({"gameId": game.id})).unwrap();
    assert_eq!(slots[0]["choice"], "chosen");
    assert_eq!(
        set(json!({"op": "remove"})).unwrap()[0]["choice"],
        "removed"
    );
    let slots = set(json!({"op": "reset"})).unwrap();
    assert_eq!(slots[0]["choice"], "automatic");
    assert!(slots[0]["shown"].is_null());
    tauri::async_runtime::block_on(store.close());
}

#[test]
fn bulk_edits_preview_apply_and_list_ids_through_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let one = tauri::async_runtime::block_on(store.upsert_game(&Game::new("One"))).unwrap();
    let two = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Two"))).unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let app = mock_builder()
        .manage(store.clone())
        .manage(bus)
        .manage(
            tauri::async_runtime::block_on(caskade_core::accounts::AccountService::open(
                store.clone(),
                Bus::default(),
            ))
            .unwrap(),
        )
        .invoke_handler(tauri::generate_handler![games_bulk_edit, library_game_ids])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let ids = invoke(&window, "library_game_ids", json!({"query": {}})).unwrap();
    assert_eq!(ids, json!([one.id, two.id]));
    let body = |apply: bool| json!({"gameIds": ids, "edit": {"tags": {"op": "add", "values": ["Co-op"]}}, "apply": apply});
    let preview = invoke(&window, "games_bulk_edit", body(false)).unwrap();
    assert_eq!(preview[0]["status"], "changed");
    assert_eq!(preview[0]["changes"][0]["after"], "Co-op");
    assert!(events.try_recv().is_err());
    invoke(&window, "games_bulk_edit", body(true)).unwrap();
    for _ in 0..2 {
        assert!(matches!(
            events.try_recv(),
            Ok(Event::LibraryGameUpdated { .. })
        ));
    }
    let again = invoke(&window, "games_bulk_edit", body(true)).unwrap();
    assert_eq!(again[1]["status"], "unchanged");
    tauri::async_runtime::block_on(store.close());
}

#[test]
fn chosen_games_refresh_in_the_background_through_ipc() {
    use caskade_core::{emulation::Catalog, hasheous::Hasheous, igdb::Igdb};
    let dir = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Cartridge"))).unwrap();
    let metadata = Metadata::new(
        store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.path().join("igdb"), "http://127.0.0.1:9"),
        Hasheous::with_base_url(dir.path().join("hasheous"), "http://127.0.0.1:9"),
        std::sync::Arc::new(Catalog::default()),
        None,
    );
    let app = mock_builder()
        .manage(metadata)
        .invoke_handler(tauri::generate_handler![
            games_refresh_start,
            games_refresh_cancel,
            games_refresh_status
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    assert!(
        invoke(&window, "games_refresh_status", json!({}))
            .unwrap()
            .is_null()
    );
    invoke(
        &window,
        "games_refresh_start",
        json!({"gameIds": [game.id], "mode": "missing"}),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        let status = invoke(&window, "games_refresh_status", json!({})).unwrap();
        if status["running"] == false {
            break status;
        }
        assert!(std::time::Instant::now() < deadline, "{status}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(status["total"], 1);
    assert_eq!(status["done"], 1);
    assert_eq!(status["mode"], "missing");
    // A title-only game has nothing to ask about: it stays unmatched.
    assert_eq!(status["unmatched"], 1);
    tauri::async_runtime::block_on(store.close());
}
