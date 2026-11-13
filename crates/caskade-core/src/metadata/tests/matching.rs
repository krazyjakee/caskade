//! Matching by store ids, Epic offers and Hasheous names, and what the game keeps.

use super::*;

#[tokio::test]
async fn store_ids_match_and_fill_the_game_in_once() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let unknown = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (report.matched, report.none, report.applied, report.deferred),
        (1, 1, 1, 0)
    );
    let game = fixture.game(&portal.id).await;
    assert_eq!(game.description, "Portal 2 from IGDB.");
    assert_eq!(game.genres, ["Puzzle"]);
    assert_eq!(game.tags, ["Science fiction", "Single player"]);
    assert_eq!(game.release_date.as_deref(), Some("2011-04-18"));
    // The title itself is no alias.
    assert_eq!(game.aliases, ["P2"]);
    // The library's images come now, the page's when it opens.
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Cover, "igdb:co72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic72".to_owned()),
        ]
    );
    assert!(fixture.metadata.page_images(&portal.id).await.unwrap());
    assert_eq!(
        fixture.artwork(&portal.id).await,
        [
            (ArtworkKind::Hero, "igdb:ar72".to_owned()),
            (ArtworkKind::Cover, "igdb:co72".to_owned()),
            (ArtworkKind::Icon, "igdb:ic72".to_owned()),
            (ArtworkKind::Screenshot, "igdb:sc72a".to_owned()),
            (ArtworkKind::Screenshot, "igdb:sc72b".to_owned()),
        ]
    );
    assert!(!fixture.metadata.page_images(&portal.id).await.unwrap());
    // Without a match there is nothing to fetch.
    assert!(!fixture.metadata.page_images(&unknown.id).await.unwrap());
    let found = fixture.metadata.game(&portal.id).await.unwrap().unwrap();
    assert_eq!(
        (found.igdb_id, found.source, found.pending),
        (Some(72), MatchSource::Store, false)
    );
    assert_eq!(found.game.unwrap().title, "Portal 2");
    let none = fixture.metadata.game(&unknown.id).await.unwrap().unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::None));
    assert!(fixture.game(&unknown.id).await.description.is_empty());

    // Nothing is left to do, so nothing is asked again.
    let requests = fixture.server.received_requests().await.unwrap().len();
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());
    assert_eq!(
        fixture.server.received_requests().await.unwrap().len(),
        requests
    );
}

#[tokio::test]
async fn epic_games_match_by_their_store_offer() {
    let fixture = Fixture::new().await;
    epic_catalogue(&fixture.server).await;
    let found = fixture.add("Satisfactory", epic("crab")).await;
    let gone = fixture.add("Gone", epic("gone")).await;
    let offline = fixture.add("Offline", epic("offline")).await;

    let report = fixture.metadata.run().await.unwrap();
    assert_eq!(
        (
            report.matched,
            report.none,
            report.deferred,
            report.unreachable
        ),
        (1, 1, 1, false)
    );
    let matched = fixture.metadata.game(&found.id).await.unwrap().unwrap();
    assert_eq!(
        (matched.igdb_id, matched.source),
        (Some(1942), MatchSource::Store)
    );
    let none = fixture.metadata.game(&gone.id).await.unwrap().unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::None));
    // Not asked about with every id it has, so it waits for the next run.
    assert!(
        fixture
            .store
            .igdb_match(&offline.id)
            .await
            .unwrap()
            .is_none()
    );

    fixture.metadata.run().await.unwrap();
    let offers =
        |namespace: &str| format!("/epic/catalog/catalog/api/shared/namespace/{namespace}/offers");
    assert_eq!(
        (
            fixture.requests(&offers("crab")).await,
            fixture.requests(&offers("gone")).await,
            fixture.requests(&offers("offline")).await,
            fixture
                .requests("/epic/account/account/api/oauth/token")
                .await,
        ),
        (1, 1, 2, 1)
    );
}

#[tokio::test]
async fn hasheous_names_come_before_store_ids() {
    let dir = tempfile::tempdir().unwrap();
    let server = server().await;
    let hasheous = hasheous_with_worms(&dir.path().join("hasheous"), &server).await;
    let fixture = Fixture::with_hasheous(dir, server, hasheous).await;
    // Steam 620 would be Portal 2; the name wins.
    let worms = fixture
        .add("Worms™", vec![ExternalId::Steam { app_id: 620 }])
        .await;

    fixture.metadata.run().await.unwrap();
    let found = fixture.metadata.game(&worms.id).await.unwrap().unwrap();
    assert_eq!(
        (found.igdb_id, found.source),
        (Some(9331), MatchSource::Hasheous)
    );
    assert_eq!(fixture.requests("/match").await, 0);
    assert_eq!(
        fixture.game(&worms.id).await.description,
        "Worms from IGDB."
    );
}

#[tokio::test]
async fn the_games_own_values_and_artwork_stay() {
    let fixture = Fixture::new().await;
    let mut game = Game::new("Portal 2");
    game.description = "From the launcher.".into();
    game.release_date = Some("2011-04-19".into());
    let game = fixture.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), "620");
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    fixture.store.upsert_entry(&entry).await.unwrap();
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let cover = cache.import_bytes(png()).await.unwrap();
    fixture
        .store
        .upsert_artwork(&cover.record(&game.id, ArtworkKind::Cover, "steam"))
        .await
        .unwrap();

    fixture.metadata.run().await.unwrap();
    let filled = fixture.game(&game.id).await;
    assert_eq!(filled.description, "From the launcher.");
    assert_eq!(filled.release_date.as_deref(), Some("2011-04-19"));
    assert_eq!(filled.genres, ["Puzzle"]);
    fixture.metadata.page_images(&game.id).await.unwrap();
    let art = fixture.artwork(&game.id).await;
    assert!(art.contains(&(ArtworkKind::Cover, "steam".to_owned())));
    assert!(!art.iter().any(|(_, source)| source == "igdb:co72"));
    assert!(art.contains(&(ArtworkKind::Hero, "igdb:ar72".to_owned())));
}
