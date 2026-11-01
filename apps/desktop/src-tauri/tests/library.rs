//! Typed IPC against a generated SQLite library; no user's database is opened.
use caskade_core::{
    db::LibraryStore,
    model::{Game, GameEntry, InstallLocation, InstallState, PlaySession, SourceId},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tauri::test::{mock_builder, mock_context, noop_assets};

use crate::ipc::invoke;

#[test]
fn real_library_commands_filter_search_page_and_read_sessions() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
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
    tauri::async_runtime::block_on(async {
        store.upsert_games(&games).await.unwrap();
        store.upsert_entry(&entry).await.unwrap();
        store.upsert_session(&session).await.unwrap();
    });
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![
            caskade_desktop::commands::library::list_games,
            caskade_desktop::commands::library::get_game,
            caskade_desktop::commands::library::library_stats,
            caskade_desktop::commands::library::list_sessions,
        ],
    );
    let app = mock_builder()
        .manage(
            tauri::async_runtime::block_on(caskade_core::accounts::AccountService::open(
                store.clone(),
                caskade_core::bus::Bus::default(),
            ))
            .unwrap(),
        )
        .manage(store.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let view = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let query = |filter: &str, text: Option<&str>, offset: u32| json!({"query":{"query":text,"filter":filter,"sort":"title","offset":offset,"limit":100}});
    let first = invoke(&view, "list_games", query("all", None, 0)).unwrap();
    assert_eq!(first["total"], 1203);
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    let tail = invoke(&view, "list_games", query("all", None, 1200)).unwrap();
    assert_eq!(tail["items"].as_array().unwrap().len(), 3);
    for filter in ["favorites", "installed", "recent"] {
        let page = invoke(&view, "list_games", query(filter, None, 0)).unwrap();
        assert_eq!(page["total"], 1, "{filter}");
        assert_eq!(page["items"][0]["id"], game_id);
    }
    let mut worst = Duration::ZERO;
    for _ in 0..20 {
        let start = Instant::now();
        let page = invoke(&view, "list_games", query("all", Some("0620"), 0)).unwrap();
        worst = worst.max(start.elapsed());
        assert_eq!(page["total"], 1);
        assert_eq!(page["items"][0]["id"], game_id);
    }
    println!("1203-game FTS5 IPC search worst of20: {worst:?}");
    assert!(
        worst < Duration::from_millis(50),
        "search exceeds50ms: {worst:?}"
    );
    let details = invoke(&view, "get_game", json!({"id":game_id})).unwrap();
    assert_eq!(details["entries"][0]["id"], entry.id);
    assert_eq!(details["playtimeMs"], 3000);
    assert_eq!(
        invoke(&view, "get_game", json!({"id":"absent"})).unwrap(),
        Value::Null
    );
    let stats = invoke(&view, "library_stats", json!({})).unwrap();
    assert_eq!(stats["total"], 1203);
    assert_eq!(stats["playtimeMs"], 3000);
    for game in [Some(&game_id), None] {
        let sessions = invoke(&view, "list_sessions", json!({"gameId":game,"limit":10})).unwrap();
        assert_eq!(sessions[0]["id"], session.id);
        assert_eq!(sessions[0]["durationMs"], 3000);
    }
    assert!(invoke(&view, "list_games", query("invented-filter", None, 0)).is_err());
    tauri::async_runtime::block_on(store.close());
}

#[test]
fn hiding_a_game_takes_it_out_of_the_library_until_it_is_shown_again() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let games = [Game::new("Decomp Buddy"), Game::new("Doom")];
    let hidden_id = games[0].id.clone();
    tauri::async_runtime::block_on(store.upsert_games(&games)).unwrap();
    let bus = caskade_core::bus::Bus::default();
    let mut events = bus.stream();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![
            caskade_desktop::commands::library::list_games,
            caskade_desktop::commands::library::library_stats,
            caskade_desktop::commands::library::set_game_hidden,
            caskade_desktop::commands::library::hidden_games,
        ],
    );
    let app = mock_builder()
        .manage(
            tauri::async_runtime::block_on(caskade_core::accounts::AccountService::open(
                store.clone(),
                caskade_core::bus::Bus::default(),
            ))
            .unwrap(),
        )
        .manage(store.clone())
        .manage(bus)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let view = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let titles = |command: &str, body: Value| {
        invoke(&view, command, body).unwrap()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|game| game["title"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let library = || titles("list_games", json!({"query":{"filter":"all","limit":100}}));
    let hidden = || titles("hidden_games", json!({"offset":0,"limit":100}));

    invoke(
        &view,
        "set_game_hidden",
        json!({"id":hidden_id,"hidden":true}),
    )
    .unwrap();
    assert_eq!(library(), ["Doom"]);
    assert_eq!(hidden(), ["Decomp Buddy"]);
    assert_eq!(
        invoke(&view, "library_stats", json!({})).unwrap()["total"],
        1
    );
    // The UI refetches on the update.
    assert!(matches!(
        events.try_recv(),
        Ok(caskade_core::bus::Event::LibraryGameUpdated { game_id }) if game_id == hidden_id
    ));

    invoke(
        &view,
        "set_game_hidden",
        json!({"id":hidden_id,"hidden":false}),
    )
    .unwrap();
    assert_eq!(library(), ["Decomp Buddy", "Doom"]);
    assert!(hidden().is_empty());
    assert!(
        invoke(
            &view,
            "set_game_hidden",
            json!({"id":"absent","hidden":true})
        )
        .is_err()
    );
    tauri::async_runtime::block_on(store.close());
}

#[test]
fn age_controls_filter_browsing_but_search_stays_locked_until_this_account_is_approved() {
    use caskade_core::{
        accounts::{
            AccountAccess, AccountRole, AccountService, DEFAULT_AVATAR, NewAccount, Permissions,
        },
        bus::Bus,
        igdb::IgdbAgeRating,
    };
    use caskade_desktop::commands::{launch, library};
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let (accounts, kid, sibling, adult, family) = tauri::async_runtime::block_on(async {
        let accounts = AccountService::open(store.clone(), bus).await.unwrap();
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
        (accounts, kid, sibling, adult, family)
    });
    let app = mock_builder()
        .manage(store.clone())
        .manage(accounts.clone())
        .invoke_handler(tauri::generate_handler![
            library::list_games,
            library::library_stats,
            launch::game_age_check,
            launch::game_age_approve
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let view = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let browse = || invoke(&view, "list_games", json!({"query": {}})).unwrap();
    assert_eq!(browse()["items"][0]["id"], family.id);
    assert_eq!(browse()["total"], 1);
    let found = invoke(&view, "list_games", json!({"query": {"query": "Adult"}})).unwrap();
    assert_eq!(found["items"][0]["id"], adult.id);
    assert_eq!(found["items"][0]["adminPinNeeded"], true);
    let check = || invoke(&view, "game_age_check", json!({"gameId": adult.id})).unwrap();
    assert_eq!(check()["accountName"], "Kid");
    let approve = |pin: &str, permanent: bool| {
        invoke(
            &view,
            "game_age_approve",
            json!({"gameId": adult.id, "accountId": kid, "pin": pin, "permanent": permanent}),
        )
    };
    assert_eq!(approve("0000", true).unwrap()["type"], "wrong");
    assert_eq!(browse()["total"], 1);
    assert_eq!(approve("2468", false).unwrap()["type"], "matched");
    assert_eq!(browse()["total"], 1);
    assert!(!check().is_null());
    assert_eq!(approve("2468", true).unwrap()["type"], "matched");
    assert!(check().is_null());
    assert_eq!(browse()["total"], 2);
    assert_eq!(
        invoke(&view, "library_stats", json!({})).unwrap()["total"],
        2
    );
    tauri::async_runtime::block_on(accounts.sign_in(&sibling, None)).unwrap();
    assert_eq!(browse()["total"], 1);
    assert!(!check().is_null());
    assert!(approve("2468", true).is_err());
}
