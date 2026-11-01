//! The library's alphabet bar: where each initial letter starts, and
//! titles in the language's order (#218).
use super::LibraryStore;
use crate::model::{Game, GameFilter, ListGames, TitleLetter};

#[tokio::test]
async fn letters_point_at_their_first_game_in_title_order() {
    let store = LibraryStore::in_memory_in("en").await.unwrap();
    for (title, sort_title, favorite, hidden) in [
        ("Zork", "Zork", false, false),
        ("The Bravo", "Bravo", true, false),
        ("alpha", "alpha", false, false),
        ("Élan", "Élan", true, false),
        ("007", "007", false, false),
        ("Beta", "Beta", true, false),
        ("Aardvark", "Aardvark", false, true),
    ] {
        let mut game = Game::new(title);
        game.sort_title = sort_title.into();
        game.favorite = favorite;
        game.hidden = hidden;
        store.upsert_game(&game).await.unwrap();
    }
    let letters = |request: ListGames| {
        let store = store.clone();
        async move {
            store
                .title_letters(&request)
                .await
                .unwrap()
                .into_iter()
                .map(|TitleLetter { letter, offset }| (letter, offset))
                .collect::<Vec<_>>()
        }
    };
    let at = |letter: &str, offset| (letter.to_owned(), offset);
    let all = letters(ListGames::default()).await;
    assert_eq!(
        all,
        [at("#", 0), at("A", 1), at("B", 2), at("E", 4), at("Z", 5)],
        "case doesn't matter, the sort title counts, hidden games don't, and Élan is an E in English"
    );
    let titles = store
        .list_games(&ListGames::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|game| game.title)
        .collect::<Vec<_>>();
    assert_eq!(
        titles,
        ["007", "alpha", "Beta", "The Bravo", "Élan", "Zork"]
    );
    let favorites = ListGames {
        filter: GameFilter::Favorites,
        ..Default::default()
    };
    assert_eq!(letters(favorites).await, [at("B", 0), at("E", 2)]);
    let search = ListGames {
        query: Some("Zork".into()),
        ..Default::default()
    };
    assert_eq!(letters(search).await, [at("Z", 0)]);
}

async fn store_with(language: &str, titles: &[&str]) -> LibraryStore {
    let store = LibraryStore::in_memory_in(language).await.unwrap();
    for title in titles {
        store.upsert_game(&Game::new(*title)).await.unwrap();
    }
    store
}

async fn titles(store: &LibraryStore, request: ListGames) -> Vec<String> {
    let page = store.list_games(&request).await.unwrap();
    page.items.into_iter().map(|game| game.title).collect()
}

#[tokio::test]
async fn titles_sort_and_page_by_the_languages_alphabet() {
    let games = ["Zork", "Åsa", "Ärlig", "Öga", "Apa", "Élan"];
    let swedish = store_with("sv", &games).await;
    assert_eq!(
        titles(&swedish, ListGames::default()).await,
        ["Apa", "Élan", "Zork", "Åsa", "Ärlig", "Öga"],
        "Å, Ä and Ö come after Z in Swedish"
    );
    let letters = swedish
        .title_letters(&ListGames::default())
        .await
        .unwrap()
        .into_iter()
        .map(|TitleLetter { letter, offset }| (letter, offset))
        .collect::<Vec<_>>();
    let at = |letter: &str, offset| (letter.to_owned(), offset);
    assert_eq!(
        letters,
        [
            at("A", 0),
            at("E", 1),
            at("Z", 2),
            at("Å", 3),
            at("Ä", 4),
            at("Ö", 5)
        ],
        "letter paging steps through Å, Ä and Ö"
    );
    // A page boundary falls where the language's order puts it.
    let second = ListGames {
        offset: 3,
        limit: 2,
        ..Default::default()
    };
    assert_eq!(titles(&swedish, second).await, ["Åsa", "Ärlig"]);

    let german = store_with("de", &games).await;
    assert_eq!(
        titles(&german, ListGames::default()).await,
        ["Apa", "Ärlig", "Åsa", "Élan", "Öga", "Zork"],
        "Ä goes with A in German"
    );
}

#[tokio::test]
async fn search_folds_case_and_accents() {
    let store = store_with("en", &["Pokémon Snap", "Ōkami", "Élan Vital", "Zork"]).await;
    let found = |query: &str| {
        let store = store.clone();
        let request = ListGames {
            query: Some(query.into()),
            ..Default::default()
        };
        async move { titles(&store, request).await }
    };
    // Trigram words (three letters or more).
    assert_eq!(found("pokemon").await, ["Pokémon Snap"]);
    assert_eq!(found("POKÉMON").await, ["Pokémon Snap"]);
    assert_eq!(found("okami").await, ["Ōkami"]);
    // Shorter words.
    assert_eq!(found("é").await, ["Élan Vital", "Pokémon Snap"]);
    assert_eq!(found("ÖK").await, ["Ōkami", "Pokémon Snap"]);
    assert_eq!(found("el vi").await, ["Élan Vital"]);
    assert!(found("ü").await.is_empty());
}
