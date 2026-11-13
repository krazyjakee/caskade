//! The IGDB commands against a server that never answers: nothing here may
//! need the network.
use caskade_app::commands::igdb as commands;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    emulation::Catalog,
    hasheous::Hasheous,
    igdb::Igdb,
    metadata::{GameIgdb, MatchSource, Metadata},
    model::{Game, GameEntry, SourceId},
};

#[tokio::test]
async fn igdb_commands_take_string_ids() {
    let dir = tempfile::tempdir().unwrap();
    // Port 9 (discard) on loopback: any request fails.
    let igdb = Igdb::with_base_url(dir.path(), "http://127.0.0.1:9");

    // IGDB has no ids above i64::MAX, so these are answered without asking.
    let beyond = 1u64 << 63;
    let ids = [beyond.to_string(), beyond.to_string()];
    for lookup in [
        commands::igdb_games(&igdb, &ids).await.unwrap(),
        commands::igdb_refresh(&igdb, &ids).await.unwrap(),
    ] {
        assert!(lookup.games.is_empty());
        assert_eq!(lookup.not_found, [beyond]);
        assert!(lookup.pending.is_empty());
    }
    let refused = commands::igdb_games(&igdb, &["1942".to_owned(), "witcher".to_owned()])
        .await
        .unwrap_err();
    assert!(refused.contains("not an IGDB id"), "{refused}");
    // A real id needs the server, which is not there.
    assert!(
        commands::igdb_games(&igdb, &["1942".to_owned()])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn game_igdb_commands_answer_without_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Portal 2")).await.unwrap();
    let mut entries = Vec::new();
    for source in ["steam", "gog"] {
        let entry = GameEntry::new(&game.id, SourceId::new(source), "portal-2");
        entries.push(store.upsert_entry(&entry).await.unwrap());
    }
    let (steam, gog) = (entries[0].clone(), entries[1].clone());
    let metadata = Metadata::new(
        store.clone(),
        Bus::default(),
        Igdb::with_base_url(dir.path().join("igdb"), "http://127.0.0.1:9"),
        Hasheous::with_base_url(dir.path().join("hasheous"), "http://127.0.0.1:9"),
        std::sync::Arc::new(Catalog::default()),
        None,
    );

    // Not looked at yet.
    assert_eq!(
        commands::game_igdb(&metadata, &game.id).await.unwrap(),
        None
    );
    assert_eq!(
        commands::game_igdb_refresh(&metadata, &game.id)
            .await
            .unwrap(),
        None
    );
    let refused = commands::game_igdb_set(&metadata, &game.id, Some("portal".into()))
        .await
        .unwrap_err();
    assert!(refused.contains("not an IGDB id"), "{refused}");
    let missing = commands::game_igdb_set(&metadata, "nope", None)
        .await
        .unwrap_err();
    assert!(missing.contains("no longer in the library"), "{missing}");

    // "No IGDB game" needs no server.
    let none = GameIgdb {
        game_id: game.id.clone(),
        igdb_id: None,
        source: MatchSource::Manual,
        pending: false,
        game: None,
    };
    assert_eq!(
        commands::game_igdb_set(&metadata, &game.id, None)
            .await
            .unwrap(),
        none
    );
    assert_eq!(
        commands::game_igdb(&metadata, &game.id).await.unwrap(),
        Some(none)
    );

    // An entry moves out into a game of its own; the last one can't.
    let apart = commands::game_separate_entry(&metadata, &gog.id)
        .await
        .unwrap();
    let ids = |entries: Vec<GameEntry>| {
        entries
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(store.entries_for_game(&apart).await.unwrap()),
        std::slice::from_ref(&gog.id)
    );
    assert_eq!(
        ids(store.entries_for_game(&game.id).await.unwrap()),
        [steam.id]
    );
    assert!(
        commands::game_separate_entry(&metadata, &gog.id)
            .await
            .is_err()
    );
}
