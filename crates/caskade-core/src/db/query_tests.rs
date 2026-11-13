//! Library queries (#105): search, filters, stats, sorting and paging.
use super::test_support::{admin, game_entry};
use super::*;
use crate::model::*;
use std::time::{Duration, Instant};

#[tokio::test]
async fn search_triggers_cover_title_sort_aliases_tags_updates_deletes_and_literal_queries() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut game = Game::new("Café Adventure");
    game.sort_title = "Sort synonym".into();
    game.aliases = vec!["Old nickname".into()];
    game.tags = vec!["cooperation".into()];
    store.upsert_game(&game).await.unwrap();
    for query in ["cafe", "adven", "synonym", "nickname", "cooper"] {
        let page = store
            .list_games(&ListGames {
                query: Some(query.into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.total, 1, "{query}");
    }
    game.title = "New title".into();
    game.aliases.clear();
    game.tags.clear();
    store.upsert_game(&game).await.unwrap();
    for query in ["adven", "nickname", "cooper"] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    query: Some(query.into()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            0,
            "{query}"
        );
    }
    sqlx::query(
        "UPDATE games SET title=?,record_json=json_set(record_json,'$.title',?) WHERE id=?",
    )
    .bind("Direct trigger")
    .bind("Direct trigger")
    .bind(&game.id)
    .execute(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        store
            .list_games(&ListGames {
                query: Some("trigger".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .total,
        1
    );
    for hostile in [
        "\" OR *",
        "\"; DROP TABLE games; --",
        "* NOT title:whatever",
    ] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    query: Some(hostile.into()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            0
        );
    }
    let literal = Game::new("100% Genuine_Thing");
    store.upsert_game(&literal).await.unwrap();
    for query in ["%", "_"] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    query: Some(query.into()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            1
        );
    }
    store.delete_game(&game.id).await.unwrap();
    assert_eq!(
        store
            .list_games(&ListGames {
                query: Some("trigger".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .total,
        0
    );
    sqlx::query("INSERT INTO game_search(game_search,rank) VALUES('integrity-check',1)")
        .execute(&store.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn list_filters_stats_sort_and_pagination_use_persisted_state() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (a, ae) = game_entry(&store, "Alpha").await;
    let (mut b, mut be) = game_entry(&store, "Beta").await;
    b.favorite = true;
    store.upsert_game(&b).await.unwrap();
    be.launch = Some(LaunchSpec::Emulator {
        platform_id: "fixture-platform".into(),
        target: crate::emulation::EmulatorTarget::AskEmulator,
        roms: vec![crate::emulation::GameRom::absolute(
            "/fixture/game/game.rom",
        )],
        args: crate::emulation::ArgsOverride::Default,
    });
    store.upsert_entry(&be).await.unwrap();
    let mut hidden = Game::new("Hidden");
    hidden.hidden = true;
    store.upsert_game(&hidden).await.unwrap();
    let mut owned = Game::new("Owned");
    owned.sort_title = "00 Owned".into();
    store.upsert_game(&owned).await.unwrap();
    for (entry, start, duration) in [(&ae, 1000, 4000), (&be, 6000, 2000)] {
        let record = PlaySession::new(&entry.game_id, &entry.id, start);
        store.start_session(&record).await.unwrap();
        store
            .finish_session(&record.id, start + duration, duration as u64)
            .await
            .unwrap();
    }
    for (filter, count) in [
        (GameFilter::All, 3),
        (GameFilter::Installed, 2),
        (GameFilter::Favorites, 1),
        (GameFilter::Recent, 2),
        (GameFilter::Emulated, 1),
    ] {
        assert_eq!(
            store
                .list_games(&ListGames {
                    filter,
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            count
        );
    }
    assert_eq!(
        store
            .list_games(&ListGames {
                sort: GameSort::Recent,
                ..Default::default()
            })
            .await
            .unwrap()
            .items[0]
            .id,
        b.id
    );
    assert_eq!(
        store
            .list_games(&ListGames {
                sort: GameSort::Playtime,
                ..Default::default()
            })
            .await
            .unwrap()
            .items[0]
            .id,
        a.id
    );
    let page = store
        .list_games(&ListGames {
            offset: 1,
            limit: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, a.id);
    assert!(
        store
            .list_games(&ListGames {
                limit: 1001,
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert_eq!(
        store.library_stats().await.unwrap(),
        LibraryStats {
            total: 3,
            installed: 2,
            favorites: 1,
            recent: 2,
            emulated: 1,
            web: 0,
            owned: 0,
            playtime_ms: 6000
        }
    );
}

#[tokio::test]
async fn installed_entries_without_a_folder_count_as_installed() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (game, mut entry) = game_entry(&store, "Launcher only").await;
    entry.location = None;
    store.upsert_entry(&entry).await.unwrap();
    let page = store
        .list_games(&ListGames {
            filter: GameFilter::Installed,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, game.id);
    assert!(page.items[0].installed);
    assert_eq!(store.library_stats().await.unwrap().installed, 1);
}

#[tokio::test]
async fn web_games_show_in_the_web_tab_only() {
    let store = LibraryStore::in_memory().await.unwrap();
    let account = admin(&store).await;
    let web_entry = |game: &Game, url: &str| {
        let mut entry = GameEntry::new(&game.id, SourceId::new("web"), url);
        entry.state = InstallState::Installed;
        entry.launch = Some(LaunchSpec::Web { url: url.into() });
        entry
    };
    game_entry(&store, "Native").await;
    let mut web = Game::new("Web only");
    web.platforms = vec!["browser".into()];
    store.upsert_game(&web).await.unwrap();
    store
        .upsert_entry(&web_entry(&web, "https://a.example/"))
        .await
        .unwrap();
    // A game with a release of another kind shows in both.
    let (both, _) = game_entry(&store, "Both").await;
    store
        .upsert_entry(&web_entry(&both, "https://b.example/"))
        .await
        .unwrap();
    store.set_game_favorite(&web.id, true).await.unwrap();

    let titles = |filter| {
        let store = &store;
        async move {
            let page = store
                .list_games(&ListGames {
                    filter,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert_eq!(page.total as usize, page.items.len());
            page.items
                .into_iter()
                .map(|game| game.title)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(titles(GameFilter::All).await, ["Both", "Native"]);
    assert_eq!(titles(GameFilter::Installed).await, ["Both", "Native"]);
    assert!(titles(GameFilter::Favorites).await.is_empty());
    assert_eq!(titles(GameFilter::Web).await, ["Both", "Web only"]);
    let stats = store.library_stats().await.unwrap();
    assert_eq!(
        (stats.total, stats.installed, stats.favorites, stats.web),
        (2, 2, 0, 2)
    );
    assert!(
        store
            .library_platforms(GameFilter::All)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .library_platforms(GameFilter::Web)
            .await
            .unwrap()
            .len(),
        1
    );

    // A manual collection keeps what was put in it, and Settings lists a
    // hidden web game.
    let shelf = store
        .save_collection(
            &account,
            &SaveCollection {
                id: None,
                name: "Shelf".into(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    store
        .add_to_collection(&account, &shelf.id, &[web.id.clone()])
        .await
        .unwrap();
    assert_eq!(
        store
            .collection_games(&account, &shelf.id, 0, 10)
            .await
            .unwrap()
            .total,
        1
    );
    store.set_game_hidden(&web.id, true).await.unwrap();
    assert_eq!(store.hidden_games(0, 10).await.unwrap().total, 1);
}

#[tokio::test]
async fn sorts_by_added_and_release_date() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut old = Game::new("Old");
    old.release_date = Some("1998-11-21".into());
    store.upsert_game(&old).await.unwrap();
    let mut undated = Game::new("Undated");
    undated.release_date = None;
    store.upsert_game(&undated).await.unwrap();
    let mut new = Game::new("New");
    new.release_date = Some("2024-02-29".into());
    store.upsert_game(&new).await.unwrap();
    let entry = |game: &Game| GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    for game in [&undated, &new] {
        store.upsert_entry(&entry(game)).await.unwrap();
    }
    // A new release of the oldest game makes it the last added.
    store.upsert_entry(&entry(&old)).await.unwrap();
    // The newest game, uninstalled: nothing of it is left to have been added.
    let mut gone = Game::new("Uninstalled");
    gone.release_date = None;
    store.upsert_game(&gone).await.unwrap();
    let release = store.upsert_entry(&entry(&gone)).await.unwrap();
    store.delete_entry(&release.id).await.unwrap();
    let titles = |sort| {
        let store = &store;
        async move {
            store
                .list_games(&ListGames {
                    sort,
                    ..Default::default()
                })
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|game| game.title)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        titles(GameSort::Added).await,
        ["Old", "New", "Undated", "Uninstalled"]
    );
    assert_eq!(
        titles(GameSort::Released).await,
        ["New", "Old", "Undated", "Uninstalled"]
    );
    let page = store
        .list_games(&ListGames {
            sort: GameSort::Released,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items[0].release_date.as_deref(), Some("2024-02-29"));
}

#[tokio::test]
async fn thousand_game_search_stays_below_fifty_ms() {
    let store = LibraryStore::in_memory().await.unwrap();
    let games: Vec<_> = (0..1203)
        .map(|i| {
            let mut g = Game::new(format!("Fixture Game {i:04}"));
            g.aliases = vec![format!("Nickname {i:04}")];
            g.tags = vec![format!("genre{}", i % 7)];
            g
        })
        .collect();
    store.upsert_games(&games).await.unwrap();
    let mut timings = Vec::new();
    for query in ["1037", "Nickname 1037", "Fixture", "genre3", "001"]
        .into_iter()
        .cycle()
        .take(25)
    {
        let start = Instant::now();
        let result = store
            .list_games(&ListGames {
                query: Some(query.into()),
                ..Default::default()
            })
            .await
            .unwrap();
        timings.push(start.elapsed());
        assert!(result.total > 0);
    }
    timings.sort();
    let p95 = timings[timings.len() * 95 / 100];
    eprintln!(
        "1203-game FTS5 search median={:?} p95={p95:?} max={:?}",
        timings[timings.len() / 2],
        timings.last().unwrap()
    );
    assert!(p95 < Duration::from_millis(50), "p95={p95:?}");
}
