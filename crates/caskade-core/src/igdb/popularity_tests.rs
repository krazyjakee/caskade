use super::*;
use crate::{
    db::LibraryStore,
    metadata::{IgdbMatch, MatchSource},
    model::{Collection, CollectionKind, Game, GameSort, ListGames},
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

async fn matched(store: &LibraryStore, title: &str, id: Option<u64>) -> String {
    let game = Game::new(title);
    store.upsert_game(&game).await.unwrap();
    rematch(store, &game.id, id).await;
    game.id
}

async fn rematch(store: &LibraryStore, game: &str, id: Option<u64>) {
    store
        .save_igdb_match(&IgdbMatch {
            game_id: game.into(),
            igdb_id: id,
            source: MatchSource::Manual,
            pending: false,
            checked_at_ms: 0,
            dumps: String::new(),
            applied: Default::default(),
        })
        .await
        .unwrap();
}

async fn titles(store: &LibraryStore) -> Vec<String> {
    store
        .list_games(&ListGames {
            sort: GameSort::Popular,
            ..Default::default()
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.title)
        .collect()
}

fn response() -> Value {
    json!({"scores": [
        {"gameId": 1, "popularityType": 91, "value": 0.8, "calculatedAt": 100},
        {"gameId": 2, "popularityType": 91, "value": 0.2, "calculatedAt": 100},
        {"gameId": 3, "popularityType": 91, "value": 0, "calculatedAt": 100}
    ], "notFound": [4]})
}

#[tokio::test]
async fn popularity_is_daily_shared_and_survives_restart_offline_and_rematching() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/popularity"))
        .and(body_json(json!([1, 2, 3, 4])))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(response())
                .set_delay(Duration::from_millis(50)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("library.sqlite");
    let store = LibraryStore::open(&database).await.unwrap();
    let high = matched(&store, "Z high", Some(1)).await;
    matched(&store, "B low", Some(2)).await;
    matched(&store, "C zero", Some(3)).await;
    matched(&store, "A missing", Some(4)).await;
    matched(&store, "D unmatched", None).await;
    let igdb = Igdb::with_base_url(dir.path().join("igdb"), server.uri());
    let (a, b) = tokio::join!(
        igdb.refresh_popularity(&store),
        igdb.refresh_popularity(&store)
    );
    assert!(a.unwrap());
    assert!(!b.unwrap());
    assert_eq!(
        titles(&store).await,
        ["Z high", "B low", "C zero", "A missing", "D unmatched"]
    );
    assert!(
        store
            .popularity_due(now_ms() - popularity::TTL_MS)
            .await
            .unwrap()
            .is_empty()
    );

    let account = store.accounts().await.unwrap()[0].id.clone();
    let smart = Collection {
        kind: CollectionKind::Smart {
            query: ListGames {
                sort: GameSort::Popular,
                ..Default::default()
            },
        },
        ..Collection::manual("Popular")
    };
    store.upsert_collection(&account, &smart).await.unwrap();
    let page = store
        .collection_games(&account, &smart.id, 1, 2)
        .await
        .unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(
        page.items
            .iter()
            .map(|g| g.title.as_str())
            .collect::<Vec<_>>(),
        ["B low", "C zero"]
    );
    store.set_game_hidden(&high, true).await.unwrap();
    assert_eq!(
        store
            .collection_games(&account, &smart.id, 0, 10)
            .await
            .unwrap()
            .total,
        4
    );
    store.set_game_hidden(&high, false).await.unwrap();

    // Expire persisted answers, then start again without a reachable server.
    store
        .save_popularity(&serde_json::from_value(response()).unwrap(), 0)
        .await
        .unwrap();
    store.close().await;
    let store = LibraryStore::open(&database).await.unwrap();
    let offline = Igdb::with_base_url(dir.path().join("igdb"), "http://127.0.0.1:1");
    assert!(offline.refresh_popularity(&store).await.is_err());
    assert_eq!(titles(&store).await[0], "Z high");
    rematch(&store, &high, None).await;
    assert_eq!(
        titles(&store).await,
        ["B low", "C zero", "A missing", "D unmatched", "Z high"]
    );
    rematch(&store, &high, Some(3)).await;
    assert_eq!(
        titles(&store).await,
        ["B low", "C zero", "Z high", "A missing", "D unmatched"]
    );
}

#[tokio::test]
async fn bad_popularity_answers_keep_scores_and_retry_after_pauses_requests() {
    let server = MockServer::start().await;
    let store = LibraryStore::in_memory().await.unwrap();
    matched(&store, "Scored", Some(1)).await;
    let old: popularity::PopularityResponse = serde_json::from_value(
        json!({"scores": [response()["scores"][0].clone()], "notFound": []}),
    )
    .unwrap();
    store.save_popularity(&old, 0).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    Mock::given(path("/popularity"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"scores": [], "notFound": []})),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert!(igdb.refresh_popularity(&store).await.is_err());
    assert!(igdb.refresh_popularity(&store).await.is_err());
    assert_eq!(store.popularity_due(now_ms()).await.unwrap(), [1]);
    assert!(igdb.retry_in().is_some());
    server.reset().await;
    igdb.unpause();
    Mock::given(path("/popularity"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
        .expect(1)
        .mount(&server)
        .await;
    assert!(igdb.refresh_popularity(&store).await.is_err());
    assert!(igdb.retry_in().unwrap().as_secs() >= 119);
    assert!(igdb.refresh_popularity(&store).await.is_err());
}

#[tokio::test]
async fn popular_catalog_pages_forward_sort_and_keep_filtered_paging_separate() {
    let server = MockServer::start().await;
    Mock::given(path("/catalog/search")).and(body_json(json!({"query":"zelda", "sort":"popular", "offset":24, "limit":24, "platforms":[130], "genres":[12], "releasedFrom":"2020-01-01"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"games":[], "nextOffset":48})))
        .expect(1).mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let search = CatalogSearch {
        query: Some("zelda".into()),
        sort: CatalogSort::Popular,
        offset: 24,
        limit: 24,
        platforms: vec![130],
        genres: vec![12],
        released_from: Some("2020-01-01".into()),
        ..Default::default()
    };
    assert_eq!(
        igdb.catalog_search(&search).await.unwrap().next_offset,
        Some(48)
    );
    assert_eq!(
        igdb.catalog_search(&search).await.unwrap().next_offset,
        Some(48)
    );
    assert!(
        igdb.catalog_search(&CatalogSearch {
            sort: CatalogSort::Newest,
            ..search
        })
        .await
        .is_err()
    );
}

#[tokio::test]
async fn a_merged_game_uses_its_kept_match_score() {
    let store = LibraryStore::in_memory().await.unwrap();
    let keep = matched(&store, "Kept", Some(2)).await;
    let donor = matched(&store, "Donor", Some(1)).await;
    matched(&store, "Middle", Some(3)).await;
    let response: popularity::PopularityResponse = serde_json::from_value(json!({"scores":[
        {"gameId":1,"popularityType":91,"value":0.9,"calculatedAt":100},
        {"gameId":2,"popularityType":91,"value":0.1,"calculatedAt":100},
        {"gameId":3,"popularityType":91,"value":0.5,"calculatedAt":100}
    ],"notFound":[]}))
    .unwrap();
    store.save_popularity(&response, now_ms()).await.unwrap();
    store.merge_games(&keep, &donor).await.unwrap();
    assert_eq!(titles(&store).await, ["Middle", "Kept"]);
}
