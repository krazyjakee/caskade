use super::*;
use serde_json::json;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{body_json, method, path},
};

fn game(id: u64, title: &str) -> Value {
    json!({
        "id": id,
        "slug": title.to_lowercase(),
        "title": title,
        "description": "",
        "genres": ["Platform"],
        "tags": [],
        "platforms": [{"name": "PC (Microsoft Windows)", "abbreviation": "PC"}],
        "releaseDate": "2020-01-02",
        "aliases": [],
        "externalIds": [{"uid": "292030", "source": 1}],
        "gameType": 0,
        "images": {
            "cover": {"imageId": "co1", "width": 528, "height": 748,
                      "url": "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co1.jpg"},
            "hero": null,
            "logo": null,
            "artworks": [],
            "screenshots": [],
            "futureField": true
        },
        "futureField": {"kept": true}
    })
}

/// Answers like the server: every requested id below 5000 is a game.
fn respond(request: &Request) -> ResponseTemplate {
    let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
    let (games, not_found): (Vec<u64>, Vec<u64>) = ids.into_iter().partition(|id| *id < 5000);
    ResponseTemplate::new(200).set_body_json(json!({
        "games": games.iter().map(|id| game(*id, &format!("Game {id}"))).collect::<Vec<_>>(),
        "notFound": not_found,
    }))
}

/// An address nothing listens on: port 1, which nothing in the tests binds
/// (it takes root on Unix). A port freed by a dropped listener could go to
/// another test's server and answer (#119).
fn unreachable() -> String {
    "http://127.0.0.1:1".to_owned()
}

async fn server() -> MockServer {
    MockServer::start().await
}

async fn bodies(server: &MockServer) -> Vec<Vec<u64>> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

#[tokio::test]
async fn answers_are_fetched_once() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let first = igdb.games(&[1942, 5000, 1942, 1020]).await.unwrap();
    let ids: Vec<u64> = first.games.iter().map(|game| game.id).collect();
    assert_eq!(ids, [1942, 1020]);
    assert_eq!(first.not_found, [5000]);
    assert_eq!(first.games[0].title, "Game 1942");
    assert_eq!(first.games[0].external_ids[0].uid, "292030");
    assert_eq!(
        first.games[0].images.cover.as_ref().unwrap().image_id,
        "co1"
    );

    // The same ids, found or not, come from the cache.
    let second = igdb.games(&[1020, 5000, 1942]).await.unwrap();
    assert_eq!(second.games.len(), 2);
    assert_eq!(second.games[0].id, 1020);
    assert_eq!(second.not_found, [5000]);

    // Only the new ids go to the server.
    igdb.games(&[1942, 7, 5000, 6000]).await.unwrap();
    assert_eq!(
        bodies(&server).await,
        [vec![1942, 5000, 1020], vec![7, 6000]]
    );
    assert_eq!(igdb.cached().await.unwrap(), 5);

    // Ids cross to the webview as strings, and read back from either form.
    let sent = serde_json::to_value(&first).unwrap();
    assert_eq!(sent["games"][0]["id"], "1942");
    assert_eq!(sent["notFound"], json!(["5000"]));
    assert_eq!(serde_json::from_value::<IgdbLookup>(sent).unwrap(), first);
}

#[tokio::test]
async fn the_cache_outlives_the_client() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(respond)
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();

    let first = Igdb::with_base_url(dir.path(), server.uri())
        .games(&[1, 2000])
        .await
        .unwrap();
    let again = Igdb::with_base_url(dir.path(), server.uri())
        .games(&[1, 2000])
        .await
        .unwrap();
    assert_eq!(first, again);
}

#[tokio::test]
async fn games_keep_the_fields_the_server_sent() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    Igdb::with_base_url(dir.path(), server.uri())
        .games(&[3])
        .await
        .unwrap();

    let store = Store::open(dir.path().join("games.sqlite")).await.unwrap();
    let raw: String = sqlx::query_scalar("SELECT game FROM games WHERE id = 3")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    let raw: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(raw["futureField"], json!({"kept": true}));
}

#[tokio::test]
async fn concurrent_lookups_share_a_request() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(move |request: &Request| {
            respond(request).set_delay(std::time::Duration::from_millis(200))
        })
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let lookups: Vec<_> = (0..8)
        .map(|_| {
            let igdb = igdb.clone();
            tokio::spawn(async move { igdb.games(&[10, 20, 9999]).await.unwrap() })
        })
        .collect();
    for lookup in lookups {
        let lookup = lookup.await.unwrap();
        assert_eq!(lookup.games.len(), 2);
        assert_eq!(lookup.not_found, [9999]);
    }
    assert_eq!(bodies(&server).await, [vec![10, 20, 9999]]);
}

#[tokio::test]
async fn large_lookups_are_batched() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let ids: Vec<u64> = (4501..=5700).collect();
    let lookup = igdb.games(&ids).await.unwrap();
    assert_eq!(lookup.games.len(), 499);
    assert_eq!(lookup.not_found.len(), 701);
    let sizes: Vec<usize> = bodies(&server).await.iter().map(Vec::len).collect();
    assert_eq!(sizes, [500, 500, 200]);

    igdb.games(&ids).await.unwrap();
    assert_eq!(bodies(&server).await.len(), 3);
}

#[tokio::test]
async fn failures_cache_nothing() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(502).set_body_json(json!({"error": "IGDB lookup failed"})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    igdb.games(&[1, 2]).await.unwrap_err();
    assert_eq!(igdb.cached().await.unwrap(), 0);
    igdb.unpause();
    assert_eq!(igdb.games(&[1, 2]).await.unwrap().games.len(), 2);
}

#[tokio::test]
async fn a_failure_pauses_every_request() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    assert_eq!(igdb.retry_in(), None);
    igdb.games(&[1]).await.unwrap_err();
    let wait = igdb.retry_in().unwrap();
    assert!(wait > Duration::from_secs(55) && wait <= PAUSE, "{wait:?}");
    // Nothing reaches the server until the pause is over.
    let error = igdb.games(&[2]).await.unwrap_err();
    assert!(error.to_string().contains("trying again in"), "{error}");
    igdb.refresh(&[1]).await.unwrap_err();
    igdb.match_stores(&[StoreId::new("steam", "1")])
        .await
        .unwrap_err();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn retry_after_sets_the_pause_and_a_success_ends_it() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1200"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "99999999"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    igdb.games(&[1]).await.unwrap_err();
    let wait = igdb.retry_in().unwrap();
    assert!(wait > Duration::from_secs(1190) && wait <= Duration::from_secs(1200));
    igdb.unpause();
    // No longer than six hours, whatever the server says.
    igdb.games(&[1]).await.unwrap_err();
    assert!(igdb.retry_in().unwrap() <= MAX_PAUSE);
    igdb.unpause();
    igdb.games(&[1]).await.unwrap();
    assert_eq!(igdb.retry_in(), None);
}

#[tokio::test]
async fn gzip_answers_are_decoded() {
    use std::io::Write;
    let server = server().await;
    let body = json!({"games": [game(1, "One")], "notFound": [2]}).to_string();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(body.as_bytes()).unwrap();
    Mock::given(method("POST"))
        .and(wiremock::matchers::header("Accept-Encoding", "gzip"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Encoding", "gzip")
                .set_body_raw(encoder.finish().unwrap(), "application/json"),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let lookup = igdb.games(&[1, 2]).await.unwrap();
    assert_eq!(lookup.games[0].title, "One");
    assert_eq!(lookup.not_found, [2]);
}

#[tokio::test]
async fn answers_for_ids_not_asked_for_are_ignored() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [game(1, "One"), game(99, "Stray")],
            "notFound": [98],
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    // 2 is in neither list: pending, and not cached.
    let lookup = igdb.games(&[1, 2]).await.unwrap();
    assert_eq!(lookup.games.len(), 1);
    assert!(lookup.not_found.is_empty());
    assert_eq!(lookup.pending, [2]);
    assert_eq!(igdb.cached().await.unwrap(), 1);
    assert_eq!(igdb.answered(&[1, 2]).await.unwrap(), HashSet::from([1]));
    // Nor asked about again for a while.
    igdb.games(&[1, 2]).await.unwrap();
    assert_eq!(bodies(&server).await, [vec![1, 2]]);
}

#[tokio::test]
async fn refresh_fetches_again() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(body_json(json!([5])))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [game(5, "Old")], "notFound": []
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_json(json!([5])))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [game(5, "New")], "notFound": []
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    assert_eq!(igdb.games(&[5]).await.unwrap().games[0].title, "Old");
    // An answer this fresh is kept.
    assert_eq!(igdb.refresh(&[5]).await.unwrap().games[0].title, "Old");
    assert_eq!(bodies(&server).await.len(), 1);
    sqlx::query("UPDATE games SET fetched = fetched - 600001")
        .execute(&igdb.store().await.unwrap().pool)
        .await
        .unwrap();
    assert_eq!(igdb.refresh(&[5]).await.unwrap().games[0].title, "New");
    assert_eq!(igdb.games(&[5]).await.unwrap().games[0].title, "New");
    assert_eq!(bodies(&server).await.len(), 2);
}

#[tokio::test]
async fn ids_igdb_cannot_have_are_never_sent() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let big = 1 << 63;
    let max = i64::MAX.cast_unsigned();
    let lookup = igdb.games(&[big, 3, max]).await.unwrap();
    assert_eq!(lookup.games[0].id, 3);
    assert_eq!(lookup.not_found, [big, max]);
    igdb.games(&[big, max]).await.unwrap();
    assert_eq!(bodies(&server).await, [vec![3, max]]);
    assert_eq!(igdb.cached().await.unwrap(), 2);
}

#[tokio::test]
async fn an_unreadable_game_does_not_sink_its_batch() {
    let server = server().await;
    let mut broken = game(2, "Broken");
    broken["images"]["cover"] = json!({"imageId": 5});
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [game(1, "One"), broken],
            "notFound": [],
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let lookup = igdb.games(&[1, 2]).await.unwrap();
    assert_eq!(lookup.games.len(), 1);
    assert_eq!(lookup.pending, [2]);
    // Only the readable game is cached; the other is asked for again,
    // but not for hours.
    assert_eq!(igdb.cached().await.unwrap(), 1);
    igdb.games(&[1, 2]).await.unwrap();
    assert_eq!(bodies(&server).await, [vec![1, 2]]);
    igdb.inner.unanswered.lock().unwrap().clear();
    igdb.games(&[1, 2]).await.unwrap();
    assert_eq!(bodies(&server).await, [vec![1, 2], vec![2]]);
    // A refresh asks straight away.
    igdb.refresh(&[2]).await.unwrap();
    assert_eq!(bodies(&server).await.len(), 3);
}

#[tokio::test]
async fn cached_answers_come_back_while_the_server_is_unreachable() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    Igdb::with_base_url(dir.path(), server.uri())
        .games(&[1, 5000])
        .await
        .unwrap();
    let igdb = Igdb::with_base_url(dir.path(), unreachable());

    let lookup = igdb.games(&[1, 2, 5000]).await.unwrap();
    assert_eq!(lookup.games[0].id, 1);
    assert_eq!(lookup.not_found, [5000]);
    assert_eq!(lookup.pending, [2]);
    let (_, failure, _) = igdb.lookup(&[3]).await.unwrap();
    assert!(failure.is_some());
    // With nothing to answer from the cache, the lookup fails, and says why.
    igdb.unpause();
    let error = igdb.games(&[2, 3]).await.unwrap_err();
    assert!(
        error.to_string().starts_with("can't reach 127.0.0.1"),
        "{error}"
    );
}

#[tokio::test]
async fn lookups_of_other_ids_do_not_wait_for_a_slow_one() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(|request: &Request| {
            let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
            let delay = if ids.contains(&1) { 3000 } else { 0 };
            respond(request).set_delay(Duration::from_millis(delay))
        })
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let slow = tokio::spawn({
        let igdb = igdb.clone();
        async move { igdb.games(&[1]).await.unwrap() }
    });
    while server.received_requests().await.unwrap().is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let started = Instant::now();
    let other = igdb.games(&[2]).await.unwrap();
    assert_eq!(other.games[0].id, 2);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "waited for the slow lookup"
    );
    // A lookup of the slow id and another waits for it, and asks only for the other.
    let both = igdb.games(&[1, 3]).await.unwrap();
    assert_eq!(both.games.len(), 2);
    assert!(slow.await.unwrap().games[0].id == 1);
    assert_eq!(bodies(&server).await, [vec![1], vec![2], vec![3]]);
}

#[tokio::test]
async fn an_answer_without_its_lists_is_a_failure() {
    let server = server().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"error": "upstream"})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    igdb.match_stores(&[StoreId::new("steam", "620")])
        .await
        .unwrap_err();
    assert!(igdb.retry_in().is_some());
    igdb.unpause();
    igdb.games(&[1]).await.unwrap_err();
    assert!(igdb.retry_in().is_some());
    assert_eq!(igdb.cached().await.unwrap(), 0);
}

#[tokio::test]
async fn store_ids_match_in_batches_and_only_as_asked() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/match"))
        .respond_with(|request: &Request| {
            let ids: Vec<StoreId> = serde_json::from_slice(&request.body).unwrap();
            let mut matches: Vec<Value> = ids
                .iter()
                .filter(|id| id.store == "steam")
                .map(|id| json!({"store": id.store, "uid": id.uid, "game": 7}))
                .collect();
            matches.push(json!({"store": "steam", "uid": "not-asked", "game": 9}));
            ResponseTemplate::new(200).set_body_json(json!({"matches": matches, "notFound": []}))
        })
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let mut ids: Vec<StoreId> = (0..600)
        .map(|uid| StoreId::new("steam", uid.to_string()))
        .collect();
    ids.push(StoreId::new("gog", "1"));
    ids.push(StoreId::new("steam", "0"));

    let found = igdb.match_stores(&ids).await.unwrap();
    assert_eq!(found.len(), 600);
    assert_eq!(found[&StoreId::new("steam", "599")], 7);
    assert!(!found.contains_key(&StoreId::new("gog", "1")));
    assert!(!found.contains_key(&StoreId::new("steam", "not-asked")));
    let sizes: Vec<usize> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| {
            serde_json::from_slice::<Vec<StoreId>>(&request.body)
                .unwrap()
                .len()
        })
        .collect();
    assert_eq!(sizes, [500, 101]);
}

#[tokio::test]
async fn searches_ask_the_server_and_cache_what_they_find() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/search"))
        .and(body_json(json!({"query": "witcher", "limit": 10})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [game(1942, "The Witcher 3"), game(80, "The Witcher")],
            "notFound": [],
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let found = igdb.search("  witcher ", 10).await.unwrap();
    let ids: Vec<u64> = found.iter().map(|game| game.id).collect();
    assert_eq!(ids, [1942, 80]);
    // Choosing one needs no request.
    assert_eq!(
        igdb.games(&[80]).await.unwrap().games[0].title,
        "The Witcher"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    // A blank query finds nothing without asking.
    assert!(igdb.search(" ", 10).await.unwrap().is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn new_fields_parse_and_answers_without_them_still_do() {
    let mut raw = game(1, "Rated");
    raw["platforms"] = json!([{"id": 6, "name": "PC (Microsoft Windows)", "abbreviation": "PC"}]);
    raw["ageRatings"] =
        json!([{"board": "pegi", "rating": "18"}, {"board": "esrb", "rating": "M"}]);
    raw["releaseDates"] = json!([
        {"platform": {"id": 6, "name": "PC", "abbreviation": "PC"}, "region": "europe",
         "date": "2026-07-01", "precision": "quarter", "year": 2026, "quarter": 3,
         "human": "Q3 2026", "status": null},
        {"platform": null, "region": null, "date": null, "precision": "decade", "human": "TBD"}
    ]);
    raw["storeLinks"] = json!([{"store": "steam", "name": "Steam",
        "url": "https://store.steampowered.com/app/1", "platforms": [], "countries": [826]}]);
    raw["websites"] = json!([{"kind": "official", "name": null, "url": "https://example.com"}]);
    raw["similarGames"] = json!([2, 3]);
    raw["status"] = json!("Released");
    let rated: IgdbGame = serde_json::from_value(raw).unwrap();
    assert_eq!(rated.platforms[0].id, Some(6));
    assert_eq!(age::min_age(&rated.age_ratings), Some(18));
    assert_eq!(rated.release_dates[0].precision, ReleasePrecision::Quarter);
    assert_eq!(rated.release_dates[0].quarter, Some(3));
    // A precision this build doesn't know reads as TBD.
    assert_eq!(rated.release_dates[1].precision, ReleasePrecision::Tbd);
    assert_eq!(rated.store_links[0].countries, [826]);
    assert_eq!(rated.similar_games, [2, 3]);
    let sent = serde_json::to_value(&rated).unwrap();
    assert_eq!(sent["similarGames"], json!(["2", "3"]));
    assert_eq!(sent["platforms"][0]["id"], "6");

    // An older server's game has none of them: unrated, not "for everyone".
    let old: IgdbGame = serde_json::from_value(game(2, "Old")).unwrap();
    assert!(old.age_ratings.is_empty() && old.store_links.is_empty());
    assert_eq!(age::min_age(&old.age_ratings), None);
    assert_eq!(old.platforms[0].id, None);
}

#[tokio::test]
async fn a_version_1_cache_is_kept_and_its_games_fetched_again_once() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(|request: &Request| {
            let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
            let games: Vec<Value> = ids
                .iter()
                .map(|id| {
                    let mut game = game(*id, "Fresh");
                    game["ageRatings"] = json!([{"board": "pegi", "rating": "16"}]);
                    game
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"games": games, "notFound": []}))
        })
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    // A cache as version 1 wrote it: a game and a not-found id.
    {
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.path().join("games.sqlite"))
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE games(id INTEGER PRIMARY KEY, game TEXT, fetched INTEGER NOT NULL);
             PRAGMA user_version = 1;",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO games VALUES(1, ?, 0), (5000, NULL, 0)")
            .bind(game(1, "Kept").to_string())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    // Offline, the old answers stand, and the game is reported outdated.
    let offline = Igdb::with_base_url(dir.path(), unreachable());
    let (lookup, failure, outdated) = offline.lookup(&[1, 5000]).await.unwrap();
    assert_eq!(lookup.games[0].title, "Kept");
    assert_eq!(lookup.not_found, [5000]);
    assert!(failure.is_some());
    assert_eq!(outdated, HashSet::from([1]));

    // Online, the game (not the not-found id) is fetched again, once.
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let (lookup, failure, outdated) = igdb.lookup(&[1, 5000]).await.unwrap();
    assert!(failure.is_none() && outdated.is_empty());
    assert_eq!(lookup.games[0].title, "Fresh");
    assert_eq!(age::min_age(&lookup.games[0].age_ratings), Some(16));
    igdb.games(&[1, 5000]).await.unwrap();
    assert_eq!(bodies(&server).await, [vec![1]]);
}

#[tokio::test]
async fn catalog_pages_are_kept_shared_and_answered_stale_on_failure() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/catalog/search"))
        .and(body_json(json!({
            "query": "zelda", "offset": 20, "limit": 20, "platforms": [130], "genres": [],
            "releasedFrom": "2017-01-01"
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"games": [game(7, "Zelda")], "nextOffset": 40}))
                .set_delay(Duration::from_millis(100)),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let search = CatalogSearch {
        query: Some(" zelda ".into()),
        offset: 20,
        limit: 20,
        platforms: vec![130, 130],
        released_from: Some("2017-01-01".into()),
        released_to: Some(" ".into()),
        ..CatalogSearch::default()
    };

    // Two at once share a request; a third comes from memory.
    let (first, second) = tokio::join!(igdb.catalog_search(&search), igdb.catalog_search(&search));
    let first = first.unwrap();
    assert_eq!(first, second.unwrap());
    assert_eq!(first.games[0].title, "Zelda");
    assert_eq!(first.next_offset, Some(40));
    assert!(!first.stale && first.releases.is_empty());
    assert_eq!(igdb.catalog_search(&search).await.unwrap(), first);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    // Its games are cached: opening one costs no request.
    assert_eq!(igdb.games(&[7]).await.unwrap().games[0].title, "Zelda");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    // Expired, and the server fails: the last copy, marked stale.
    igdb.expire_catalog();
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "30"))
        .mount(&server)
        .await;
    let stale = igdb.catalog_search(&search).await.unwrap();
    assert!(stale.stale);
    assert_eq!(stale.games, first.games);
    // Paused: answered without asking, still stale; a page never fetched fails.
    assert!(igdb.catalog_search(&search).await.unwrap().stale);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    let other = CatalogSearch {
        query: Some("mario".into()),
        ..CatalogSearch::default()
    };
    assert!(igdb.catalog_search(&other).await.is_err());
    assert!(igdb.retry_in().unwrap() > Duration::from_secs(20));
}

#[tokio::test]
async fn catalog_requests_out_of_range_are_refused_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), unreachable());
    let deep = CatalogSearch {
        offset: 5001,
        ..CatalogSearch::default()
    };
    let bad_date = CatalogSearch {
        released_to: Some("2017-1-1".into()),
        ..CatalogSearch::default()
    };
    let many = CatalogReleases {
        platforms: (1..=51).collect(),
        ..CatalogReleases::default()
    };
    for error in [
        igdb.catalog_search(&deep).await.unwrap_err(),
        igdb.catalog_search(&bad_date).await.unwrap_err(),
        igdb.catalog_upcoming(&many).await.unwrap_err(),
    ] {
        assert!(matches!(error, Error::Invalid(_)), "{error}");
    }
    assert!(igdb.retry_in().is_none());
}

#[tokio::test]
async fn upcoming_releases_carry_the_release_that_listed_them() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/catalog/upcoming"))
        .and(body_json(json!({"offset": 0, "limit": 20, "platforms": [], "genres": [], "tbd": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [game(8, "Soon"), game(9, "Later")],
            "releases": [
                {"game": 8, "platform": {"id": 508, "name": "Nintendo Switch 2"}, "region": "worldwide",
                 "date": null, "precision": "tbd", "human": "TBD", "status": null},
                {"game": 9, "platform": null, "region": null, "date": null, "precision": "tbd",
                 "human": "TBD", "status": "Cancelled"},
                // A release for a game the page doesn't have, and an unreadable one.
                {"game": 10, "precision": "tbd"},
                {"precision": "tbd"}
            ],
            "nextOffset": null,
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/catalog/recent"))
        .and(body_json(
            json!({"offset": 0, "limit": 50, "platforms": [6], "genres": []}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "games": [], "releases": [], "nextOffset": null,
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let page = igdb
        .catalog_upcoming(&CatalogReleases {
            tbd: true,
            ..CatalogReleases::default()
        })
        .await
        .unwrap();
    let listed: Vec<u64> = page.releases.iter().map(|release| release.game).collect();
    assert_eq!(listed, [8, 9]);
    assert_eq!(
        page.releases[1].release.status.as_deref(),
        Some("Cancelled")
    );
    assert_eq!(
        page.releases[0].release.platform.as_ref().unwrap().id,
        Some(508)
    );
    assert_eq!(page.next_offset, None);
    let sent = serde_json::to_value(&page).unwrap();
    assert_eq!(sent["releases"][0]["game"], "8");
    // Recent never sends `tbd`, and caps the limit.
    let recent = igdb
        .catalog_recent(&CatalogReleases {
            limit: 500,
            platforms: vec![6],
            tbd: true,
            ..CatalogReleases::default()
        })
        .await
        .unwrap();
    assert!(recent.games.is_empty());
}

#[tokio::test]
async fn catalog_filters_cross_as_strings() {
    let server = server().await;
    Mock::given(method("GET"))
        .and(path("/catalog/filters"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "platforms": [{"id": 6, "name": "PC (Microsoft Windows)", "abbreviation": "PC"}],
            "genres": [{"id": 12, "name": "Role-playing (RPG)"}],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    let filters = igdb.catalog_filters().await.unwrap();
    assert_eq!(filters, igdb.catalog_filters().await.unwrap());
    let sent = serde_json::to_value(&filters).unwrap();
    assert_eq!(sent["platforms"][0]["id"], "6");
    assert_eq!(sent["genres"][0]["abbreviation"], Value::Null);
    assert_eq!(sent["stale"], false);
}

#[tokio::test]
async fn catalog_games_fetch_old_answers_again_and_keep_them_offline() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());
    // Fetched long ago, in the current shape.
    igdb.store()
        .await
        .unwrap()
        .put_shape(1, &game(1, "Old copy"), store::SHAPE)
        .await
        .unwrap();
    igdb.games(&[2]).await.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    let lookup = igdb.catalog_games(&[1, 2]).await.unwrap();
    assert_eq!(lookup.games[0].title, "Game 1");
    // Only the old one is asked for again.
    assert_eq!(bodies(&server).await, [vec![2], vec![1]]);

    let offline = Igdb::with_base_url(dir.path(), unreachable());
    offline
        .store()
        .await
        .unwrap()
        .put_shape(1, &game(1, "Old copy"), store::SHAPE)
        .await
        .unwrap();
    let kept = offline.catalog_games(&[1, 3]).await.unwrap();
    assert_eq!(kept.games[0].title, "Old copy");
    assert_eq!(kept.pending, [3]);
    assert!(offline.catalog_games(&[3]).await.is_err());
}

/// `cargo test -p caskade-core --lib igdb::tests::live -- --ignored`
#[tokio::test]
#[ignore = "needs the live metadata server"]
async fn live() {
    let dir = tempfile::tempdir().unwrap();
    let igdb = Igdb::new(dir.path());
    let lookup = igdb.games(&[1942, 999_999_999, u64::MAX]).await.unwrap();
    assert_eq!(
        lookup.games[0].slug.as_deref(),
        Some("the-witcher-3-wild-hunt")
    );
    assert_eq!(lookup.not_found, [999_999_999, u64::MAX]);
    assert_eq!(igdb.cached().await.unwrap(), 2);
    let found = igdb.search("witcher 3", 5).await.unwrap();
    assert!(found.iter().any(|game| game.id == 1942));
    // The Witcher 3 is ESRB M and PEGI 18.
    let ratings = &lookup.games[0].age_ratings;
    assert!(
        ratings
            .iter()
            .any(|r| r.board == "pegi" && r.rating == "18"),
        "{ratings:?}"
    );
    assert_eq!(age::min_age(ratings), Some(18));
}

/// A damaged cache (#160) is set aside and refilled from the server instead
/// of failing every lookup until someone deletes it by hand.
#[tokio::test]
async fn a_damaged_cache_is_started_again() {
    let server = server().await;
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(respond)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("games.sqlite"), vec![0x5a; 8192]).unwrap();
    let igdb = Igdb::with_base_url(dir.path(), server.uri());

    let lookup = igdb.games(&[1942]).await.unwrap();
    assert_eq!(lookup.games[0].title, "Game 1942");
    assert!(dir.path().join("games.sqlite.damaged").exists());
    assert_eq!(igdb.cached().await.unwrap(), 1);
}
