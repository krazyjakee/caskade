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
    offers_popularity(&server).await;
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

/// #253: a server without popularity yet (production before its deploy)
/// pauses popularity alone. The last scores still sort, nothing else is
/// paused, and the endpoint isn't asked again for an hour.
#[tokio::test]
async fn a_server_without_popularity_pauses_it_alone_and_keeps_the_last_scores() {
    let server = MockServer::start().await;
    let store = LibraryStore::in_memory().await.unwrap();
    matched(&store, "Low", Some(2)).await;
    matched(&store, "High", Some(1)).await;
    matched(&store, "Alpha", Some(4)).await;
    let old: popularity::PopularityResponse = serde_json::from_value(json!({"scores": [
        {"gameId": 1, "popularityType": 91, "value": 0.8, "calculatedAt": 100},
        {"gameId": 2, "popularityType": 91, "value": 0.2, "calculatedAt": 100}
    ], "notFound": []}))
    .unwrap();
    // Old enough to be due again.
    store.save_popularity(&old, 0).await.unwrap();
    Mock::given(path("/popularity"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    assert!(igdb.refresh_popularity(&store).await.is_err());
    assert!(igdb.retry_in().is_none(), "other requests go on");
    assert!(igdb.popularity_retry_in().unwrap() > Duration::from_secs(55 * 60));
    // Within that pause it isn't asked again (`expect(1)`).
    assert!(igdb.refresh_popularity(&store).await.is_err());
    // Sorting keeps the last scores, and games without one follow by title.
    assert_eq!(titles(&store).await, ["High", "Low", "Alpha"]);
}

/// A server with popularity (#253): it answers the client's check.
async fn offers_popularity(server: &MockServer) {
    Mock::given(path("/popularity"))
        .and(body_json(json!([1942])))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"scores": [], "notFound": [1942]})),
        )
        .mount(server)
        .await;
}

fn popular(offset: u32) -> CatalogSearch {
    CatalogSearch {
        sort: CatalogSort::Popular,
        offset,
        ..Default::default()
    }
}

/// #253: a server without the sorts (production before its deploy) would
/// ignore `sort` and answer in its usual order as if it were popular. The
/// client learns it has no popularity, asks for the usual order and says
/// so, without pausing anything; it doesn't ask again for an hour.
#[tokio::test]
async fn a_server_without_the_sorts_answers_its_usual_order_flagged() {
    let server = MockServer::start().await;
    Mock::given(path("/popularity"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/catalog/search"))
        .and(body_json(
            json!({"sort": "popular", "offset": 0, "limit": 20, "platforms": [], "genres": []}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"games": []})))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(path("/catalog/search"))
        .and(body_json(
            json!({"offset": 20, "limit": 20, "platforms": [], "genres": []}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"games": [], "nextOffset": 40})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/catalog/search"))
        .and(body_json(
            json!({"offset": 0, "limit": 20, "platforms": [], "genres": []}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"games": [], "nextOffset": 20})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let page = igdb.catalog_search(&popular(0)).await.unwrap();
    assert!(page.sort_unavailable);
    assert_eq!(page.next_offset, Some(20));
    // The next page too, without asking about popularity again.
    let next = igdb.catalog_search(&popular(20)).await.unwrap();
    assert!(next.sort_unavailable);
    assert_eq!(next.next_offset, Some(40));
    assert!(igdb.retry_in().is_none(), "nothing else is paused");
    assert!(igdb.popularity_retry_in().is_some());
}

/// #253: a server with popularity that still refuses the sort answers the
/// usual order, flagged; one that's down is no refusal: an error and the
/// shared pause.
#[tokio::test]
async fn a_sort_the_server_refuses_answers_its_usual_order() {
    let server = MockServer::start().await;
    offers_popularity(&server).await;
    Mock::given(path("/catalog/search"))
        .and(body_json(
            json!({"sort": "popular", "offset": 0, "limit": 20, "platforms": [], "genres": []}),
        ))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/catalog/search"))
        .and(body_json(
            json!({"offset": 0, "limit": 20, "platforms": [], "genres": []}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"games": [], "nextOffset": 20})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let page = igdb.catalog_search(&popular(0)).await.unwrap();
    assert!(page.sort_unavailable);
    assert_eq!(page.next_offset, Some(20));
    assert!(igdb.retry_in().is_none());
    server.reset().await;
    Mock::given(path("/catalog/search"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    assert!(igdb.catalog_search(&popular(20)).await.is_err());
    assert!(igdb.retry_in().is_some());
}

/// The live server's popularity, once it is deployed with it (#253):
/// `cargo test -p caskade-core --lib igdb::popularity_tests::live -- --ignored`
#[tokio::test]
#[ignore = "needs the live metadata server, deployed with popularity"]
async fn live() {
    // The Witcher 3, Portal 2, Celeste.
    let ids = [1942, 72, 26226];
    let response = super::fetch::popularity(BASE_URL, &ids)
        .await
        .map_err(|failure| failure.error)
        .expect("POST /popularity answers");
    assert!(!response.scores.is_empty(), "{response:?}");
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::new(dir.path());
    let page = igdb
        .catalog_search(&CatalogSearch {
            sort: CatalogSort::Popular,
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!page.sort_unavailable, "the server sorts by popularity");
    assert_eq!(page.games.len(), 10);
    assert!(page.next_offset.is_some());
}
