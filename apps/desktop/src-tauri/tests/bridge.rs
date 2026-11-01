//! Bus events reach the webview as typed tauri-specta events.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use caskade_core::bus::{Bus, Event, GameSession};
use tauri::{
    Listener,
    test::{MockRuntime, mock_builder, mock_context, noop_assets},
};

fn app() -> tauri::App<MockRuntime> {
    let specta = tauri_specta::Builder::<MockRuntime>::new().events(tauri_specta::collect_events![
        caskade_desktop::events::GameStarted,
        caskade_desktop::events::GameExited,
        caskade_desktop::events::LibraryChanged,
        caskade_desktop::events::SettingsChanged,
    ]);
    let app = mock_builder().build(mock_context(noop_assets())).expect("mock app");
    specta.mount_events(&app);
    app
}

fn record(app: &tauri::App<MockRuntime>, names: &[&'static str]) -> Arc<Mutex<Vec<(String, serde_json::Value)>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    for &name in names {
        let seen = seen.clone();
        app.listen(name, move |event| {
            seen.lock()
                .unwrap()
                .push((name.to_string(), serde_json::from_str(event.payload()).unwrap()));
        });
    }
    seen
}

#[test]
fn bus_events_are_forwarded_with_typed_names() {
    let app = app();
    let seen = record(&app, &["game-started", "game-exited", "library-changed", "settings-changed"]);
    let bus = Bus::default();
    caskade_desktop::events::bridge(app.handle().clone(), &bus);
    let session = GameSession {
        game_id: "g1".into(),
        entry_id: "e1".into(),
        session_id: "s1".into(),
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: None,
        duration_ms: 0,
    };
    tauri::async_runtime::block_on(async {
        bus.emit(Event::GameStarted(session.clone()));
        bus.emit(Event::LibraryGameAdded { game_id: "g1".into() });
        bus.emit(Event::LibraryScanCompleted { sources: vec![] });
        bus.emit(Event::SettingsChanged {
            namespace: "library".into(),
            version: 3,
        });
        // Not forwarded.
        bus.emit(Event::AppReady);
    });
    for _ in 0..100 {
        if seen.lock().unwrap().len() >= 4 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        [
            (
                "game-started".to_owned(),
                serde_json::to_value(&session).unwrap()
            ),
            (
                "library-changed".to_owned(),
                serde_json::json!({ "change": "added", "gameId": "g1" })
            ),
            // Other events don't wait for the library window to close.
            (
                "settings-changed".to_owned(),
                serde_json::json!({ "namespace": "library", "version": 3 })
            ),
            (
                "library-changed".to_owned(),
                serde_json::json!({ "change": "scanned", "gameId": null })
            ),
        ]
    );
}

fn wait_for(seen: &Mutex<Vec<(String, serde_json::Value)>>, count: usize) {
    for _ in 0..200 {
        if seen.lock().unwrap().len() >= count {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_burst_of_library_events_reaches_the_webview_as_two() {
    let app = app();
    let seen = record(&app, &["library-changed"]);
    let bus = Bus::default();
    caskade_desktop::events::bridge(app.handle().clone(), &bus);
    for i in 0..50 {
        bus.emit(Event::LibraryGameAdded {
            game_id: format!("g{i}"),
        });
    }
    wait_for(&seen, 2);
    // Nothing else trails the burst.
    std::thread::sleep(caskade_desktop::events::LIBRARY_WINDOW * 3);
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (
                "library-changed".to_owned(),
                serde_json::json!({ "change": "added", "gameId": "g0" })
            ),
            (
                "library-changed".to_owned(),
                serde_json::json!({ "change": "scanned", "gameId": null })
            ),
        ]
    );
    // A quiet period has passed: the next event goes out at once.
    bus.emit(Event::LibraryGameRemoved { game_id: "g7".into() });
    wait_for(&seen, 3);
    assert_eq!(
        seen.lock().unwrap()[2].1,
        serde_json::json!({ "change": "removed", "gameId": "g7" })
    );
}
