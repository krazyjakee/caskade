//! A write that reads first waits for another connection's write instead of
//! failing with "database is locked" (found in #71: RomM marking 10,000 ROMs
//! unavailable failed at startup while other startup work wrote).
use super::LibraryStore;
use crate::model::{Game, GameEntry, SourceId};
use sqlx::{Connection, sqlite::SqliteConnection};
use std::time::Duration;

#[tokio::test]
async fn a_write_that_reads_first_waits_for_another_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.sqlite");
    let store = LibraryStore::open(&path).await.unwrap();
    let game = Game::new("Doom");
    let entry = GameEntry::new(&game.id, SourceId::new("steam"), "2280");
    let id = store
        .import_group(&game, &[entry], None)
        .await
        .unwrap()
        .game_id;

    // Another connection (an import, a sync) holds the write lock a moment.
    let mut other = SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut other)
        .await
        .unwrap();
    sqlx::query("UPDATE games SET title=title")
        .execute(&mut other)
        .await
        .unwrap();
    let favorite = tokio::spawn({
        let store = store.clone();
        let id = id.clone();
        async move { store.set_game_favorite(&id, true).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!favorite.is_finished(), "{:?}", favorite.await);
    sqlx::query("COMMIT").execute(&mut other).await.unwrap();

    favorite.await.unwrap().unwrap();
    assert!(store.game(&id).await.unwrap().unwrap().favorite);
}
