//! Web pages without artwork can still identify their game by exact title.

use super::*;

impl Fixture {
    async fn web_game(&self, title: &str) -> Game {
        let game = self.store.upsert_game(&Game::new(title)).await.unwrap();
        let url = format!("https://games.example/{}", game.id);
        let mut entry = GameEntry::new(&game.id, crate::web::WEB_SOURCE, &url);
        entry.launch = Some(crate::model::LaunchSpec::Web { url });
        self.store.upsert_entry(&entry).await.unwrap();
        game
    }

    async fn title_results(&self, games: Vec<Value>) {
        Mock::given(method("POST"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "games": games, "notFound": [],
            })))
            .mount(&self.server)
            .await;
    }
}

#[tokio::test]
async fn a_web_title_gets_igdb_artwork_and_keeps_user_choices() {
    let fixture = Fixture::new().await;
    let game = fixture.web_game("Angry Birds 2").await;
    fixture
        .title_results(vec![igdb_game(&fixture.server.uri(), 72, "Angry Birds 2")])
        .await;
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.matched, report.applied), (1, 1));
    let row = fixture.store.igdb_match(&game.id).await.unwrap().unwrap();
    assert_eq!((row.igdb_id, row.source), (Some(72), MatchSource::Title));
    assert!(
        fixture
            .artwork(&game.id)
            .await
            .contains(&(ArtworkKind::Cover, "igdb:co72".into()))
    );
    assert_eq!(fixture.requests("/search").await, 1);
    assert_eq!(
        fixture.requests("/games").await,
        0,
        "search cached the game"
    );
    // A second run uses the match. Refresh preserves an intentional removal
    // and a description the player wrote.
    fixture.metadata.run().await.unwrap();
    fixture
        .store
        .remove_artwork(&game.id, ArtworkKind::Cover)
        .await
        .unwrap();
    fixture
        .store
        .edit_game(
            &game.id,
            &GameEdit {
                description: Some(TextEdit::Set {
                    value: "Mine".into(),
                }),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let status = fixture
        .metadata
        .refresh_games(
            std::slice::from_ref(&game.id),
            RefreshMode::Missing,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    assert!(status.failed.is_empty());
    assert_eq!(fixture.game(&game.id).await.description, "Mine");
    let slots = fixture
        .store
        .artwork_slots(&game.id)
        .await
        .unwrap()
        .unwrap();
    let cover = slots
        .iter()
        .find(|slot| slot.kind == ArtworkKind::Cover)
        .unwrap();
    assert_eq!(cover.choice, crate::model::ArtworkChoice::Removed);
    assert_eq!(fixture.requests("/search").await, 1);
}

#[tokio::test]
async fn title_matching_rejects_fuzzy_ambiguous_and_truncated_results() {
    for titles in [
        vec![],
        vec!["Angry Birds"],
        vec!["Angry Birds 2", "Angry Birds 2"],
        vec!["Angry Birds 2"; 20],
    ] {
        let fixture = Fixture::new().await;
        let game = fixture.web_game("Angry Birds 2").await;
        fixture
            .title_results(
                titles
                    .iter()
                    .enumerate()
                    .map(|(i, title)| igdb_game(&fixture.server.uri(), 100 + i as u64, title))
                    .collect(),
            )
            .await;
        let report = fixture.metadata.run().await.unwrap();
        assert_eq!((report.matched, report.none), (0, 1), "{titles:?}");
        assert!(fixture.artwork(&game.id).await.is_empty());
        fixture.metadata.run().await.unwrap();
        assert_eq!(
            fixture.requests("/search").await,
            1,
            "negative result waits a week"
        );
    }
}

#[tokio::test]
async fn title_matching_accepts_only_the_exact_title_among_similar_games() {
    let fixture = Fixture::new().await;
    let game = fixture.web_game("Angry  Birds™ 2").await;
    fixture
        .title_results(vec![
            igdb_game(&fixture.server.uri(), 1942, "Angry Birds"),
            igdb_game(&fixture.server.uri(), 72, "angry birds 2"),
        ])
        .await;
    fixture.metadata.run().await.unwrap();
    assert_eq!(
        fixture
            .store
            .igdb_match(&game.id)
            .await
            .unwrap()
            .unwrap()
            .igdb_id,
        Some(72)
    );
}

#[tokio::test]
async fn title_search_respects_manual_choices_and_store_id_precedence() {
    let fixture = Fixture::new().await;
    let manual = fixture.web_game("Manual web game").await;
    fixture.metadata.set(&manual.id, None).await.unwrap();
    let store = fixture.web_game("Store web game").await;
    let mut entry = fixture
        .store
        .entries_for_game(&store.id)
        .await
        .unwrap()
        .remove(0);
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    fixture.store.upsert_entry(&entry).await.unwrap();
    fixture.add("Unknown desktop", Vec::new()).await;
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/search").await, 0);
    let matched = fixture.store.igdb_match(&store.id).await.unwrap().unwrap();
    assert_eq!(
        (matched.igdb_id, matched.source),
        (Some(72), MatchSource::Store)
    );
    assert_eq!(
        fixture
            .store
            .igdb_match(&manual.id)
            .await
            .unwrap()
            .unwrap()
            .igdb_id,
        None
    );
}

#[tokio::test]
async fn a_failed_title_search_stops_and_leaves_games_due_for_retry() {
    let fixture = Fixture::new().await;
    let first = fixture.web_game("A web game").await;
    let second = fixture.web_game("B web game").await;
    Mock::given(method("POST"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    fixture.title_results(Vec::new()).await;
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.deferred, report.none, report.unreachable),
        (2, 0, true)
    );
    for game in [&first, &second] {
        assert!(fixture.store.igdb_match(&game.id).await.unwrap().is_none());
    }
    fixture.metadata.run().await.unwrap();
    assert_eq!(fixture.requests("/search").await, 1, "cooldown holds");
    fixture.metadata.inner.igdb.unpause();
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(report.none, 2);
    assert_eq!(fixture.requests("/search").await, 3);
}

#[tokio::test]
async fn the_title_migration_retries_only_automatic_unmatched_web_games() {
    let fixture = Fixture::new().await;
    let web = fixture.web_game("Web game").await;
    let manual = fixture.web_game("Manual choice").await;
    let desktop = fixture.add("Desktop game", Vec::new()).await;
    for game in [&web, &desktop] {
        let mut row = IgdbMatch::new(&game.id, None, MatchSource::None, "");
        row.pending = false;
        fixture.store.save_igdb_match(&row).await.unwrap();
    }
    fixture.metadata.set(&manual.id, None).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0039_igdb_title.down.sql"))
        .execute(&fixture.store.pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0039_igdb_title.up.sql"))
        .execute(&fixture.store.pool)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .igdb_match(&web.id)
            .await
            .unwrap()
            .unwrap()
            .checked_at_ms,
        0
    );
    for game in [&manual, &desktop] {
        assert!(
            fixture
                .store
                .igdb_match(&game.id)
                .await
                .unwrap()
                .unwrap()
                .checked_at_ms
                > 0
        );
    }
    assert_eq!(
        fixture
            .store
            .igdb_pending(None, retry_none_before())
            .await
            .unwrap(),
        vec![web.id],
    );
}

#[tokio::test]
#[ignore = "needs the Angry Birds web page, metadata server and IGDB images"]
async fn live_artwork_for_a_web_page_without_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let artwork = ArtworkCache::new(dir.path().join("artwork")).unwrap();
    let id = crate::web::WebGames::new(store.clone())
        .with_artwork(artwork.clone())
        .add("https://naotomori1.github.io/AngryBirds2Port/")
        .await
        .unwrap();
    let metadata = Metadata::new(
        store.clone(),
        Bus::default(),
        Igdb::new(dir.path().join("igdb")),
        Hasheous::new(dir.path().join("hasheous")),
        Arc::new(Catalog::default()),
        Some(artwork),
    );
    let report = metadata.run().await.unwrap();
    assert!(!report.unreachable, "{report:?}");
    let matched = store.igdb_match(&id).await.unwrap().unwrap();
    assert_eq!(
        (matched.igdb_id, matched.source),
        (Some(19273), MatchSource::Title)
    );
    assert!(
        store
            .artwork_for_game(&id)
            .await
            .unwrap()
            .iter()
            .any(|art| art.kind == ArtworkKind::Cover)
    );
}
