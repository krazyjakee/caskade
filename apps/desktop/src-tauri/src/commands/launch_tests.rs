//! Typed IPC -> real core launch -> bus -> typed Tauri events -> SQLite.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    launch::LaunchConfig,
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Listener, Manager,
    test::{mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

/// The #51 fixture game as a stored, installed entry; it writes `token`
/// and exits.
fn fixture_entry(temp: &std::path::Path) -> (LibraryStore, GameEntry, std::path::PathBuf) {
    let exe = temp.join("fixture-game");
    let token = temp.join("token");
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../crates/caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    )
    .unwrap();
    let (store, entry) = tauri::async_runtime::block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = Game::new("IPC fixture");
        store.upsert_game(&game).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "ipc");
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External { path: temp.into() });
        entry.launch = Some(LaunchSpec::Executable {
            path: exe,
            args: vec!["--token".into(), token.to_str().unwrap().into()],
            cwd: None,
            env: BTreeMap::new(),
        });
        store.upsert_entry(&entry).await.unwrap();
        (store, entry)
    });
    (store, entry, token)
}

fn fast_launches(store: &LibraryStore, bus: &Bus) -> LaunchService {
    LaunchService::new(store.clone())
        .with_bus(bus.clone())
        .with_aliases(None)
        .with_config(LaunchConfig {
            poll_interval: Duration::from_millis(10),
            exit_grace: Duration::from_millis(20),
            ..Default::default()
        })
}

#[test]
fn stored_entry_launch_records_session_and_forwards_typed_events() {
    let temp = tempfile::tempdir().unwrap();
    let (store, entry, token) = fixture_entry(temp.path());
    let bus = Bus::default();
    let service = fast_launches(&store, &bus);
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![launch_game, running_games])
        .events(tauri_specta::collect_events![
            crate::events::GameStarted,
            crate::events::GameExited
        ]);
    let app = mock_builder()
        .manage(service)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    crate::events::bridge(app.handle().clone(), &bus);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    for event in [
        crate::events::GameStarted::NAME,
        crate::events::GameExited::NAME,
    ] {
        let seen = events.clone();
        let event_name = event.to_owned();
        app.listen(event, move |event| {
            seen.lock().unwrap().push((
                event_name.clone(),
                serde_json::from_str::<serde_json::Value>(event.payload()).unwrap(),
            ))
        });
    }
    assert!(
        invoke(
            &window,
            "launch_game",
            serde_json::json!({"entryId":"/bin/sh"})
        )
        .is_err()
    );
    let ticket = invoke(
        &window,
        "launch_game",
        serde_json::json!({"entryId":entry.id}),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let saved =
            tauri::async_runtime::block_on(store.sessions_for_game(&entry.game_id, 10)).unwrap();
        if saved.first().is_some_and(|s| s.ended_at_ms.is_some())
            && events.lock().unwrap().len() == 2
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "launch completion or UI events missing"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(token.is_file());
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, crate::events::GameStarted::NAME);
    assert_eq!(events[1].0, crate::events::GameExited::NAME);
    assert_eq!(events[0].1["sessionId"], ticket["sessionId"]);
    assert_eq!(events[1].1["sessionId"], ticket["sessionId"]);
    assert!(events[0].1["endedAtMs"].is_null());
    assert!(events[1].1["endedAtMs"].is_number());
    assert_eq!(
        invoke(&window, "running_games", serde_json::json!({})).unwrap(),
        serde_json::json!([])
    );
    app.state::<LaunchService>().shutdown();
}

/// #150: a pad's confirm reaches the webview, which presses the focused
/// Play; that launch starts the #51 fixture game on the real core, and its
/// session is recorded.
#[test]
fn a_pads_confirm_on_play_launches_the_fixture_game_and_records_its_session() {
    use crate::commands::input::{InputActionEvent, sink};
    use caskade_input::{Action, Button, GlyphFamily, InputAction, InputService, Raw, Timing};

    let temp = tempfile::tempdir().unwrap();
    let (store, entry, token) = fixture_entry(temp.path());
    let bus = Bus::default();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![launch_game, running_games])
        .events(tauri_specta::collect_events![
            InputActionEvent,
            crate::events::GameStarted,
            crate::events::GameExited
        ]);
    let app = mock_builder()
        .manage(fast_launches(&store, &bus))
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    crate::events::bridge(app.handle().clone(), &bus);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let pressed = Arc::new(Mutex::new(Vec::<Action>::new()));
    let seen = pressed.clone();
    app.listen(InputActionEvent::NAME, move |event| {
        let action: InputAction = serde_json::from_str(event.payload()).unwrap();
        seen.lock().unwrap().push(action.action);
    });
    let pads = InputService::without_pads(
        sink(app.handle().clone(), None, |_| true, |_| {}),
        Timing::default(),
    );
    pads.inject(Raw::Connected {
        pad: 1,
        name: "Xbox Wireless Controller".into(),
        glyphs: GlyphFamily::Xbox,
    });
    pads.inject(Raw::Pressed(1, Button::South));
    pads.inject(Raw::Released(1, Button::South));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !pressed.lock().unwrap().contains(&Action::Confirm) {
        assert!(Instant::now() < deadline, "the pad's confirm never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }

    // The webview's Play has focus: confirm presses it.
    invoke(
        &window,
        "launch_game",
        serde_json::json!({ "entryId": entry.id }),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let session = loop {
        let saved =
            tauri::async_runtime::block_on(store.sessions_for_game(&entry.game_id, 10)).unwrap();
        if let Some(session) = saved.into_iter().find(|s| s.ended_at_ms.is_some()) {
            break session;
        }
        assert!(Instant::now() < deadline, "no session was recorded");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(token.is_file(), "the fixture game ran");
    assert_eq!(session.entry_id, entry.id);
    app.state::<LaunchService>().shutdown();
}

/// #209: over IPC, a game above the signed-in account's age limit doesn't
/// start until an admin's PIN approves one launch of it.
#[test]
fn a_game_over_the_age_limit_starts_once_per_admin_approval() {
    use caskade_core::{
        accounts::{AccountAccess, AccountRole, DEFAULT_AVATAR, NewAccount, Permissions},
        igdb::IgdbAgeRating,
    };

    let temp = tempfile::tempdir().unwrap();
    let (store, entry, token) = fixture_entry(temp.path());
    let bus = Bus::default();
    let accounts = tauri::async_runtime::block_on(async {
        store
            .set_game_age_ratings(
                &entry.game_id,
                &[IgdbAgeRating {
                    board: "pegi".into(),
                    rating: "18".into(),
                }],
            )
            .await
            .unwrap();
        let accounts = AccountService::open(store.clone(), bus.clone())
            .await
            .unwrap();
        let admin = accounts.active_id().unwrap();
        accounts.set_pin(&admin, Some("2468".into())).await.unwrap();
        let kid = accounts
            .create(NewAccount {
                name: "Kid".into(),
                avatar: DEFAULT_AVATAR.into(),
                access: AccountAccess {
                    role: AccountRole::Standard,
                    permissions: Permissions::default(),
                    max_age: Some(12),
                    unrated_needs_pin: false,
                },
                pin: None,
            })
            .await
            .unwrap();
        accounts.sign_in(&kid.id, None).await.unwrap();
        accounts
    });
    let service = fast_launches(&store, &bus).with_play_gate(Arc::new(accounts.clone()));
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![launch_game_with, game_age_check, game_age_approve],
    );
    let app = mock_builder()
        .manage(service)
        .manage(accounts.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let launch = || {
        invoke(
            &window,
            "launch_game_with",
            serde_json::json!({ "entryId": entry.id, "choice": null }),
        )
        .unwrap()
    };
    let approve = |pin: &str| {
        invoke(
            &window,
            "game_age_approve",
            serde_json::json!({ "gameId": entry.game_id, "accountId": accounts.active_id(), "pin": pin, "permanent": false }),
        )
        .unwrap()
    };

    let check = invoke(
        &window,
        "game_age_check",
        serde_json::json!({ "gameId": entry.game_id }),
    )
    .unwrap();
    assert_eq!(check["minAge"], 18);
    assert_eq!(check["maxAge"], 12);
    let refused = launch();
    assert_eq!(refused["type"], "age_restricted");
    assert_eq!(refused["restriction"]["title"], "IPC fixture");
    assert_eq!(approve("1111")["type"], "wrong");
    assert_eq!(launch()["type"], "age_restricted");
    assert!(!token.exists(), "nothing ran");

    assert_eq!(approve("2468")["type"], "matched");
    assert_eq!(launch()["type"], "started");
    // The approval was for that launch only.
    assert_eq!(launch()["type"], "age_restricted");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !token.is_file() {
        assert!(Instant::now() < deadline, "the approved launch never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.state::<LaunchService>().shutdown();
}
