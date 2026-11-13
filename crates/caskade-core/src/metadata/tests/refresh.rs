//! Refreshing chosen games: choices kept, cancels, failures and retries.

use super::*;

#[tokio::test]
async fn refreshing_chosen_games_matches_fills_in_and_respects_choices() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let unknown = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;
    let mine = fixture
        .add("Witcher 3", vec![ExternalId::Gog { product_id: 1 }])
        .await;
    fixture.metadata.set(&mine.id, None).await.unwrap();
    let ids = vec![
        portal.id.clone(),
        unknown.id.clone(),
        mine.id.clone(),
        "gone".to_owned(),
    ];
    let mut seen = Vec::new();
    let status = fixture
        .metadata
        .refresh_games(
            &ids,
            RefreshMode::Missing,
            &CancellationToken::new(),
            |status| seen.push(status.done),
        )
        .await
        .unwrap();
    assert_eq!(
        (
            status.updated,
            status.unmatched,
            status.skipped,
            status.failed.len()
        ),
        (1, 1, 2, 0)
    );
    assert!(!status.running);
    assert_eq!(seen.last(), Some(&4));
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );
    // "Not on IGDB" stays so.
    let none = fixture.metadata.game(&mine.id).await.unwrap().unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::Manual));

    // A field the user set stays theirs through an automatic refresh, and
    // the refresh asks for the matched games in one request.
    let edit = GameEdit {
        description: Some(TextEdit::Set {
            value: "Mine.".into(),
        }),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&portal.id, &edit, None)
        .await
        .unwrap();
    let before = fixture.requests("/games").await;
    let status = fixture
        .metadata
        .refresh_games(
            &ids,
            RefreshMode::Automatic,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert!(status.failed.is_empty());
    assert_eq!(fixture.game(&portal.id).await.description, "Mine.");
    assert!(fixture.requests("/games").await <= before + 1);
}

#[tokio::test]
async fn a_cancelled_refresh_leaves_the_rest_and_a_failed_one_can_be_retried() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let status = fixture
        .metadata
        .refresh_games(
            std::slice::from_ref(&portal.id),
            RefreshMode::Missing,
            &cancel,
            |_| {},
        )
        .await
        .unwrap();
    assert!(status.cancelled);
    assert_eq!(status.remaining, std::slice::from_ref(&portal.id));
    assert!(fixture.game(&portal.id).await.description.is_empty());

    // The server down: the game is listed to retry, and keeps what it had.
    let dir = tempfile::tempdir().unwrap();
    let down = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&down)
        .await;
    let hasheous = Hasheous::with_base_url(dir.path().join("hasheous"), down.uri());
    let offline = Fixture::with_hasheous(dir, down, hasheous).await;
    let game = offline
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let status = offline
        .metadata
        .refresh_games(
            std::slice::from_ref(&game.id),
            RefreshMode::Automatic,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(status.failed, std::slice::from_ref(&game.id));
    assert_eq!(offline.game(&game.id).await.title, "Portal 2");
}

impl Fixture {
    /// Refreshes the missing details of `ids`, as Retry does for `failed`.
    async fn refresh(&self, ids: &[String]) -> RefreshStatus {
        self.metadata
            .refresh_games(ids, RefreshMode::Missing, &CancellationToken::new(), |_| {})
            .await
            .unwrap()
    }

    async fn igdb_id(&self, game_id: &str) -> Option<u64> {
        let found = self.metadata.game(game_id).await.unwrap().unwrap();
        found.igdb_id
    }
}

#[tokio::test]
async fn a_failed_epic_read_fails_the_game_in_a_refresh_and_a_retry_matches_it() {
    let fixture = Fixture::new().await;
    epic_catalogue(&fixture.server).await;
    let found = fixture.add("Satisfactory", epic("crab")).await;
    let gone = fixture.add("Gone", epic("gone")).await;
    let offline = fixture.add("Offline", epic("offline")).await;

    let status = fixture
        .refresh(&[found.id.clone(), gone.id.clone(), offline.id.clone()])
        .await;
    // Epic's own "no such namespace" is still not found; only the game it
    // never answered for is failed.
    assert_eq!(
        (status.updated, status.unmatched, status.unchanged),
        (1, 1, 0)
    );
    assert_eq!(status.failed, std::slice::from_ref(&offline.id));
    assert!(
        fixture
            .store
            .igdb_match(&offline.id)
            .await
            .unwrap()
            .is_none()
    );

    // Epic answers again, and Retry takes what the status listed.
    Mock::given(method("GET"))
        .and(path(
            "/epic/catalog/catalog/api/shared/namespace/offline/offers",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"elements": [
                {"id": "offer-crab", "categories": [{"path": "games/edition/base"}]},
            ]})),
        )
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let status = fixture.refresh(&status.failed).await;
    assert_eq!((status.updated, status.unmatched), (1, 0));
    assert!(status.failed.is_empty());
    assert_eq!(fixture.igdb_id(&offline.id).await, Some(1942));
}

#[tokio::test]
async fn an_expired_epic_token_fails_the_game_in_a_refresh_and_a_retry_renews_it() {
    let fixture = Fixture::new().await;
    epic_catalogue(&fixture.server).await;
    Mock::given(method("GET"))
        .and(path(
            "/epic/catalog/catalog/api/shared/namespace/crab/offers",
        ))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let game = fixture.add("Satisfactory", epic("crab")).await;

    let status = fixture.refresh(std::slice::from_ref(&game.id)).await;
    assert_eq!(status.unmatched, 0);
    assert_eq!(status.failed, std::slice::from_ref(&game.id));

    let status = fixture.refresh(&status.failed).await;
    assert_eq!((status.updated, status.failed.len()), (1, 0));
    assert_eq!(fixture.igdb_id(&game.id).await, Some(1942));
    // The refused token was dropped, not sent again.
    assert_eq!(
        fixture
            .requests("/epic/account/account/api/oauth/token")
            .await,
        2
    );
}

#[tokio::test]
async fn dumps_that_cannot_be_searched_fail_the_game_in_a_refresh_and_a_retry_matches_it() {
    let dir = tempfile::tempdir().unwrap();
    // A file where the cache's folder goes: not damage it can start again
    // from, so every search of the dumps fails.
    let folder = dir.path().join("hasheous");
    std::fs::write(&folder, "").unwrap();
    let server = server().await;
    let hasheous = Hasheous::with_base_url(&folder, server.uri());
    let fixture = Fixture::with_hasheous(dir, server, hasheous).await;
    let worms = fixture.add("Worms", Vec::new()).await;

    let status = fixture.refresh(std::slice::from_ref(&worms.id)).await;
    assert_eq!(status.unmatched, 0);
    assert_eq!(status.failed, std::slice::from_ref(&worms.id));
    assert!(fixture.store.igdb_match(&worms.id).await.unwrap().is_none());

    std::fs::remove_file(&folder).unwrap();
    let _dumps = hasheous_with_worms(&folder, &fixture.server).await;
    let status = fixture.refresh(&status.failed).await;
    assert_eq!((status.updated, status.failed.len()), (1, 0));
    assert_eq!(fixture.igdb_id(&worms.id).await, Some(9331));
}

#[tokio::test]
async fn one_refresh_runs_at_a_time_and_reports_as_it_goes() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let mut updates = fixture.metadata.refresh_updates();
    fixture
        .metadata
        .start_refresh(vec![portal.id.clone()], RefreshMode::Missing)
        .unwrap();
    assert!(
        fixture
            .metadata
            .start_refresh(vec![portal.id.clone()], RefreshMode::Missing)
            .is_err()
    );
    loop {
        updates.changed().await.unwrap();
        let status = updates.borrow().clone().unwrap();
        if !status.running {
            assert_eq!(status.updated, 1);
            break;
        }
    }
    // Done, so another may start.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!fixture.metadata.cancel_refresh());
    fixture
        .metadata
        .start_refresh(vec![portal.id.clone()], RefreshMode::Missing)
        .unwrap();
}
