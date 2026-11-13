//! The library commands against a generated SQLite library; no user's
//! database is opened.
use caskade_app::commands::{launch, library};
use caskade_core::{
    accounts::{AccountService, PinCheck},
    bus::Bus,
    db::LibraryStore,
    launch::LaunchService,
    model::{
        Game, GameEntry, GameFilter, GameSort, InstallLocation, InstallState, ListGames,
        PlaySession, SourceId,
    },
};
use std::time::{Duration, Instant};

#[tokio::test]
async fn real_library_commands_filter_search_page_and_read_sessions() {
    let store = LibraryStore::in_memory().await.unwrap();
    let mut games = (0..1203)
        .map(|i| Game::new(format!("Original Fixture {i:04}")))
        .collect::<Vec<_>>();
    games[620].favorite = true;
    let game_id = games[620].id.clone();
    let mut entry = GameEntry::new(&game_id, SourceId::new("fixture"), "fixture-620");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: std::env::temp_dir().join("caskade-original-fixture-620"),
    });
    let mut session = PlaySession::new(&game_id, &entry.id, 1_700_000_000_000);
    session.ended_at_ms = Some(session.started_at_ms + 3000);
    session.duration_ms = 3000;
    store.upsert_games(&games).await.unwrap();
    store.upsert_entry(&entry).await.unwrap();
    store.upsert_session(&session).await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let launch = LaunchService::new(store.clone());
    let query = |filter: GameFilter, text: Option<&str>, offset: u32| ListGames {
        query: text.map(str::to_owned),
        filter,
        platform: None,
        sort: GameSort::Title,
        offset,
        limit: 100,
    };
    let first = library::list_games(&store, &accounts, &query(GameFilter::All, None, 0))
        .await
        .unwrap();
    assert_eq!(first.total, 1203);
    assert_eq!(first.items.len(), 100);
    let tail = library::list_games(&store, &accounts, &query(GameFilter::All, None, 1200))
        .await
        .unwrap();
    assert_eq!(tail.items.len(), 3);
    for filter in [
        GameFilter::Favorites,
        GameFilter::Installed,
        GameFilter::Recent,
    ] {
        let page = library::list_games(&store, &accounts, &query(filter, None, 0))
            .await
            .unwrap();
        assert_eq!(page.total, 1, "{filter:?}");
        assert_eq!(page.items[0].id, game_id);
    }
    let mut worst = Duration::ZERO;
    for _ in 0..20 {
        let start = Instant::now();
        let page = library::list_games(&store, &accounts, &query(GameFilter::All, Some("0620"), 0))
            .await
            .unwrap();
        worst = worst.max(start.elapsed());
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, game_id);
    }
    println!("1203-game FTS5 search worst of 20: {worst:?}");
    assert!(
        worst < Duration::from_millis(50),
        "search exceeds 50ms: {worst:?}"
    );
    let details = library::get_game(&store, &game_id).await.unwrap().unwrap();
    assert_eq!(details.entries[0].id, entry.id);
    assert_eq!(details.playtime_ms, 3000);
    assert!(library::get_game(&store, "absent").await.unwrap().is_none());
    let stats = library::library_stats(&store, &accounts).await.unwrap();
    assert_eq!(stats.total, 1203);
    assert_eq!(stats.playtime_ms, 3000);
    for game in [Some(game_id.as_str()), None] {
        let sessions = library::list_sessions(&store, &launch, game, 10)
            .await
            .unwrap();
        assert_eq!(sessions[0].id, session.id);
        assert_eq!(sessions[0].duration_ms, 3000);
    }
    store.close().await;
}

#[tokio::test]
async fn hiding_a_game_takes_it_out_of_the_library_until_it_is_shown_again() {
    let store = LibraryStore::in_memory().await.unwrap();
    let games = [Game::new("Decomp Buddy"), Game::new("Doom")];
    let hidden_id = games[0].id.clone();
    store.upsert_games(&games).await.unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let all = ListGames {
        filter: GameFilter::All,
        limit: 100,
        ..ListGames::default()
    };
    let titles = |page: caskade_core::model::GamePage| {
        page.items
            .into_iter()
            .map(|game| game.title)
            .collect::<Vec<_>>()
    };

    library::set_game_hidden(&store, &bus, &hidden_id, true)
        .await
        .unwrap();
    let shown = titles(library::list_games(&store, &accounts, &all).await.unwrap());
    assert_eq!(shown, ["Doom"]);
    let hidden = titles(library::hidden_games(&store, 0, 100).await.unwrap());
    assert_eq!(hidden, ["Decomp Buddy"]);
    assert_eq!(
        library::library_stats(&store, &accounts)
            .await
            .unwrap()
            .total,
        1
    );
    // The UI refetches on the update.
    assert!(matches!(
        events.try_recv(),
        Ok(caskade_core::bus::Event::LibraryGameUpdated { game_id }) if game_id == hidden_id
    ));

    library::set_game_hidden(&store, &bus, &hidden_id, false)
        .await
        .unwrap();
    let shown = titles(library::list_games(&store, &accounts, &all).await.unwrap());
    assert_eq!(shown, ["Decomp Buddy", "Doom"]);
    assert!(
        library::hidden_games(&store, 0, 100)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        library::set_game_hidden(&store, &bus, "absent", true)
            .await
            .is_err()
    );
    store.close().await;
}

#[tokio::test]
async fn age_controls_filter_browsing_but_search_stays_locked_until_this_account_is_approved() {
    use caskade_core::{
        accounts::{AccountAccess, AccountRole, DEFAULT_AVATAR, NewAccount, Permissions},
        igdb::IgdbAgeRating,
    };
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    accounts
        .set_pin(&accounts.active_id().unwrap(), Some("2468".into()))
        .await
        .unwrap();
    let new = |name: &str| NewAccount {
        name: name.into(),
        avatar: DEFAULT_AVATAR.into(),
        access: AccountAccess {
            role: AccountRole::Standard,
            permissions: Permissions::default(),
            max_age: Some(12),
            unrated_needs_pin: true,
        },
        pin: None,
    };
    let kid = accounts.create(new("Kid")).await.unwrap().id;
    let sibling = accounts.create(new("Sibling")).await.unwrap().id;
    let adult = Game::new("Adult Game");
    let family = Game::new("Family Game");
    store
        .upsert_games(&[adult.clone(), family.clone()])
        .await
        .unwrap();
    for (game, rating) in [(&adult, "18"), (&family, "7")] {
        store
            .set_game_age_ratings(
                &game.id,
                &[IgdbAgeRating {
                    board: "pegi".into(),
                    rating: rating.into(),
                }],
            )
            .await
            .unwrap();
    }
    accounts.sign_in(&kid, None).await.unwrap();

    let (store_ref, accounts_ref) = (&store, &accounts);
    let browse = || async move {
        library::list_games(store_ref, accounts_ref, &ListGames::default())
            .await
            .unwrap()
    };
    assert_eq!(browse().await.items[0].id, family.id);
    assert_eq!(browse().await.total, 1);
    let search = ListGames {
        query: Some("Adult".into()),
        ..ListGames::default()
    };
    let found = library::list_games(&store, &accounts, &search)
        .await
        .unwrap();
    assert_eq!(found.items[0].id, adult.id);
    assert!(found.items[0].admin_pin_needed);
    let adult_id = adult.id.as_str();
    let check = || async move {
        launch::game_age_check(accounts_ref, adult_id)
            .await
            .unwrap()
    };
    assert_eq!(check().await.unwrap().account_name, "Kid");
    let kid_id = kid.as_str();
    let approve = |pin: &'static str, permanent: bool| {
        launch::game_age_approve(accounts_ref, adult_id, kid_id, pin.into(), permanent)
    };
    assert_eq!(approve("0000", true).await.unwrap(), PinCheck::Wrong);
    assert_eq!(browse().await.total, 1);
    assert_eq!(approve("2468", false).await.unwrap(), PinCheck::Matched);
    assert_eq!(browse().await.total, 1);
    assert!(check().await.is_some());
    assert_eq!(approve("2468", true).await.unwrap(), PinCheck::Matched);
    assert!(check().await.is_none());
    assert_eq!(browse().await.total, 2);
    assert_eq!(
        library::library_stats(&store, &accounts)
            .await
            .unwrap()
            .total,
        2
    );
    accounts.sign_in(&sibling, None).await.unwrap();
    assert_eq!(browse().await.total, 1);
    assert!(check().await.is_some());
    assert!(approve("2468", true).await.is_err());
}
