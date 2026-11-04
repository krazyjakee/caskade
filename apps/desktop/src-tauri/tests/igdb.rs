//! IGDB commands on the mock runtime, against a server that never answers:
//! nothing here may need the network.
use caskade_core::igdb::Igdb;
use serde_json::{Value, json};
use tauri::test::{mock_builder, mock_context, noop_assets};

use crate::ipc::invoke;

#[test]
fn igdb_commands_take_string_ids() {
    let dir = tempfile::tempdir().unwrap();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![
            caskade_desktop::commands::igdb::igdb_games,
            caskade_desktop::commands::igdb::igdb_refresh,
        ],
    );
    let app = mock_builder()
        // Port 9 (discard) on loopback: any request fails.
        .manage(Igdb::with_base_url(dir.path(), "http://127.0.0.1:9"))
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let view = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    // IGDB has no ids above i64::MAX, so these are answered without asking.
    let beyond = (1u64 << 63).to_string();
    for command in ["igdb_games", "igdb_refresh"] {
        let lookup = invoke(&view, command, json!({"ids": [beyond, beyond]})).unwrap();
        assert_eq!(
            lookup,
            json!({"games": [], "notFound": [beyond], "pending": []}),
            "{command}"
        );
    }
    let refused = invoke(&view, "igdb_games", json!({"ids": ["1942", "witcher"]})).unwrap_err();
    assert!(
        refused.as_str().unwrap().contains("not an IGDB id"),
        "{refused}"
    );
    // A real id needs the server, which is not there.
    assert!(invoke(&view, "igdb_games", json!({"ids": ["1942"]})).is_err());
}

#[test]
fn game_igdb_commands_answer_without_the_network() {
    use caskade_core::{
        bus::Bus,
        db::LibraryStore,
        emulation::Catalog,
        hasheous::Hasheous,
        metadata::Metadata,
        model::{Game, GameEntry},
    };
    use caskade_discovery::SourceId;
    let dir = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Portal 2"))).unwrap();
    let [steam, gog] = ["steam", "gog"].map(|source| {
        let entry = GameEntry::new(&game.id, SourceId::new(source), "portal-2");
        tauri::async_runtime::block_on(store.upsert_entry(&entry)).unwrap()
    });
    let metadata = Metadata::new(
        store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.path().join("igdb"), "http://127.0.0.1:9"),
        Hasheous::with_base_url(dir.path().join("hasheous"), "http://127.0.0.1:9"),
        std::sync::Arc::new(Catalog::default()),
        None,
    );
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![
            caskade_desktop::commands::igdb::game_igdb,
            caskade_desktop::commands::igdb::game_igdb_set,
            caskade_desktop::commands::igdb::game_igdb_refresh,
            caskade_desktop::commands::igdb::game_separate_entry,
        ],
    );
    let app = mock_builder()
        .manage(metadata)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let view = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    // Not looked at yet.
    let id = json!({"gameId": game.id});
    assert_eq!(invoke(&view, "game_igdb", id.clone()).unwrap(), Value::Null);
    assert_eq!(
        invoke(&view, "game_igdb_refresh", id.clone()).unwrap(),
        Value::Null
    );
    let refused = invoke(
        &view,
        "game_igdb_set",
        json!({"gameId": game.id, "igdbId": "portal"}),
    )
    .unwrap_err();
    assert!(
        refused.as_str().unwrap().contains("not an IGDB id"),
        "{refused}"
    );
    let missing = invoke(
        &view,
        "game_igdb_set",
        json!({"gameId": "nope", "igdbId": null}),
    )
    .unwrap_err();
    assert!(
        missing
            .as_str()
            .unwrap()
            .contains("no longer in the library"),
        "{missing}"
    );

    // "No IGDB game" needs no server.
    let none = json!({
        "gameId": game.id, "igdbId": null, "source": "manual", "pending": false, "game": null
    });
    assert_eq!(
        invoke(
            &view,
            "game_igdb_set",
            json!({"gameId": game.id, "igdbId": null})
        )
        .unwrap(),
        none
    );
    assert_eq!(invoke(&view, "game_igdb", id).unwrap(), none);

    // An entry moves out into a game of its own; the last one can't.
    let apart = invoke(&view, "game_separate_entry", json!({"entryId": gog.id})).unwrap();
    let apart = apart.as_str().unwrap();
    let entries = |game_id: &str| {
        tauri::async_runtime::block_on(store.entries_for_game(game_id))
            .unwrap()
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(entries(apart), std::slice::from_ref(&gog.id));
    assert_eq!(entries(&game.id), [steam.id]);
    assert!(invoke(&view, "game_separate_entry", json!({"entryId": gog.id})).is_err());
}
