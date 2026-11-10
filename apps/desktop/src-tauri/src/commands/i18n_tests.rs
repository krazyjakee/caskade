//! The language command and event on the mock runtime.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::i18n;
use std::sync::{Arc, Mutex};
use tauri::{
    Listener,
    test::{MockRuntime, mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

fn app() -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
    let specta = tauri_specta::Builder::<MockRuntime>::new()
        .commands(tauri_specta::collect_commands![locale_current])
        .events(tauri_specta::collect_events![LocaleChanged]);
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, window)
}

#[test]
fn the_webview_learns_the_language_and_what_system_means() {
    let (_app, window) = app();
    let reply: LocaleInfo =
        serde_json::from_value(invoke(&window, "locale_current", serde_json::json!({})).unwrap())
            .unwrap();
    assert_eq!(reply.available.first().map(String::as_str), Some("en"));
    assert!(reply.available.iter().any(|tag| tag == "sv"));
    assert!(reply.available.contains(&reply.locale));
    // System is the OS's language when Caskade ships it, else English.
    assert!(reply.available.contains(&reply.system));
    assert_eq!(reply.system, i18n::resolve(i18n::SYSTEM));
}

#[test]
fn each_change_of_language_reaches_the_webview() {
    let (app, _window) = app();
    let seen = Arc::new(Mutex::new(Vec::new()));
    app.listen(LocaleChanged::NAME, {
        let seen = seen.clone();
        move |event| {
            let payload: caskade_app::events::LocaleChanged =
                serde_json::from_str(event.payload()).unwrap();
            seen.lock().unwrap().push(payload.locale);
        }
    });
    // A channel of its own: the core's language is the whole process's.
    let (language, changes) = watch::channel("en".to_owned());
    let announcing = tauri::async_runtime::spawn(announce(app.handle().clone(), changes));
    let wait_for = |count: usize| {
        for _ in 0..200 {
            if seen.lock().unwrap().len() >= count {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("no LocaleChanged");
    };

    language.send("sv".into()).unwrap();
    wait_for(1);
    language.send("en".into()).unwrap();
    wait_for(2);
    assert_eq!(seen.lock().unwrap().as_slice(), ["sv", "en"]);
    announcing.abort();
}
