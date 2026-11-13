//! Runs: an unreachable server, retries, and images that can't come.

use super::*;

#[tokio::test]
async fn an_unreachable_server_leaves_games_for_the_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let fixture = Fixture::with_hasheous(dir, down, hasheous).await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.matched, report.deferred, report.unreachable),
        (0, 1, true)
    );
    assert_eq!(fixture.metadata.game(&game.id).await.unwrap(), None);
    // Still due, but the server is left alone for a while.
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 1);
    fixture.metadata.inner.igdb.unpause();
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 2);
}

#[tokio::test]
async fn a_run_stops_at_the_first_failure() {
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let fixture = Fixture::with_hasheous(dir, down, hasheous).await;
    for app_id in 0..(CHUNK as u32 * 2 + 1) {
        fixture
            .add(
                &format!("Game {app_id}"),
                vec![ExternalId::Steam { app_id }],
            )
            .await;
    }

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.deferred, report.unreachable), (CHUNK * 2 + 1, true));
    assert_eq!(fixture.server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn new_dumps_look_again_locally_and_the_server_only_weekly() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 1);
    let mut row = fixture.store.igdb_match(&game.id).await.unwrap().unwrap();
    let checked = row.checked_at_ms;
    row.dumps = "older dumps".into();
    fixture.store.save_igdb_match(&row).await.unwrap();
    // Unknown dumps are no change.
    assert!(
        fixture
            .store
            .igdb_pending(None, 0)
            .await
            .unwrap()
            .is_empty()
    );

    // Only the Hasheous dumps are asked again.
    assert_eq!(fixture.metadata.run().await.unwrap().deferred, 0);
    assert_eq!(fixture.requests("/match").await, 1);
    let row = fixture.store.igdb_match(&game.id).await.unwrap().unwrap();
    assert_eq!(row.dumps, fixture.metadata.dumps().await.unwrap());
    assert_eq!(
        (row.source, row.checked_at_ms),
        (MatchSource::None, checked)
    );
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());

    // A week on, the server is asked again.
    let mut row = row;
    row.checked_at_ms = 0;
    fixture.store.save_igdb_match(&row).await.unwrap();
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/match").await, 2);
}

#[tokio::test]
async fn a_game_the_server_does_not_send_undoes_nothing() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    // From now on the server answers 1942 with neither a game nor "none".
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"games": [], "notFound": []})),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let mut row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    row.igdb_id = Some(1942);
    row.pending = true;
    fixture.store.save_igdb_match(&row).await.unwrap();

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.applied, report.deferred, report.unreachable),
        (0, 1, false)
    );
    let row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    assert_eq!((row.igdb_id, row.pending), (Some(1942), true));
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );
    let error = fixture.metadata.refresh(&portal.id).await.unwrap_err();
    assert!(
        error.to_string().contains("no readable game 1942"),
        "{error}"
    );
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );
}

#[test]
fn retries_wait_longer_after_each_failure() {
    let minutes = |failures, random| retry_delay(failures, random, MAX_RETRY).as_secs() / 60;
    // `random` 2000 is no jitter; 0 is a fifth less, 3999 nearly a fifth more.
    assert_eq!(minutes(1, 2000), 10);
    assert_eq!(minutes(2, 2000), 20);
    assert_eq!(minutes(3, 2000), 40);
    assert_eq!(minutes(7, 2000), 360);
    assert_eq!(minutes(u32::MAX, 2000), 360);
    assert_eq!(minutes(1, 0), 8);
    assert_eq!(minutes(7, 3999), 431);
    // Offline, an hour at most, so the network coming back is noticed.
    let offline = |failures| retry_delay(failures, 2000, MAX_OFFLINE_RETRY).as_secs() / 60;
    assert_eq!(offline(3), 40);
    assert_eq!(offline(4), 60);
    assert_eq!(offline(u32::MAX), 60);
}

#[tokio::test]
async fn an_image_that_cannot_come_leaves_the_game_without_it() {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/img/co72.png"))
        .respond_with(ResponseTemplate::new(404))
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.applied, report.deferred, report.unreachable),
        (1, 0, false)
    );
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [(ArtworkKind::Icon, "igdb:ic72".to_owned())]
    );
    let row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    assert!(!row.pending);
    // Nothing is left to do, so the cover isn't asked for again.
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());
    assert_eq!(fixture.requests("/img/co72.png").await, 1);
}

#[tokio::test]
async fn a_new_cover_that_cannot_download_keeps_the_old_one() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    Mock::given(method("GET"))
        .and(path("/img/co1942.png"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&fixture.server)
        .await;

    let set = fixture.metadata.set(&portal.id, Some(1942)).await.unwrap();
    assert!(set.pending);
    // The new icon replaced the old; the old cover stays until the new one
    // is in.
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Cover, "igdb:co72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic1942".to_owned()),
        ]
    );

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.applied, report.deferred), (1, 0));
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Cover, "igdb:co1942".to_owned()),
            (ArtworkKind::Icon, "igdb:ic1942".to_owned()),
        ]
    );
}

#[tokio::test]
async fn an_unreachable_image_host_stops_the_run() {
    let fixture = Fixture::new().await;
    // The metadata server answers, but its images are somewhere unreachable:
    // port 1, which nothing in the tests binds (it takes root on Unix), so a
    // connection is always refused. A port freed by a dropped listener could
    // go to another test's server and answer (#119).
    let gone = "http://127.0.0.1:1".to_owned();
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(move |request: &Request| {
            let ids: Vec<u64> = serde_json::from_slice(&request.body).unwrap();
            let games: Vec<Value> = ids
                .iter()
                .map(|id| igdb_game(&gone, *id, "Portal 2"))
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"games": games, "notFound": []}))
        })
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    // Different IGDB games, so none of them merge.
    for app_id in 0..=CHUNK {
        let game = fixture.add(&format!("Game {app_id}"), vec![]).await;
        let mut row = IgdbMatch::new(&game.id, Some(1000 + app_id as u64), MatchSource::Store, "");
        row.pending = true;
        fixture.store.save_igdb_match(&row).await.unwrap();
    }

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.deferred, report.unreachable), (CHUNK + 1, true));
    // Only the first chunk was looked up; its text went in.
    assert_eq!(fixture.requests("/games").await, 1);
    let first = fixture.store.igdb_pending(None, 0).await.unwrap();
    assert_eq!(first.len(), CHUNK + 1);
    assert_eq!(
        fixture.game(&first[0]).await.description,
        "Portal 2 from IGDB."
    );
}

#[tokio::test]
async fn a_page_image_download_does_not_hold_up_a_match_by_hand() {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/img/ar72.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(png())
                .set_delay(std::time::Duration::from_secs(3)),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();

    let page = tokio::spawn({
        let metadata = fixture.metadata.clone();
        let id = portal.id.clone();
        async move { metadata.page_images(&id).await.unwrap() }
    });
    while fixture.requests("/img/ar72.png").await == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let started = std::time::Instant::now();
    let set = fixture.metadata.set(&portal.id, None).await.unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the match waited for the page's images"
    );
    assert_eq!((set.igdb_id, set.source), (None, MatchSource::Manual));
    // The page's images are for a match that is gone.
    assert!(!page.await.unwrap());
    assert!(fixture.artwork(&portal.id).await.is_empty());
}
