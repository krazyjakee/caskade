//! Launch commands -> real core launch -> bus -> the app's events -> SQLite.
use super::*;
use crate::events::{Emit, Event, Events};
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

/// What reached the UI.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Event>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

/// The #51 fixture game as a stored, installed entry; it writes `token`
/// and exits.
async fn fixture_entry(temp: &std::path::Path) -> (LibraryStore, GameEntry, std::path::PathBuf) {
    let exe = temp.join("fixture-game");
    let token = temp.join("token");
    caskade_core::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    );
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

/// The game starts and exits the UI heard, in order.
fn sessions(sink: &Sink) -> Vec<(&'static str, caskade_core::bus::GameSession)> {
    sink.0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            Event::GameStarted(crate::events::GameStarted(session)) => {
                Some(("game.started", session.clone()))
            }
            Event::GameExited(crate::events::GameExited(session)) => {
                Some(("game.exited", session.clone()))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn stored_entry_launch_records_session_and_forwards_typed_events() {
    let temp = tempfile::tempdir().unwrap();
    let (store, entry, token) = fixture_entry(temp.path()).await;
    let bus = Bus::default();
    let service = fast_launches(&store, &bus);
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);
    assert!(launch_game(&service, "/bin/sh").await.is_err());
    let ticket = launch_game(&service, &entry.id).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let saved = store.sessions_for_game(&entry.game_id, 10).await.unwrap();
        if saved.first().is_some_and(|s| s.ended_at_ms.is_some()) && sessions(&sink).len() == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "launch completion or UI events missing"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(token.is_file());
    let events = sessions(&sink);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "game.started");
    assert_eq!(events[1].0, "game.exited");
    assert_eq!(events[0].1.session_id, ticket.session_id);
    assert_eq!(events[1].1.session_id, ticket.session_id);
    assert!(events[0].1.ended_at_ms.is_none());
    assert!(events[1].1.ended_at_ms.is_some());
    assert!(running_games(&service).await.unwrap().is_empty());
    service.shutdown();
}

/// #150: a pad's confirm reaches the UI, which presses the focused Play;
/// that launch starts the #51 fixture game on the real core, and its
/// session is recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_pads_confirm_on_play_launches_the_fixture_game_and_records_its_session() {
    use crate::commands::input::{InputActionEvent, sink as input_sink};
    use caskade_input::{Action, Button, GlyphFamily, InputService, Raw, Timing};

    let temp = tempfile::tempdir().unwrap();
    let (store, entry, token) = fixture_entry(temp.path()).await;
    let bus = Bus::default();
    let service = fast_launches(&store, &bus);
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);
    let pressed = || {
        sink.0.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                Event::InputActionEvent(InputActionEvent(action)) if action.action == Action::Confirm
            )
        })
    };
    let pads = InputService::without_pads(
        input_sink(Events::new(sink.clone()), None, || true, || {}),
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
    while !pressed() {
        assert!(Instant::now() < deadline, "the pad's confirm never arrived");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // The UI's Play has focus: confirm presses it.
    launch_game(&service, &entry.id).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let session = loop {
        let saved = store.sessions_for_game(&entry.game_id, 10).await.unwrap();
        if let Some(session) = saved.into_iter().find(|s| s.ended_at_ms.is_some()) {
            break session;
        }
        assert!(Instant::now() < deadline, "no session was recorded");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(token.is_file(), "the fixture game ran");
    assert_eq!(session.entry_id, entry.id);
    service.shutdown();
}

/// Launches `entry_id` with no choice made.
async fn launch(service: &LaunchService, entry_id: &str) -> LaunchOutcome {
    launch_game_with(service, entry_id, None).await.unwrap()
}

/// #209: a game above the signed-in account's age limit doesn't start until
/// an admin's PIN approves one launch of it.
#[tokio::test(flavor = "multi_thread")]
async fn a_game_over_the_age_limit_starts_once_per_admin_approval() {
    use caskade_core::{
        accounts::{AccountAccess, AccountRole, DEFAULT_AVATAR, NewAccount, Permissions},
        igdb::IgdbAgeRating,
    };

    let temp = tempfile::tempdir().unwrap();
    let (store, entry, token) = fixture_entry(temp.path()).await;
    let bus = Bus::default();
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
    let service = fast_launches(&store, &bus).with_play_gate(Arc::new(accounts.clone()));
    let signed_in = accounts.active_id().unwrap();

    let check = game_age_check(&accounts, &entry.game_id)
        .await
        .unwrap()
        .expect("the game is over the kid's limit");
    assert_eq!(check.min_age, Some(18));
    assert_eq!(check.max_age, 12);
    match launch(&service, &entry.id).await {
        LaunchOutcome::AgeRestricted { restriction } => {
            assert_eq!(restriction.title, "IPC fixture");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        game_age_approve(&accounts, &entry.game_id, &signed_in, "1111".into(), false)
            .await
            .unwrap(),
        PinCheck::Wrong
    );
    assert!(matches!(
        launch(&service, &entry.id).await,
        LaunchOutcome::AgeRestricted { .. }
    ));
    assert!(!token.exists(), "nothing ran");

    assert_eq!(
        game_age_approve(&accounts, &entry.game_id, &signed_in, "2468".into(), false)
            .await
            .unwrap(),
        PinCheck::Matched
    );
    assert!(matches!(
        launch(&service, &entry.id).await,
        LaunchOutcome::Started { .. }
    ));
    // The approval was for that launch only.
    assert!(matches!(
        launch(&service, &entry.id).await,
        LaunchOutcome::AgeRestricted { .. }
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !token.is_file() {
        assert!(Instant::now() < deadline, "the approved launch never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    service.shutdown();
}
