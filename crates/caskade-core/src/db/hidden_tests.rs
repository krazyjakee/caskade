//! Hidden games: out of every library view, kept through re-imports and
//! merges, and listed for Settings.
use super::LibraryStore;
use crate::model::{Game, GameEntry, GameFilter, ListGames, SourceId};

async fn titles(store: &LibraryStore, query: Option<&str>) -> Vec<String> {
    let page = store
        .list_games(&ListGames {
            query: query.map(String::from),
            filter: GameFilter::All,
            ..Default::default()
        })
        .await
        .unwrap();
    page.items.into_iter().map(|game| game.title).collect()
}

async fn hidden_titles(store: &LibraryStore) -> Vec<String> {
    let page = store.hidden_games(0, 100).await.unwrap();
    assert_eq!(page.total as usize, page.items.len());
    page.items.into_iter().map(|game| game.title).collect()
}

/// Imports `title` from `source`/`key`, as discovery would.
async fn import(store: &LibraryStore, title: &str, source: &'static str, key: &str) -> String {
    let game = Game::new(title);
    let entry = GameEntry::new(&game.id, SourceId::new(source), key);
    store
        .import_group(&game, &[entry], None)
        .await
        .unwrap()
        .game_id
}

#[tokio::test]
async fn a_hidden_game_leaves_the_library_and_comes_back_when_unhidden() {
    let store = LibraryStore::in_memory().await.unwrap();
    let decomp = import(&store, "Decomp Buddy", "desktop", "decomp-buddy.desktop").await;
    import(&store, "Doom", "steam", "2280").await;

    let game = store.set_game_hidden(&decomp, true).await.unwrap();
    assert!(game.hidden);
    assert_eq!(titles(&store, None).await, ["Doom"]);
    assert!(titles(&store, Some("decomp")).await.is_empty());
    assert_eq!(store.library_stats().await.unwrap().total, 1);
    assert_eq!(hidden_titles(&store).await, ["Decomp Buddy"]);

    store.set_game_hidden(&decomp, false).await.unwrap();
    assert_eq!(titles(&store, None).await, ["Decomp Buddy", "Doom"]);
    assert!(hidden_titles(&store).await.is_empty());
}

#[tokio::test]
async fn a_rescan_does_not_bring_a_hidden_game_back() {
    let store = LibraryStore::in_memory().await.unwrap();
    let decomp = import(&store, "Decomp Buddy", "desktop", "decomp-buddy.desktop").await;
    store.set_game_hidden(&decomp, true).await.unwrap();

    let again = import(&store, "Decomp Buddy", "desktop", "decomp-buddy.desktop").await;
    assert_eq!(again, decomp);
    assert!(titles(&store, None).await.is_empty());
    assert_eq!(hidden_titles(&store).await, ["Decomp Buddy"]);
}

#[tokio::test]
async fn merging_a_hidden_game_with_a_duplicate_keeps_it_hidden() {
    let store = LibraryStore::in_memory().await.unwrap();
    let hidden = import(&store, "Decomp Buddy", "desktop", "decomp-buddy.desktop").await;
    let visible = import(&store, "Decomp Buddy", "flatpak", "io.decomp.Buddy").await;
    store.set_game_hidden(&hidden, true).await.unwrap();

    // A later scan finds both identities belong to one game.
    let game = Game::new("Decomp Buddy");
    let entries = [
        GameEntry::new(&game.id, SourceId::new("flatpak"), "io.decomp.Buddy"),
        GameEntry::new(&game.id, SourceId::new("desktop"), "decomp-buddy.desktop"),
    ];
    let merged = store.import_group(&game, &entries, None).await.unwrap();
    assert_eq!(merged.game_id, visible);
    assert!(store.game(&visible).await.unwrap().unwrap().hidden);
    assert!(titles(&store, None).await.is_empty());

    // An explicit merge into a visible game keeps it hidden too.
    let keep = import(&store, "Quake", "steam", "2310").await;
    let donor = import(&store, "Quake", "gog", "1435828198").await;
    store.set_game_hidden(&donor, true).await.unwrap();
    assert!(store.merge_games(&keep, &donor).await.unwrap().hidden);
}

#[tokio::test]
async fn hiding_a_missing_game_fails() {
    let store = LibraryStore::in_memory().await.unwrap();
    assert!(store.set_game_hidden("missing", true).await.is_err());
}
