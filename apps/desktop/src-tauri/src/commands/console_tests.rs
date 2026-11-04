//! Console mode: the first mode, the window going fullscreen and back, and
//! the toggle through typed IPC on the mock runtime.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::db::LibraryStore;
use std::cell::RefCell;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{
    Listener,
    test::{MockRuntime, mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

fn settings(start: &str, active: bool) -> ConsoleSettings {
    ConsoleSettings {
        start: start.into(),
        active,
    }
}

/// #244: a new handheld or TV starts in console mode, as the first
/// settings by kind of device leave it; a new desktop doesn't.
#[test]
fn a_new_handheld_starts_in_console_mode_and_a_desktop_doesnt() {
    use caskade_core::device_type::{DeviceType, settle_new_device};
    let first_mode = |kind: DeviceType| {
        tauri::async_runtime::block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let store = LibraryStore::in_memory().await.unwrap();
            let service = SettingsService::new(store.clone());
            service
                .register_core(&caskade_core::paths::AppPaths::under(temp.path()))
                .unwrap();
            settle_new_device(&service, &store, kind).await.unwrap();
            let preferences: ConsoleSettings = service.read_or_default(SETTINGS).await;
            Startup::from(&["caskade".to_owned()], env(&[])).console(&preferences)
        })
    };
    assert!(first_mode(DeviceType::Handheld));
    assert!(first_mode(DeviceType::Tv));
    assert!(!first_mode(DeviceType::Desktop));
    assert!(!first_mode(DeviceType::Laptop));
}

#[test]
fn the_flag_then_the_environment_then_the_setting_decide_the_first_mode() {
    let args = |extra: &[&str]| {
        std::iter::once("caskade")
            .chain(extra.iter().copied())
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let plain = Startup::from(&args(&[]), env(&[]));
    assert!(!plain.console(&settings("auto", false)));
    assert!(plain.console(&settings("auto", true)), "auto remembers");
    assert!(plain.console(&settings("console", false)));
    assert!(!plain.console(&settings("desktop", true)));

    let flagged = Startup::from(&args(&["--console"]), env(&[("CASKADE_CONSOLE", "0")]));
    assert!(
        flagged.console(&settings("desktop", false)),
        "--console wins"
    );

    let deck = Startup::from(&args(&[]), env(&[("SteamGamepadUI", "1")]));
    assert!(deck.gamescope);
    assert!(deck.console(&settings("auto", false)));
    assert!(
        !deck.console(&settings("desktop", false)),
        "the setting overrides"
    );

    let forced_off = Startup::from(
        &args(&[]),
        env(&[
            ("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"),
            ("CASKADE_CONSOLE", "0"),
        ]),
    );
    assert!(!forced_off.console(&settings("console", true)));
    let forced_on = Startup::from(&args(&[]), env(&[("CASKADE_CONSOLE", "1")]));
    assert!(forced_on.console(&settings("desktop", false)));
}

#[test]
fn gamescope_is_detected_from_its_environment() {
    assert!(in_gamescope(env(&[(
        "GAMESCOPE_WAYLAND_DISPLAY",
        "gamescope-0"
    )])));
    assert!(in_gamescope(env(&[("XDG_CURRENT_DESKTOP", "gamescope")])));
    assert!(!in_gamescope(env(&[("GAMESCOPE_WAYLAND_DISPLAY", "")])));
    assert!(!in_gamescope(env(&[("XDG_CURRENT_DESKTOP", "GNOME")])));
    // The program name is never the flag.
    assert!(!wants_console(&["--console".to_string()]));
}

/// A window that records what console mode does to it.
#[derive(Default)]
struct FakeFrame {
    maximized: bool,
    calls: RefCell<Vec<String>>,
}

impl Frame for FakeFrame {
    fn desktop(&self) -> tauri::Result<DesktopFrame> {
        Ok(DesktopFrame {
            maximized: self.maximized,
            position: PhysicalPosition::new(120, 80),
            size: PhysicalSize::new(1280, 800),
        })
    }
    fn fullscreen(&self, on: bool) -> tauri::Result<()> {
        self.calls.borrow_mut().push(format!("fullscreen {on}"));
        Ok(())
    }
    fn restore(&self, desktop: &DesktopFrame) -> tauri::Result<()> {
        self.calls.borrow_mut().push(if desktop.maximized {
            "maximize".into()
        } else {
            format!(
                "restore {}x{} at {},{}",
                desktop.size.width, desktop.size.height, desktop.position.x, desktop.position.y
            )
        });
        Ok(())
    }
}

#[test]
fn leaving_console_mode_puts_the_desktop_window_back() {
    let console = ConsoleWindow::default();
    let frame = FakeFrame::default();
    assert!(console.enter(&frame, None).unwrap());
    assert!(console.active());
    assert!(!console.enter(&frame, None).unwrap(), "already on");
    assert!(console.leave(&frame).unwrap());
    assert!(!console.leave(&frame).unwrap(), "already off");
    assert_eq!(
        frame.calls.borrow().as_slice(),
        [
            "fullscreen true",
            "fullscreen false",
            "restore 1280x800 at 120,80"
        ]
    );

    let maximized = FakeFrame {
        maximized: true,
        ..FakeFrame::default()
    };
    console.enter(&maximized, None).unwrap();
    console.leave(&maximized).unwrap();
    assert_eq!(
        maximized.calls.borrow().as_slice(),
        ["fullscreen true", "fullscreen false", "maximize"]
    );

    // Started in console mode: the frame window-state saved, not the window
    // it is still restoring.
    let started = FakeFrame::default();
    let saved = desktop_from_saved(
        r#"{"main":{"width":1200,"height":760,"x":150,"y":120,"prev_x":0,"prev_y":0,"maximized":false,"visible":true,"decorated":true,"fullscreen":false}}"#,
    );
    console.enter(&started, saved).unwrap();
    console.leave(&started).unwrap();
    assert_eq!(
        started.calls.borrow().as_slice(),
        [
            "fullscreen true",
            "fullscreen false",
            "restore 1200x760 at 150,120"
        ]
    );
    assert_eq!(desktop_from_saved("{}"), None);
    assert_eq!(
        desktop_from_saved(r#"{"main":{"width":0,"height":0,"x":0,"y":0,"maximized":false}}"#),
        None
    );
}

#[test]
fn the_toggle_switches_the_window_remembers_the_choice_and_tells_the_webview() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let service = SettingsService::new(store);
    service
        .register_core(&caskade_core::AppPaths::under(std::path::Path::new(
            "fixture",
        )))
        .unwrap();
    let specta = tauri_specta::Builder::<MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            console_initial_mode,
            set_console_mode::<MockRuntime>
        ])
        .events(tauri_specta::collect_events![ConsoleModeEvent]);
    let app = mock_builder()
        .manage(service.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    // Nothing asks for console mode: Caskade starts on the desktop.
    setup(app.handle(), Startup::default(), &service);
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let events = seen.clone();
    app.listen(ConsoleModeEvent::NAME, move |event| {
        let payload: ConsoleModeEvent = serde_json::from_str(event.payload()).unwrap();
        events.lock().unwrap().push(payload.active);
    });
    assert_eq!(
        invoke(&window, "console_initial_mode", serde_json::json!({})),
        Ok(serde_json::json!(false))
    );

    invoke(
        &window,
        "set_console_mode",
        serde_json::json!({ "active": true }),
    )
    .unwrap();
    assert_eq!(
        invoke(&window, "console_initial_mode", serde_json::json!({})),
        Ok(serde_json::json!(true))
    );
    let stored = tauri::async_runtime::block_on(service.get("core.console")).unwrap();
    assert_eq!(stored.value["active"], true);

    // Asking again changes nothing and tells nobody.
    invoke(
        &window,
        "set_console_mode",
        serde_json::json!({ "active": true }),
    )
    .unwrap();
    invoke(
        &window,
        "set_console_mode",
        serde_json::json!({ "active": false }),
    )
    .unwrap();
    let stored = tauri::async_runtime::block_on(service.get("core.console")).unwrap();
    assert_eq!(stored.value["active"], false);
    assert_eq!(*seen.lock().unwrap(), [true, false]);

    // A restart with `auto` comes back in the remembered mode.
    let mut remembered = stored.value.clone();
    remembered["active"] = true.into();
    tauri::async_runtime::block_on(service.set("core.console", remembered, None)).unwrap();
    let preferences: ConsoleSettings =
        tauri::async_runtime::block_on(service.read_or_default(super::SETTINGS));
    assert!(Startup::default().console(&preferences));
}

/// `close_hold` from the webview, on a console window: a held close doesn't
/// quit or leave console mode, a release lets it through, and Stay forgets
/// the quit.
#[test]
fn the_webview_holds_a_close_until_it_has_asked() {
    use crate::tray::Close;

    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let service = SettingsService::new(store);
    service
        .register_core(&caskade_core::AppPaths::under(std::path::Path::new(
            "fixture",
        )))
        .unwrap();
    let specta = tauri_specta::Builder::<MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            close_hold,
            set_console_mode::<MockRuntime>
        ])
        .events(tauri_specta::collect_events![ConsoleModeEvent]);
    let app = mock_builder()
        .manage(service.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    setup(app.handle(), Startup::default(), &service);
    crate::tray::attach(app.handle(), &service);
    let tray = app.state::<Tray>();
    tray.set_shown(true);
    let hold = |held: bool| {
        invoke(&window, "close_hold", serde_json::json!({ "held": held })).unwrap();
    };
    let console = |active: bool| {
        invoke(
            &window,
            "set_console_mode",
            serde_json::json!({ "active": active }),
        )
        .unwrap();
    };

    // Nothing held: console mode restores, as ever.
    console(true);
    assert!(restores_on_close(app.handle()));

    // Edits: the close waits, in console mode, and nothing restores.
    hold(true);
    assert!(!restores_on_close(app.handle()));
    assert!(is_on(app.handle()));
    assert!(matches!(tray.close(true), Close::Held(_)));

    // Save or Discard: released, the same close goes on through the restore.
    hold(false);
    assert!(restores_on_close(app.handle()));
    assert_eq!(tray.close(true), Close::Go);
    console(false);

    // The tray's Quit waits too, and hearing it doesn't forget it.
    hold(true);
    tray.quit();
    let Close::Held(quit) = tray.close(false) else {
        panic!("the quit wasn't held");
    };
    assert!(tray.unanswered(quit));
    hold(true);
    assert!(!tray.unanswered(quit));
    assert!(!tray.hides_on_close(false), "the quit is still on");

    // Stay: a fresh hold forgets the quit, so the close button hides again.
    hold(false);
    hold(true);
    assert!(tray.hides_on_close(false));
    assert_eq!(tray.close(false), Close::Hide);

    // A release after hearing a quit keeps it: the next close quits.
    tray.quit();
    assert!(matches!(tray.close(false), Close::Held(_)));
    hold(true);
    hold(false);
    assert_eq!(tray.close(false), Close::Go);
}
