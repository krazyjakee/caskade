//! Age ratings.

use super::*;

#[tokio::test]
async fn age_ratings_apply_and_a_game_without_any_is_unrated() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let witcher = fixture
        .add("The Witcher 3", vec![ExternalId::Gog { product_id: 1 }])
        .await;
    let unknown = fixture
        .add("Unknown", vec![ExternalId::Steam { app_id: 9 }])
        .await;
    fixture.metadata.run().await.unwrap();

    let rated = fixture
        .store
        .game_age_ratings(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rated.ratings,
        [rating("pegi", "12"), rating("esrb", "E10+")]
    );
    assert_eq!(rated.min_age, Some(12));
    // Matched without ratings, or not matched: unrated, not 0.
    for id in [&witcher.id, &unknown.id] {
        let unrated = fixture.store.game_age_ratings(id).await.unwrap().unwrap();
        assert_eq!(unrated, crate::db::GameAgeRatings::default());
    }

    // Ratings of the game's own stay, like its other fields.
    let own = [rating("usk", "18")];
    fixture
        .store
        .set_game_age_ratings(&portal.id, &own)
        .await
        .unwrap();
    fixture.metadata.refresh(&portal.id).await.unwrap();
    let kept = fixture
        .store
        .game_age_ratings(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (kept.ratings.as_slice(), kept.min_age),
        (own.as_slice(), Some(18))
    );
}

#[tokio::test]
async fn games_matched_before_ratings_get_them_once_without_a_rematch() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    // As a library from before ratings, after migration 0015: no ratings,
    // the match pending again, and the cache's answer from before them.
    fixture
        .store
        .set_game_age_ratings(&portal.id, &[])
        .await
        .unwrap();
    let mut row = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    row.pending = true;
    row.applied.age_ratings = None;
    fixture.store.save_igdb_match(&row).await.unwrap();
    let base = fixture.server.uri();
    let mut old = igdb_game(&base, 72, "Portal 2");
    old.as_object_mut().unwrap().remove("ageRatings");
    fixture.metadata.inner.igdb.cache_outdated(72, &old).await;
    let matches = fixture.requests("/match").await;
    let games = fixture.requests("/games").await;

    // Offline, the game keeps what it has and waits.
    Mock::given(method("POST"))
        .and(path("/games"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&fixture.server)
        .await;
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.deferred, report.unreachable), (1, true));
    let waiting = fixture.store.igdb_match(&portal.id).await.unwrap().unwrap();
    assert!(waiting.pending);
    assert_eq!(
        fixture.game(&portal.id).await.description,
        "Portal 2 from IGDB."
    );

    // Back online, the game is fetched again once and gets its ratings.
    fixture.metadata.inner.igdb.unpause();
    let report = fixture.metadata.run().await.unwrap();
    assert_eq!((report.applied, report.deferred), (1, 0));
    let rated = fixture
        .store
        .game_age_ratings(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rated.min_age, Some(12));
    assert_eq!(fixture.requests("/games").await, games + 2);
    assert_eq!(fixture.requests("/match").await, matches);
    assert_eq!(fixture.metadata.run().await.unwrap(), RunReport::default());
    assert_eq!(fixture.requests("/games").await, games + 2);
}

#[tokio::test]
async fn a_merged_game_takes_the_highest_rating_of_its_releases() {
    let fixture = Fixture::new().await;
    let keep = fixture.add("Portal 2", vec![]).await;
    let donor = fixture.add("Portal 2 (Japan)", vec![]).await;
    fixture
        .store
        .set_game_age_ratings(&keep.id, &[rating("pegi", "12")])
        .await
        .unwrap();
    fixture
        .store
        .set_game_age_ratings(&donor.id, &[rating("cero", "C"), rating("pegi", "12")])
        .await
        .unwrap();
    fixture
        .store
        .merge_games(&keep.id, &donor.id)
        .await
        .unwrap();
    let merged = fixture
        .store
        .game_age_ratings(&keep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merged.ratings, [rating("pegi", "12"), rating("cero", "C")]);
    assert_eq!(merged.min_age, Some(15));
}
