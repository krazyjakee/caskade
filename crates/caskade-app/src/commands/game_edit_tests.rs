//! Game editing: edits, artwork, bulk edits and refreshes of chosen games,
//! through the commands. Edits are built from the JSON the editor sends and
//! replies read as JSON, to keep the assertions on their fields.
use super::*;
use caskade_core::{bus::Bus, model::Game};
use serde_json::{Value, json};

fn to_json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn edits_save_reject_and_report() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Portal 2")).await.unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();

    let state = game_edit_state(&store, game.id.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(state.manual.is_empty());
    let saved = game_edit(
        &store,
        &bus,
        game.id.clone(),
        from_json(json!({"title": {"op": "set", "value": "Portal Two"}, "rating": {"value": 80}})),
        Some(state.game.clone()),
    )
    .await
    .unwrap();
    let saved = to_json(saved);
    assert_eq!(saved["status"], "saved");
    assert_eq!(saved["changed"], json!(["title", "rating"]));
    assert_eq!(saved["state"]["game"]["title"], "Portal Two");
    assert_eq!(saved["state"]["automatic"]["title"], "Portal 2");
    assert_eq!(saved["state"]["manual"], json!(["title"]));
    assert!(matches!(
        events.try_recv(),
        Ok(Event::LibraryGameUpdated { game_id }) if game_id == game.id
    ));

    let rejected = game_edit(
        &store,
        &bus,
        game.id.clone(),
        from_json(json!({"releaseDate": {"op": "set", "value": "1999-13-01"}})),
        None,
    )
    .await
    .unwrap();
    let rejected = to_json(rejected);
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["errors"][0]["field"], "releaseDate");
    assert_eq!(rejected["errors"][0]["problem"], "invalid");
    assert!(events.try_recv().is_err());
    // An unknown key is no edit; nothing is saved.
    assert!(serde_json::from_value::<GameEdit>(json!({"completion": "done"})).is_err());
    let missing = game_edit_state(&store, "nope".into()).await.unwrap();
    assert!(missing.is_none());
    store.close().await;
}

/// A 1×1 PNG.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[tokio::test]
async fn artwork_is_imported_removed_and_reset() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Portal 2")).await.unwrap();
    let bus = Bus::default();
    let cache = EditArtwork(Some(
        ArtworkCache::new(temp.path().join("artwork")).unwrap(),
    ));
    let slots = to_json(game_artwork(&store, game.id.clone()).await.unwrap());
    assert_eq!(slots[0]["kind"], "cover");
    assert_eq!(slots[0]["choice"], "automatic");

    let image = temp.path().join("cover.png");
    std::fs::write(&image, PIXEL).unwrap();
    let set = |change: ArtworkChange| {
        game_artwork_set(
            &store,
            &bus,
            &cache,
            game.id.clone(),
            ArtworkKind::Cover,
            change,
        )
    };
    let slots = to_json(set(ArtworkChange::Import { path: image }).await.unwrap());
    assert_eq!(slots[0]["choice"], "chosen");
    assert_eq!(slots[0]["shown"]["source"], "user");
    let broken = temp.path().join("broken.png");
    std::fs::write(&broken, b"nope").unwrap();
    assert!(set(ArtworkChange::Import { path: broken }).await.is_err());
    let slots = to_json(game_artwork(&store, game.id.clone()).await.unwrap());
    assert_eq!(slots[0]["choice"], "chosen");
    assert_eq!(
        to_json(set(ArtworkChange::Remove).await.unwrap())[0]["choice"],
        "removed"
    );
    let slots = to_json(set(ArtworkChange::Reset).await.unwrap());
    assert_eq!(slots[0]["choice"], "automatic");
    assert!(slots[0]["shown"].is_null());
    store.close().await;
}

#[tokio::test]
async fn bulk_edits_preview_apply_and_list_ids() {
    let store = LibraryStore::in_memory().await.unwrap();
    let one = store.upsert_game(&Game::new("One")).await.unwrap();
    let two = store.upsert_game(&Game::new("Two")).await.unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let ids = library_game_ids(&store, &accounts, ListGames::default())
        .await
        .unwrap();
    assert_eq!(ids, [one.id.clone(), two.id.clone()]);
    let edit = || -> GameEdit { from_json(json!({"tags": {"op": "add", "values": ["Co-op"]}})) };
    let preview = to_json(
        games_bulk_edit(&store, &bus, ids.clone(), edit(), false)
            .await
            .unwrap(),
    );
    assert_eq!(preview[0]["status"], "changed");
    assert_eq!(preview[0]["changes"][0]["after"], "Co-op");
    assert!(events.try_recv().is_err());
    games_bulk_edit(&store, &bus, ids.clone(), edit(), true)
        .await
        .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            events.try_recv(),
            Ok(Event::LibraryGameUpdated { .. })
        ));
    }
    let again = games_bulk_edit(&store, &bus, ids, edit(), true)
        .await
        .unwrap();
    assert_eq!(again[1].status, BulkStatus::Unchanged);
    store.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn chosen_games_refresh_in_the_background() {
    use caskade_core::{emulation::Catalog, hasheous::Hasheous, igdb::Igdb};
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Cartridge")).await.unwrap();
    let metadata = Metadata::new(
        store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.path().join("igdb"), "http://127.0.0.1:9"),
        Hasheous::with_base_url(dir.path().join("hasheous"), "http://127.0.0.1:9"),
        std::sync::Arc::new(Catalog::default()),
        None,
    );
    assert!(games_refresh_status(&metadata).is_none());
    games_refresh_start(&metadata, vec![game.id.clone()], RefreshMode::Missing)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        let status = games_refresh_status(&metadata);
        if let Some(status) = status.as_ref().filter(|status| !status.running) {
            break status.clone();
        }
        assert!(std::time::Instant::now() < deadline, "{status:?}");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    assert_eq!(status.total, 1);
    assert_eq!(status.done, 1);
    assert_eq!(status.mode, RefreshMode::Missing);
    // A title-only game has nothing to ask about: it stays unmatched.
    assert_eq!(status.unmatched, 1);
    store.close().await;
}
