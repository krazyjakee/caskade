//! The typed `hello` round trip on Tauri's mock runtime, and the committed
//! TypeScript bindings being current.

use std::sync::{Arc, Mutex};

use crate::ipc::invoke;
use caskade_desktop::{bindings, commands::Greeting};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};

fn app() -> tauri::App<tauri::test::MockRuntime> {
    // The app's builder is for Wry; the same command and event on the
    // mock runtime.
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            caskade_desktop::commands::hello::<tauri::test::MockRuntime>
        ])
        .events(tauri_specta::collect_events![
            caskade_desktop::events::Hello
        ]);
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("mock app");
    specta.mount_events(&app);
    app
}

#[test]
fn hello_command_and_event_round_trip() {
    let app = app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    app.listen("hello", move |event| {
        seen.lock().unwrap().push(event.payload().to_owned());
    });

    let reply = invoke(
        &webview,
        "hello",
        serde_json::json!({ "name": "  Caskade " }),
    )
    .unwrap();
    let greeting: Greeting = serde_json::from_value(reply.clone()).unwrap();
    assert_eq!(greeting.message, "Hello, Caskade!");
    assert_eq!(greeting.core_version, caskade_core::VERSION);
    assert_eq!(reply["coreVersion"], caskade_core::VERSION);
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&events[0]).unwrap(),
        reply
    );

    assert_eq!(
        invoke(&webview, "hello", serde_json::json!({ "name": "   " })).unwrap_err(),
        "name is empty"
    );
}

#[test]
fn committed_bindings_are_current() {
    let committed = std::fs::read_to_string(bindings::default_path()).expect("committed bindings");
    let fresh = bindings::render().expect("render bindings");
    assert!(
        committed == fresh,
        "apps/desktop/src/api/bindings.ts is stale: run `just bindings`"
    );
}
