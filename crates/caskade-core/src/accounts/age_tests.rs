//! Age limits on play (#209) over a private database: who is held back,
//! and approvals used once, for one game and one account.
use super::super::{
    AccountAccess, AccountRole, AccountService, DEFAULT_AVATAR, NewAccount, Permissions, PinCheck,
};
use crate::{
    bus::Bus,
    db::LibraryStore,
    igdb::IgdbAgeRating,
    launch::{LaunchError, PlayGate},
    model::Game,
};

const ADMIN_PIN: &str = "2468";

struct Setup {
    accounts: AccountService,
    admin: String,
    /// Limited to 12; unrated games play.
    kid: String,
    /// Limited to 12; unrated games need a PIN.
    strict: String,
    /// Rated PEGI 18.
    adult: String,
    /// Rated PEGI 7.
    family: String,
    unrated: String,
}

async fn game(store: &LibraryStore, title: &str, pegi: Option<&str>) -> String {
    let game = Game::new(title);
    store.upsert_game(&game).await.unwrap();
    let ratings: Vec<_> = pegi
        .map(|rating| IgdbAgeRating {
            board: "pegi".into(),
            rating: rating.into(),
        })
        .into_iter()
        .collect();
    store
        .set_game_age_ratings(&game.id, &ratings)
        .await
        .unwrap();
    game.id
}

async fn setup() -> Setup {
    setup_store(LibraryStore::in_memory().await.unwrap()).await
}

async fn setup_store(store: LibraryStore) -> Setup {
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let admin = accounts.active_id().unwrap();
    accounts
        .set_pin(&admin, Some(ADMIN_PIN.into()))
        .await
        .unwrap();
    let limited = |name: &str, unrated_needs_pin| NewAccount {
        name: name.into(),
        avatar: DEFAULT_AVATAR.into(),
        access: AccountAccess {
            role: AccountRole::Standard,
            permissions: Permissions::default(),
            max_age: Some(12),
            unrated_needs_pin,
        },
        pin: None,
    };
    let kid = accounts.create(limited("Kid", false)).await.unwrap().id;
    let strict = accounts.create(limited("Strict", true)).await.unwrap().id;
    Setup {
        adult: game(&store, "Grown-up Game", Some("18")).await,
        family: game(&store, "Family Game", Some("7")).await,
        unrated: game(&store, "Homebrew", None).await,
        accounts,
        admin,
        kid,
        strict,
    }
}

fn restricted(result: crate::launch::Result<()>) -> bool {
    matches!(result, Err(LaunchError::AgeRestricted(_)))
}

#[tokio::test]
async fn a_game_over_the_limit_needs_an_approval_and_one_under_it_plays() {
    let s = setup().await;
    s.accounts.sign_in(&s.kid, None).await.unwrap();
    s.accounts.admit(&s.family).await.unwrap();
    let error = s.accounts.admit(&s.adult).await.unwrap_err();
    let LaunchError::AgeRestricted(restriction) = &error else {
        panic!("{error}");
    };
    assert_eq!(restriction.title, "Grown-up Game");
    assert_eq!(restriction.min_age, Some(18));
    assert_eq!(restriction.max_age, 12);
    assert_eq!(
        error.to_string(),
        "an admin PIN is needed to play Grown-up Game, rated PEGI 18"
    );
    // The read the UI shows agrees.
    assert!(
        s.accounts
            .age_restriction(&s.adult)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        s.accounts
            .age_restriction(&s.family)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn an_approval_is_used_once_for_its_game() {
    let s = setup().await;
    s.accounts.sign_in(&s.kid, None).await.unwrap();
    // A wrong PIN approves nothing.
    assert_eq!(
        s.accounts
            .approve_play(&s.adult, "1111".into())
            .await
            .unwrap(),
        PinCheck::Wrong
    );
    assert!(restricted(s.accounts.admit(&s.adult).await));
    // An approval for another game doesn't cover this one.
    assert_eq!(
        s.accounts
            .approve_play(&s.unrated, ADMIN_PIN.into())
            .await
            .unwrap(),
        PinCheck::Matched
    );
    assert!(restricted(s.accounts.admit(&s.adult).await));

    s.accounts
        .approve_play(&s.adult, ADMIN_PIN.into())
        .await
        .unwrap();
    assert!(
        !s.accounts
            .store
            .game_age_approved(&s.kid, &s.adult)
            .await
            .unwrap()
    );
    // Allowed until the game starts: a launch that asks for a disc first
    // still has it.
    s.accounts.admit(&s.adult).await.unwrap();
    s.accounts.admit(&s.adult).await.unwrap();
    s.accounts.started(&s.adult);
    assert!(restricted(s.accounts.admit(&s.adult).await));
    // Still restricted for the UI: every Play asks.
    assert!(
        s.accounts
            .age_restriction(&s.adult)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn switching_accounts_drops_an_approval() {
    let s = setup().await;
    s.accounts.sign_in(&s.kid, None).await.unwrap();
    s.accounts
        .approve_play(&s.adult, ADMIN_PIN.into())
        .await
        .unwrap();
    s.accounts.sign_in(&s.strict, None).await.unwrap();
    assert!(restricted(s.accounts.admit(&s.adult).await));
    s.accounts.sign_in(&s.kid, None).await.unwrap();
    assert!(restricted(s.accounts.admit(&s.adult).await));
}

#[tokio::test]
async fn wrong_pins_go_through_the_admin_throttle() {
    let s = setup().await;
    s.accounts.sign_in(&s.kid, None).await.unwrap();
    for _ in 0..5 {
        s.accounts
            .approve_play(&s.adult, "0000".into())
            .await
            .unwrap();
    }
    let locked = s
        .accounts
        .approve_play(&s.adult, ADMIN_PIN.into())
        .await
        .unwrap();
    assert!(matches!(locked, PinCheck::Locked { .. }), "{locked:?}");
    assert!(restricted(s.accounts.admit(&s.adult).await));
}

#[tokio::test]
async fn admins_are_never_asked() {
    let s = setup().await;
    assert_eq!(s.accounts.active_id().as_deref(), Some(s.admin.as_str()));
    for game in [&s.adult, &s.family, &s.unrated] {
        s.accounts.admit(game).await.unwrap();
        assert!(s.accounts.age_restriction(game).await.unwrap().is_none());
    }
}

#[tokio::test]
async fn unrated_games_need_a_pin_only_where_the_admin_chose_it() {
    let s = setup().await;
    s.accounts.sign_in(&s.kid, None).await.unwrap();
    s.accounts.admit(&s.unrated).await.unwrap();
    s.accounts.sign_in(&s.strict, None).await.unwrap();
    let error = s.accounts.admit(&s.unrated).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "an admin PIN is needed to play Homebrew, which is unrated"
    );
    s.accounts.admit(&s.family).await.unwrap();
}

#[tokio::test]
async fn permanent_approval_survives_reopening_and_isolates_accounts_and_games() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.sqlite");
    let s = setup_store(LibraryStore::open(&path).await.unwrap()).await;
    s.accounts.sign_in(&s.strict, None).await.unwrap();
    assert_eq!(
        s.accounts
            .approve_play_for(&s.strict, &s.adult, "0000".into(), true)
            .await
            .unwrap(),
        PinCheck::Wrong
    );
    assert!(restricted(s.accounts.admit(&s.adult).await));
    s.accounts
        .approve_play_for(&s.strict, &s.adult, ADMIN_PIN.into(), true)
        .await
        .unwrap();
    s.accounts.admit(&s.adult).await.unwrap();
    s.accounts.started(&s.adult);
    s.accounts.admit(&s.adult).await.unwrap();
    assert!(
        s.accounts
            .age_restriction(&s.adult)
            .await
            .unwrap()
            .is_none()
    );
    assert!(restricted(s.accounts.admit(&s.unrated).await));
    assert_eq!(s.accounts.signed_in().unwrap().max_age, Some(12));
    s.accounts.store.close().await;
    let reopened = AccountService::open(LibraryStore::open(&path).await.unwrap(), Bus::default())
        .await
        .unwrap();
    reopened.sign_in(&s.strict, None).await.unwrap();
    reopened.admit(&s.adult).await.unwrap();
    assert!(restricted(reopened.admit(&s.unrated).await));
    reopened.sign_in(&s.kid, None).await.unwrap();
    assert!(restricted(reopened.admit(&s.adult).await));
    assert!(
        reopened
            .approve_play_for(&s.strict, &s.unrated, ADMIN_PIN.into(), true)
            .await
            .is_err()
    );
    reopened.sign_in(&s.strict, None).await.unwrap();
    assert!(restricted(reopened.admit(&s.unrated).await));
}

#[tokio::test]
async fn browsing_counts_pages_and_search_follow_the_accounts_persistent_exceptions() {
    use crate::model::{GameFilter, ListGames};
    let s = setup().await;
    let store = &s.accounts.store;
    let all = ListGames::default();
    let titles =
        |page: crate::model::GamePage| page.items.into_iter().map(|g| g.title).collect::<Vec<_>>();
    s.accounts.sign_in(&s.strict, None).await.unwrap();
    assert_eq!(
        titles(
            store
                .list_games_for_account(&all, Some(&s.strict))
                .await
                .unwrap()
        ),
        ["Family Game"]
    );
    assert_eq!(
        store
            .library_stats_for_account(Some(&s.strict))
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .game_ids_for_account(&all, Some(&s.strict))
            .await
            .unwrap()
            .as_slice(),
        std::slice::from_ref(&s.family)
    );
    let letters = store
        .title_letters_for_account(&all, Some(&s.strict))
        .await
        .unwrap();
    assert_eq!(letters.len(), 1);
    assert_eq!(letters[0].letter, "F");
    for id in [&s.adult, &s.family, &s.unrated] {
        let mut game = store.game(id).await.unwrap().unwrap();
        game.platforms = vec![game.title.clone()];
        game.favorite = true;
        store.upsert_game(&game).await.unwrap();
    }
    let platforms = store
        .library_platforms_for_account(GameFilter::All, Some(&s.strict))
        .await
        .unwrap();
    assert_eq!(platforms.len(), 1);
    assert_eq!(platforms[0].name, "Family Game");
    let search = ListGames {
        query: Some("Game".into()),
        ..all.clone()
    };
    let found = store
        .list_games_for_account(&search, Some(&s.strict))
        .await
        .unwrap();
    assert_eq!(found.total, 2);
    assert!(
        found
            .items
            .iter()
            .find(|g| g.id == s.adult)
            .unwrap()
            .admin_pin_needed
    );
    assert!(restricted(s.accounts.admit(&s.adult).await));
    // Temporary permission does not change browsing, counts or lock labels.
    s.accounts
        .approve_play(&s.adult, ADMIN_PIN.into())
        .await
        .unwrap();
    assert_eq!(
        store
            .list_games_for_account(&all, Some(&s.strict))
            .await
            .unwrap()
            .total,
        1
    );
    assert!(
        store
            .list_games_for_account(&search, Some(&s.strict))
            .await
            .unwrap()
            .items
            .iter()
            .find(|g| g.id == s.adult)
            .unwrap()
            .admin_pin_needed
    );
    s.accounts.started(&s.adult);
    s.accounts
        .approve_play_for(&s.strict, &s.adult, ADMIN_PIN.into(), true)
        .await
        .unwrap();
    assert_eq!(
        store
            .library_stats_for_account(Some(&s.strict))
            .await
            .unwrap()
            .total,
        2
    );
    let first = ListGames {
        filter: GameFilter::Favorites,
        limit: 1,
        ..all.clone()
    };
    let page = store
        .list_games_for_account(&first, Some(&s.strict))
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(titles(page), ["Family Game"]);
    let next = ListGames { offset: 1, ..first };
    let page = store
        .list_games_for_account(&next, Some(&s.strict))
        .await
        .unwrap();
    assert!(!page.items[0].admin_pin_needed);
    assert_eq!(titles(page), ["Grown-up Game"]);
    // Unrated titles are searchable too; whitespace is still browsing.
    let unrated = ListGames {
        query: Some("Homebrew".into()),
        ..all.clone()
    };
    assert!(
        store
            .list_games_for_account(&unrated, Some(&s.strict))
            .await
            .unwrap()
            .items[0]
            .admin_pin_needed
    );
    let blank = ListGames {
        query: Some("  ".into()),
        ..all.clone()
    };
    assert_eq!(
        store
            .list_games_for_account(&blank, Some(&s.strict))
            .await
            .unwrap()
            .total,
        2
    );
    assert_eq!(
        store
            .list_games_for_account(&all, Some(&s.kid))
            .await
            .unwrap()
            .total,
        2
    );
    assert_eq!(
        store
            .library_stats_for_account(Some(&s.admin))
            .await
            .unwrap()
            .total,
        3
    );
    assert_eq!(store.library_stats().await.unwrap().total, 3);
    // An unrated game's permanent exception has the same visibility rule.
    s.accounts
        .approve_play_for(&s.strict, &s.unrated, ADMIN_PIN.into(), true)
        .await
        .unwrap();
    assert_eq!(
        store
            .list_games_for_account(&all, Some(&s.strict))
            .await
            .unwrap()
            .total,
        3
    );
}
