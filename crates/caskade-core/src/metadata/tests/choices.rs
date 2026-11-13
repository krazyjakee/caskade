//! What the user set: fields, artwork, matches by hand and sort titles.

use super::*;

#[tokio::test]
async fn fields_the_user_set_stay_and_reset_brings_igdbs_back() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let edit = GameEdit {
        description: Some(TextEdit::Set {
            value: String::new(),
        }),
        genres: Some(ListEdit::Set {
            values: vec!["Mine".into()],
        }),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&portal.id, &edit, None)
        .await
        .unwrap();

    fixture.metadata.run().await.unwrap();
    let state = fixture
        .store
        .game_edit_state(&portal.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.game.description, "");
    assert_eq!(state.game.genres, ["Mine"]);
    // What the user didn't set is IGDB's as before.
    assert_eq!(state.game.tags, ["Science fiction", "Single player"]);
    assert_eq!(state.automatic.description, "Portal 2 from IGDB.");
    assert_eq!(state.automatic.genres, ["Puzzle"]);

    let reset = GameEdit {
        description: Some(TextEdit::Reset),
        genres: Some(ListEdit::Reset),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&portal.id, &reset, None)
        .await
        .unwrap();
    let game = fixture.game(&portal.id).await;
    assert_eq!(game.description, "Portal 2 from IGDB.");
    assert_eq!(game.genres, ["Puzzle"]);
}

#[tokio::test]
async fn igdbs_cover_comes_beneath_one_the_user_chose() {
    let fixture = Fixture::new().await;
    let portal = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let mine = cache.import_bytes(png()).await.unwrap();
    fixture
        .store
        .set_user_artwork(&portal.id, ArtworkKind::Cover, &mine)
        .await
        .unwrap();
    fixture.metadata.run().await.unwrap();
    assert!(
        fixture
            .artwork(&portal.id)
            .await
            .contains(&(ArtworkKind::Cover, "igdb:co72".to_owned()))
    );
    let shown = |game: GameDetails| {
        game.artwork
            .into_iter()
            .filter(|art| art.kind == ArtworkKind::Cover)
            .map(|art| art.source)
            .collect::<Vec<_>>()
    };
    let details = fixture.store.get_game(&portal.id).await.unwrap().unwrap();
    assert_eq!(shown(details), ["user"]);
    fixture
        .store
        .reset_artwork(&portal.id, ArtworkKind::Cover)
        .await
        .unwrap();
    let details = fixture.store.get_game(&portal.id).await.unwrap().unwrap();
    assert_eq!(shown(details), ["igdb:co72"]);
}

#[tokio::test]
async fn a_match_by_hand_keeps_a_title_the_user_set() {
    let fixture = Fixture::new().await;
    let game = fixture.add("Portal II", vec![]).await;
    let edit = GameEdit {
        title: Some(TextEdit::Set {
            value: "My Portal".into(),
        }),
        ..Default::default()
    };
    fixture
        .store
        .edit_game(&game.id, &edit, None)
        .await
        .unwrap();
    fixture.metadata.set(&game.id, Some(72)).await.unwrap();
    let state = fixture
        .store
        .game_edit_state(&game.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.game.title, "My Portal");
    assert_eq!(state.automatic.title, "Portal 2");
}

#[tokio::test]
async fn igdbs_icon_joins_an_installers_and_the_library_shows_it() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    let cache = ArtworkCache::new(fixture._dir.path().join("artwork")).unwrap();
    let exe = cache.import_bytes(png()).await.unwrap();
    fixture
        .store
        .upsert_artwork(&exe.record(&game.id, ArtworkKind::Icon, "installer"))
        .await
        .unwrap();

    fixture.metadata.run().await.unwrap();
    let art = fixture.artwork(&game.id).await;
    assert!(art.contains(&(ArtworkKind::Icon, "installer".to_owned())));
    assert!(art.contains(&(ArtworkKind::Icon, "igdb:ic72".to_owned())));
    let page = fixture
        .store
        .list_games(&crate::model::ListGames::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].icon.as_ref().unwrap().source, "igdb:ic72");
}

#[tokio::test]
async fn a_match_by_hand_replaces_what_the_last_one_wrote() {
    let fixture = Fixture::new().await;
    let game = fixture
        .add("Portal 2", vec![ExternalId::Steam { app_id: 620 }])
        .await;
    fixture.metadata.run().await.unwrap();
    // The user edits the genres; IGDB's description stays IGDB's.
    let mut edited = fixture.game(&game.id).await;
    edited.genres = vec!["Mine".into()];
    fixture.store.upsert_game(&edited).await.unwrap();

    let set = fixture.metadata.set(&game.id, Some(1942)).await.unwrap();
    assert_eq!(
        (set.igdb_id, set.source, set.pending),
        (Some(1942), MatchSource::Manual, false)
    );
    let now = fixture.game(&game.id).await;
    // The game takes the chosen game's name, sort title too.
    assert_eq!(
        (now.title.as_str(), now.sort_title.as_str()),
        ("The Witcher 3", "The Witcher 3")
    );
    assert_eq!(now.description, "The Witcher 3 from IGDB.");
    assert_eq!(now.genres, ["Mine"]);
    let art = fixture.artwork(&game.id).await;
    assert!(art.contains(&(ArtworkKind::Cover, "igdb:co1942".to_owned())));
    assert!(!art.iter().any(|(_, source)| source.contains("72")));

    // No game: IGDB's values and artwork go, the user's stay.
    let none = fixture.metadata.set(&game.id, None).await.unwrap();
    assert_eq!((none.igdb_id, none.source), (None, MatchSource::Manual));
    let cleared = fixture.game(&game.id).await;
    assert!(cleared.description.is_empty());
    assert_eq!(cleared.release_date, None);
    assert!(cleared.aliases.is_empty());
    assert_eq!(cleared.genres, ["Mine"]);
    assert!(fixture.artwork(&game.id).await.is_empty());
    // Runs leave a choice made by hand alone, even one made mid-run.
    let auto = IgdbMatch::new(&game.id, Some(72), MatchSource::Store, "");
    assert!(!fixture.store.save_auto_igdb_match(&auto).await.unwrap());
    fixture.metadata.run().await.unwrap();
    assert_eq!(
        fixture
            .metadata
            .game(&game.id)
            .await
            .unwrap()
            .unwrap()
            .source,
        MatchSource::Manual
    );

    // Matching automatically again finds the store match.
    let again = fixture.metadata.rematch(&game.id).await.unwrap().unwrap();
    assert_eq!(
        (again.igdb_id, again.source),
        (Some(72), MatchSource::Store)
    );
    assert_eq!(
        fixture.game(&game.id).await.description,
        "Portal 2 from IGDB."
    );

    let error = fixture.metadata.set(&game.id, Some(5)).await.unwrap_err();
    assert!(error.to_string().contains("IGDB has no game 5"), "{error}");
}

#[tokio::test]
async fn a_match_by_hand_keeps_a_sort_title_of_the_users() {
    let fixture = Fixture::new().await;
    let game = fixture.add("witcher3", vec![]).await;
    let mut edited = fixture.game(&game.id).await;
    edited.sort_title = "Witcher 3".into();
    fixture.store.upsert_game(&edited).await.unwrap();

    fixture.metadata.set(&game.id, Some(1942)).await.unwrap();
    let now = fixture.game(&game.id).await;
    assert_eq!(
        (now.title.as_str(), now.sort_title.as_str()),
        ("The Witcher 3", "Witcher 3")
    );
}
