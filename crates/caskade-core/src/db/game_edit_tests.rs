//! Manual game edits (#171): validated patches, conflicts, and the user's
//! fields kept through automatic writes, imports, merges and separation.
use super::LibraryStore;
use crate::model::*;

async fn import(store: &LibraryStore, game: &Game, source: &'static str, key: &str) -> String {
    let entry = GameEntry::new(&game.id, SourceId::new(source), key);
    store
        .import_group(game, &[entry], None)
        .await
        .unwrap()
        .game_id
}

async fn portal(store: &LibraryStore) -> Game {
    let mut game = Game::new("Portal 2");
    game.description = "From the launcher.".into();
    game.genres = vec!["Puzzle".into()];
    game.release_date = Some("2011-04-19".into());
    let id = import(store, &game, "steam", "620").await;
    store.game(&id).await.unwrap().unwrap()
}

fn set_text(value: &str) -> Option<TextEdit> {
    Some(TextEdit::Set {
        value: value.into(),
    })
}

fn set_list(values: &[&str]) -> Option<ListEdit> {
    Some(ListEdit::Set {
        values: values.iter().map(|value| (*value).to_owned()).collect(),
    })
}

fn saved(outcome: GameEditOutcome) -> (GameEditState, Vec<GameField>) {
    match outcome {
        GameEditOutcome::Saved { state, changed } => (state, changed),
        GameEditOutcome::Rejected { errors, .. } => panic!("rejected: {errors:?}"),
    }
}

fn rejected(outcome: GameEditOutcome) -> Vec<(GameField, FieldProblem)> {
    match outcome {
        GameEditOutcome::Rejected { errors, .. } => errors
            .into_iter()
            .map(|error| (error.field, error.problem))
            .collect(),
        GameEditOutcome::Saved { .. } => panic!("saved"),
    }
}

async fn search(store: &LibraryStore, query: &str) -> Vec<String> {
    store
        .list_games(&ListGames {
            query: Some(query.into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|game| game.title)
        .collect()
}

#[tokio::test]
async fn an_edit_changes_only_its_fields_and_the_library_shows_them() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let edit = GameEdit {
        title: set_text("  Portal Two  "),
        tags: set_list(&["co-op", " ", "co-op", "Puzzle "]),
        rating: Some(RatingEdit { value: Some(90) }),
        notes: Some(NotesEdit {
            value: "Finish the co-op campaign.".into(),
        }),
        ..Default::default()
    };
    let (state, changed) = saved(store.edit_game(&game.id, &edit, Some(&game)).await.unwrap());
    assert_eq!(
        changed,
        [
            GameField::Title,
            GameField::Tags,
            GameField::Rating,
            GameField::Notes
        ]
    );
    assert_eq!(state.game.title, "Portal Two");
    assert_eq!(state.game.tags, ["co-op", "Puzzle"]);
    assert_eq!(state.game.rating, Some(90));
    assert_eq!(state.game.description, "From the launcher.");
    // Only fields automatic sources fill are the user's; rating and notes
    // always are.
    assert_eq!(state.manual, [GameField::Title, GameField::Tags]);
    assert_eq!(state.automatic.title, "Portal 2");
    assert!(state.automatic.tags.is_empty());

    let stored = store.game(&game.id).await.unwrap().unwrap();
    assert_eq!(stored, state.game);
    assert_eq!(search(&store, "two").await, ["Portal Two"]);
    let summary = store
        .list_games(&ListGames::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(
        (summary.title, summary.rating, summary.tags),
        ("Portal Two".to_owned(), Some(90), stored.tags)
    );
    assert_eq!(
        store.game_edit_state(&game.id).await.unwrap().unwrap(),
        state
    );
}

#[tokio::test]
async fn an_invalid_edit_saves_nothing_and_names_each_field() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let edit = GameEdit {
        title: set_text("   "),
        description: set_text("Fine."),
        release_date: Some(DateEdit::Set {
            value: Some("2011-02-30".into()),
        }),
        rating: Some(RatingEdit { value: Some(101) }),
        ..Default::default()
    };
    let errors = rejected(store.edit_game(&game.id, &edit, None).await.unwrap());
    assert_eq!(
        errors,
        [
            (GameField::Rating, FieldProblem::Invalid),
            (GameField::Title, FieldProblem::Invalid),
            (GameField::ReleaseDate, FieldProblem::Invalid),
        ]
    );
    let state = store.game_edit_state(&game.id).await.unwrap().unwrap();
    assert_eq!(state.game, game);
    assert!(state.manual.is_empty());

    let negative = GameEdit {
        rating: Some(RatingEdit { value: Some(-1) }),
        ..Default::default()
    };
    assert_eq!(
        rejected(store.edit_game(&game.id, &negative, None).await.unwrap()),
        [(GameField::Rating, FieldProblem::Invalid)]
    );
    assert!(
        store
            .edit_game("nope", &GameEdit::default(), None)
            .await
            .is_err()
    );
}

#[test]
fn unknown_patch_keys_fail_the_whole_patch() {
    let edit = serde_json::json!({ "title": { "op": "set", "value": "X" }, "completion": "done" });
    assert!(serde_json::from_value::<GameEdit>(edit).is_err());
    let edit = serde_json::json!({ "title": { "op": "set", "value": "X", "extra": 1 } });
    assert!(serde_json::from_value::<GameEdit>(edit).is_err());
    let edit = serde_json::json!({ "aliases": { "op": "add", "values": ["X"] } });
    assert!(serde_json::from_value::<GameEdit>(edit).is_ok());
}

#[tokio::test]
async fn a_stale_editor_conflicts_only_on_fields_that_changed_under_it() {
    let store = LibraryStore::in_memory().await.unwrap();
    let opened = portal(&store).await;
    // A refresh meanwhile changes the description and the genres.
    store
        .update_automatic_game(&opened.id, |game| {
            game.description = "From IGDB.".into();
            game.genres = vec!["Puzzle".into(), "Platform".into()];
        })
        .await
        .unwrap();

    let edit = GameEdit {
        description: set_text("My own."),
        title: set_text("Portal Two"),
        ..Default::default()
    };
    assert_eq!(
        rejected(
            store
                .edit_game(&opened.id, &edit, Some(&opened))
                .await
                .unwrap()
        ),
        [(GameField::Description, FieldProblem::Conflict)]
    );
    assert_eq!(
        store.game(&opened.id).await.unwrap().unwrap().title,
        "Portal 2"
    );
    // A field the editor didn't touch is the refresh's.
    let edit = GameEdit {
        title: set_text("Portal Two"),
        ..Default::default()
    };
    let (state, _) = saved(
        store
            .edit_game(&opened.id, &edit, Some(&opened))
            .await
            .unwrap(),
    );
    assert_eq!(state.game.description, "From IGDB.");
    assert_eq!(state.game.genres, ["Puzzle", "Platform"]);
    // The same value arriving meanwhile is no conflict, and adding to a
    // list never is.
    let edit = GameEdit {
        title: set_text("Portal Two"),
        genres: Some(ListEdit::Add {
            values: vec!["Co-op".into()],
        }),
        ..Default::default()
    };
    let (state, changed) = saved(
        store
            .edit_game(&opened.id, &edit, Some(&opened))
            .await
            .unwrap(),
    );
    assert_eq!(changed, [GameField::Genres]);
    assert_eq!(state.game.genres, ["Puzzle", "Platform", "Co-op"]);
}

#[tokio::test]
async fn automatic_writes_go_beneath_the_users_fields_and_reset_brings_them_back() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let edit = GameEdit {
        description: set_text(""),
        genres: set_list(&[]),
        release_date: Some(DateEdit::Set { value: None }),
        ..Default::default()
    };
    saved(store.edit_game(&game.id, &edit, Some(&game)).await.unwrap());

    // A refresh through the automatic layer.
    let (_, shown_changed) = store
        .update_automatic_game(&game.id, |game| {
            game.description = "From IGDB.".into();
            game.genres = vec!["Platform".into()];
            game.tags = vec!["Science fiction".into()];
        })
        .await
        .unwrap()
        .unwrap();
    assert!(shown_changed);
    // And a writer that saves a whole game it read (an install's update).
    let mut whole = store.game(&game.id).await.unwrap().unwrap();
    whole.release_date = Some("2011-04-18".into());
    whole.title = "Portal 2 (Update)".into();
    let returned = store.upsert_game(&whole).await.unwrap();
    assert_eq!(returned.release_date, None);

    let state = store.game_edit_state(&game.id).await.unwrap().unwrap();
    // The user's clears stay; the rest follows.
    assert_eq!(state.game.description, "");
    assert!(state.game.genres.is_empty());
    assert_eq!(state.game.release_date, None);
    assert_eq!(state.game.tags, ["Science fiction"]);
    assert_eq!(state.game.title, "Portal 2 (Update)");
    assert_eq!(state.automatic.description, "From IGDB.");
    assert_eq!(state.automatic.genres, ["Platform"]);
    assert_eq!(state.automatic.release_date.as_deref(), Some("2011-04-18"));

    let reset = GameEdit {
        description: Some(TextEdit::Reset),
        release_date: Some(DateEdit::Reset),
        ..Default::default()
    };
    let (state, changed) = saved(store.edit_game(&game.id, &reset, None).await.unwrap());
    assert_eq!(changed, [GameField::Description, GameField::ReleaseDate]);
    assert_eq!(state.game.description, "From IGDB.");
    assert_eq!(state.game.release_date.as_deref(), Some("2011-04-18"));
    assert!(state.game.genres.is_empty());
    assert_eq!(state.manual, [GameField::Genres]);
    // Reset fields follow automatic writes again.
    store
        .update_automatic_game(&game.id, |game| game.description = "Newer.".into())
        .await
        .unwrap();
    assert_eq!(
        store.game(&game.id).await.unwrap().unwrap().description,
        "Newer."
    );
}

#[tokio::test]
async fn a_value_set_to_its_automatic_value_is_still_the_users() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let edit = GameEdit {
        description: set_text("From the launcher."),
        ..Default::default()
    };
    let (state, changed) = saved(store.edit_game(&game.id, &edit, None).await.unwrap());
    assert_eq!(changed, [GameField::Description]);
    assert_eq!(state.manual, [GameField::Description]);
    store
        .update_automatic_game(&game.id, |game| game.description = "From IGDB.".into())
        .await
        .unwrap();
    assert_eq!(
        store.game(&game.id).await.unwrap().unwrap().description,
        "From the launcher."
    );
    // Saving it again changes nothing.
    let (_, changed) = saved(store.edit_game(&game.id, &edit, None).await.unwrap());
    assert!(changed.is_empty());
}

#[tokio::test]
async fn a_rescan_keeps_the_users_fields() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let edit = GameEdit {
        title: set_text("Portal Two"),
        notes: Some(NotesEdit {
            value: "Keep.".into(),
        }),
        ..Default::default()
    };
    saved(store.edit_game(&game.id, &edit, None).await.unwrap());
    let mut proposed = Game::new("Portal 2");
    proposed.description = "Rescanned.".into();
    let id = import(&store, &proposed, "steam", "620").await;
    assert_eq!(id, game.id);
    let stored = store.game(&id).await.unwrap().unwrap();
    assert_eq!(
        (stored.title.as_str(), stored.notes.as_str()),
        ("Portal Two", "Keep.")
    );
}

#[tokio::test]
async fn a_merge_keeps_both_games_fields_and_notes() {
    let store = LibraryStore::in_memory().await.unwrap();
    let keep = portal(&store).await;
    let mut other = Game::new("Portal 2 (GOG)");
    other.description = "From GOG.".into();
    other.notes = "GOG notes.".into();
    let other_id = import(&store, &other, "gog", "portal2").await;
    let other = store.game(&other_id).await.unwrap().unwrap();

    let edit = GameEdit {
        title: set_text("Portal 2: Mine"),
        tags: set_list(&["Mine"]),
        ..Default::default()
    };
    saved(store.edit_game(&keep.id, &edit, None).await.unwrap());
    let edit = GameEdit {
        title: set_text("GOG Mine"),
        description: set_text("The donor's own."),
        notes: Some(NotesEdit {
            value: "Donor notes.".into(),
        }),
        ..Default::default()
    };
    saved(store.edit_game(&other.id, &edit, None).await.unwrap());
    let keep_notes = GameEdit {
        notes: Some(NotesEdit {
            value: "Kept notes.".into(),
        }),
        ..Default::default()
    };
    saved(store.edit_game(&keep.id, &keep_notes, None).await.unwrap());

    store.merge_games(&keep.id, &other.id).await.unwrap();
    let state = store.game_edit_state(&keep.id).await.unwrap().unwrap();
    assert_eq!(state.game.title, "Portal 2: Mine");
    assert!(state.game.aliases.contains(&"GOG Mine".to_owned()));
    assert_eq!(state.game.description, "The donor's own.");
    assert_eq!(state.game.tags, ["Mine"]);
    assert_eq!(state.game.notes, "Kept notes.\n\nGOG Mine:\nDonor notes.");
    // The kept name is the user's; the donor's title isn't carried as one.
    assert_eq!(
        state.manual,
        [GameField::Title, GameField::Description, GameField::Tags]
    );
    assert_eq!(state.automatic.title, "Portal 2");
    assert_eq!(state.automatic.description, "From the launcher.");
    assert!(state.game.aliases.contains(&"Portal 2 (GOG)".to_owned()));
}

#[tokio::test]
async fn separating_an_entry_keeps_the_users_fields_on_both_games() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let entry = GameEntry::new(&game.id, SourceId::new("gog"), "portal2");
    let entry = store.upsert_entry(&entry).await.unwrap();
    let edit = GameEdit {
        title: set_text("Portal Two"),
        platforms: set_list(&["Windows", "Linux"]),
        ..Default::default()
    };
    saved(store.edit_game(&game.id, &edit, None).await.unwrap());

    let apart = store
        .separate_entry(&entry.id, &["Linux".into()], &["Windows".into()])
        .await
        .unwrap();
    let apart = store.game_edit_state(&apart.id).await.unwrap().unwrap();
    let left = store.game_edit_state(&game.id).await.unwrap().unwrap();
    assert_eq!(apart.game.title, "Portal Two");
    assert_eq!(apart.game.platforms, ["Linux"]);
    assert_eq!(apart.automatic.platforms, ["Linux"]);
    assert_eq!(apart.manual, [GameField::Title, GameField::Platforms]);
    assert_eq!(left.game.platforms, ["Windows"]);
    assert_eq!(left.manual, [GameField::Title, GameField::Platforms]);
}

#[tokio::test]
async fn hiding_and_favouriting_leave_the_users_fields_alone() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = portal(&store).await;
    let edit = GameEdit {
        title: set_text("Portal Two"),
        ..Default::default()
    };
    saved(store.edit_game(&game.id, &edit, None).await.unwrap());
    store.set_game_favorite(&game.id, true).await.unwrap();
    store.set_game_hidden(&game.id, true).await.unwrap();
    let state = store.game_edit_state(&game.id).await.unwrap().unwrap();
    assert_eq!(state.automatic.title, "Portal 2");
    assert!(state.game.favorite && state.game.hidden);
}

#[tokio::test]
async fn a_bulk_edit_previews_then_applies_only_its_fields() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut ids = Vec::new();
    for (title, tags, description) in [
        ("Alpha", vec!["Puzzle"], "A"),
        ("Beta", vec!["Co-op", "Racing"], "B"),
        ("Gamma", vec![], "C"),
    ] {
        let mut game = Game::new(title);
        game.tags = tags.into_iter().map(String::from).collect();
        game.description = description.into();
        ids.push(store.upsert_game(&game).await.unwrap().id);
    }
    let edit = GameEdit {
        tags: Some(ListEdit::Add {
            values: vec!["Co-op".into()],
        }),
        rating: Some(RatingEdit { value: Some(70) }),
        ..Default::default()
    };
    let mut targets = ids.clone();
    targets.push("gone".into());
    let preview = store.bulk_edit_games(&targets, &edit, false).await.unwrap();
    let statuses: Vec<_> = preview.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [
            BulkStatus::Changed,
            BulkStatus::Changed,
            BulkStatus::Changed,
            BulkStatus::Missing
        ]
    );
    assert_eq!(
        preview[0].changes,
        [
            BulkChange {
                field: GameField::Tags,
                before: "Puzzle".into(),
                after: "Puzzle, Co-op".into()
            },
            BulkChange {
                field: GameField::Rating,
                before: String::new(),
                after: "70".into()
            },
        ]
    );
    // Beta already has the tag: only its rating changes.
    assert_eq!(preview[1].changes.len(), 1);
    // A preview saves nothing.
    assert!(store.game(&ids[0]).await.unwrap().unwrap().rating.is_none());

    let applied = store.bulk_edit_games(&targets, &edit, true).await.unwrap();
    assert_eq!(applied[..3], preview[..3]);
    let games: Vec<Game> = futures_join(&store, &ids).await;
    assert_eq!(games[0].tags, ["Puzzle", "Co-op"]);
    assert_eq!(games[1].tags, ["Co-op", "Racing"]);
    // Adding a tag Beta had leaves its tags to automatic sources.
    let beta = store.game_edit_state(&ids[1]).await.unwrap().unwrap();
    assert!(!beta.manual.contains(&GameField::Tags));
    assert_eq!(games[2].tags, ["Co-op"]);
    // Mixed values stay mixed.
    assert_eq!(
        games
            .iter()
            .map(|g| g.description.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert!(games.iter().all(|g| g.rating == Some(70)));
    // A retry has nothing left to do.
    let again = store.bulk_edit_games(&ids, &edit, true).await.unwrap();
    assert!(again.iter().all(|r| r.status == BulkStatus::Unchanged));

    let invalid = GameEdit {
        release_date: Some(DateEdit::Set {
            value: Some("someday".into()),
        }),
        ..Default::default()
    };
    let rejected = store.bulk_edit_games(&ids, &invalid, true).await.unwrap();
    assert!(
        rejected.iter().all(
            |r| r.status == BulkStatus::Rejected && r.errors[0].field == GameField::ReleaseDate
        )
    );
}

async fn futures_join(store: &LibraryStore, ids: &[String]) -> Vec<Game> {
    let mut games = Vec::new();
    for id in ids {
        games.push(store.game(id).await.unwrap().unwrap());
    }
    games
}

#[tokio::test]
async fn select_all_lists_every_game_a_query_finds() {
    let store = LibraryStore::in_memory().await.unwrap();
    let games: Vec<Game> = (0..1205)
        .map(|i| Game::new(format!("Game {i:04}")))
        .collect();
    store.upsert_games(&games).await.unwrap();
    let all = store.game_ids(&ListGames::default()).await.unwrap();
    assert_eq!(all.len(), 1205);
    // A search: the same games, in the same order, as its pages.
    let search = ListGames {
        query: Some("Game 120".into()),
        limit: 1000,
        ..Default::default()
    };
    let page = store.list_games(&search).await.unwrap();
    let found = store.game_ids(&search).await.unwrap();
    assert!(!found.is_empty() && found.len() < 1205);
    assert_eq!(
        found,
        page.items.into_iter().map(|game| game.id).collect::<Vec<_>>()
    );
}
