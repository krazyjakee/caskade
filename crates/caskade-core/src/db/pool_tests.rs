//! The library's connections under load (#77): a synced library's counts
//! don't hold them while everything else waits.

use super::LibraryStore;
use crate::model::{Game, GameEntry, GameFilter, ListGames};
use std::time::{Duration, Instant};

const DEVICE: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

/// Another device's copy of `game` here: its peer entry and the copy it
/// announced, keyed under `keyed_game` (the game it was when it arrived).
async fn peer_copy(store: &LibraryStore, game: &str, keyed_game: &str, entry: &str, copy: &str) {
    let peer = GameEntry::new(game, format!("peer:{DEVICE}").into(), entry);
    store.upsert_entry(&peer).await.unwrap();
    sqlx::query(
        "INSERT INTO sync_fields(entity,key,field,value_json,origin,wall,counter) \
         VALUES('copy',?,'',?,?,1,0)",
    )
    .bind(format!("{DEVICE}|{keyed_game}|{entry}"))
    .bind(copy)
    .bind(DEVICE)
    .execute(&store.pool)
    .await
    .unwrap();
}

const COPYABLE: &str = r#"{"os":"windows","copyable":true,"location":"managed"}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ready_to_install_counts_leave_connections_for_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    // A file, so the pool has its four connections, as in the app.
    let store = LibraryStore::open(&dir.path().join("library.db"))
        .await
        .unwrap();
    // A synced library where another device has 2,500 games installed.
    let games: Vec<Game> = (0..2500).map(|i| Game::new(format!("Game {i}"))).collect();
    store.upsert_games(&games).await.unwrap();
    for (i, game) in games.iter().enumerate() {
        peer_copy(&store, &game.id, &game.id, &format!("entry-{i}"), COPYABLE).await;
    }

    // Library asks for its counts again and again while games change; each
    // used to compare every peer entry with every copy and take seconds.
    let started = Instant::now();
    let counts: Vec<_> = (0..4)
        .map(|_| {
            let store = store.clone();
            tokio::spawn(async move { store.library_stats().await.map(|stats| stats.owned) })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let asked = Instant::now();
    store.get_setting("core.sync").await.unwrap();
    let waited = asked.elapsed();
    for count in counts {
        assert_eq!(count.await.unwrap().unwrap(), 2500);
    }
    let page = store
        .list_games(&ListGames {
            filter: GameFilter::Owned,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 2500);
    assert!(
        waited < Duration::from_secs(2),
        "a setting waited {waited:?} for a connection"
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the counts took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn ready_to_install_counts_only_copies_this_system_can_take() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut games = Vec::new();
    for title in ["Copyable", "Launcher", "Mac only", "Merged", "Gone", "Mine"] {
        games.push(store.upsert_game(&Game::new(title)).await.unwrap());
    }
    let id = |i: usize| games[i].id.clone();
    peer_copy(&store, &id(0), &id(0), "a", COPYABLE).await;
    let launcher = r#"{"os":"windows","copyable":false,"location":"launcher"}"#;
    peer_copy(&store, &id(1), &id(1), "b", launcher).await;
    let mac = r#"{"os":"mac","copyable":true,"location":"managed"}"#;
    peer_copy(&store, &id(2), &id(2), "c", mac).await;
    // Keyed by a game that was merged into this one since.
    peer_copy(&store, &id(3), "merged-away-game", "d", COPYABLE).await;
    // A copy announced for an entry that is no longer here.
    sqlx::query(
        "INSERT INTO sync_fields(entity,key,field,value_json,origin,wall,counter) \
         VALUES('copy',?,'',?,?,1,0)",
    )
    .bind(format!("{DEVICE}|{}|e", id(4)))
    .bind(COPYABLE)
    .bind(DEVICE)
    .execute(&store.pool)
    .await
    .unwrap();
    // This device's own copy, announced, isn't a peer entry.
    let mine = GameEntry::new(id(5), "manual".to_string().into(), "f");
    store.upsert_entry(&mine).await.unwrap();

    let owned = store
        .list_games(&ListGames {
            filter: GameFilter::Owned,
            ..Default::default()
        })
        .await
        .unwrap();
    let mut titles: Vec<String> = owned.items.into_iter().map(|game| game.title).collect();
    titles.sort();
    let expected: Vec<String> = if cfg!(target_os = "macos") {
        vec!["Mac only".into()]
    } else {
        vec!["Copyable".into(), "Merged".into()]
    };
    assert_eq!(titles, expected);
    assert_eq!(
        store.library_stats().await.unwrap().owned,
        expected.len() as u64
    );
}
