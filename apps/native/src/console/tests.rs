//! Console mode (#107), ported from the webview's `console.test.tsx` and
//! `GameCurtain.test.tsx`: View switches between the shells and back, the
//! console shell has its sidebar, top bar and hints in the glyphs of the
//! controller in hand, and the curtain comes down over a running game and
//! lifts on back and when the last game exits.

use crate::frame::tests::{ONE, Window, source};
use caskade_app::events::{Event, GameExited, GameStarted};
use caskade_core::bus::GameSession;
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};

/// Leaves the mode as the next test expects it: desktop.
struct Desktop;

impl Drop for Desktop {
    fn drop(&mut self) {
        let _ = caskade_app::runtime::block_on(source().settings.remember_console(false));
    }
}

fn open() -> Window {
    let mut window = Window::open();
    window.settle_until("the desktop shell", |window| {
        window.find("data-cs", "app-shell").is_some()
    });
    window
}

fn to_console(window: &mut Window) {
    window.pad(Action::View);
    window.settle_until("the console shell", |window| {
        window.find("data-cs", "console-shell").is_some()
    });
}

#[test]
fn view_switches_to_console_mode_and_back() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _desktop = Desktop;
    let mut window = open();
    assert_eq!(
        window
            .attr(window.get("data-mode", "desktop"), "data-mode")
            .as_deref(),
        Some("desktop")
    );
    to_console(&mut window);
    // The console token set follows `data-mode`.
    assert!(window.find("data-mode", "console").is_some());
    assert!(window.find("data-cs", "app-shell").is_none());
    let sidebar = window.get("data-cs", "console-sidebar");
    let text = window.text(sidebar);
    for link in ["Home", "Library", "Settings", "Desktop mode"] {
        assert!(text.contains(link), "{link} in {text}");
    }
    assert!(window.find("data-cs", "console-topbar").is_some());
    // The choice is remembered for the next start.
    window.settle_until("remembered", |_| {
        caskade_app::runtime::block_on(source().settings.get("core.console"))
            .is_ok_and(|snapshot| snapshot.value["active"] == true)
    });
    // View again: the desktop.
    window.pad(Action::View);
    window.settle_until("the desktop shell", |window| {
        window.find("data-cs", "app-shell").is_some()
            && window.find("data-cs", "console-shell").is_none()
    });
}

#[test]
fn the_hint_bar_shows_what_each_button_does_in_the_controllers_glyphs() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _desktop = Desktop;
    let mut window = open();
    to_console(&mut window);
    window.settle_until("pad hints", |window| {
        window
            .find("data-cs", "hint-bar")
            .is_some_and(|bar| window.attr(bar, "data-glyphs").as_deref() == Some("xbox"))
    });
    let bar = window.get("data-cs", "hint-bar");
    let hint = |window: &Window, action: &str| {
        window
            .find_all("data-part", "hint")
            .into_iter()
            .find(|&hint| window.attr(hint, "data-action").as_deref() == Some(action))
            .map(|hint| window.text(hint))
    };
    assert_eq!(hint(&window, "confirm").as_deref(), Some("ASelect"));
    assert_eq!(hint(&window, "back").as_deref(), Some("BBack"));
    assert_eq!(hint(&window, "view").as_deref(), Some("ViewDesktop mode"));
    assert!(
        window.find_in(bar, "data-part", "clock").is_some(),
        "{}",
        window.text(bar)
    );
    // A key pressed: the keyboard's names.
    window.key(Key::Named(NamedKey::ArrowDown));
    window.settle_until("keyboard hints", |window| {
        window
            .find("data-cs", "hint-bar")
            .is_some_and(|bar| window.attr(bar, "data-glyphs").as_deref() == Some("keyboard"))
    });
    assert_eq!(hint(&window, "confirm").as_deref(), Some("EnterSelect"));
}

fn session(id: &str) -> GameSession {
    serde_json::from_value(serde_json::json!({
        "gameId": "fixture-1",
        "entryId": "e1",
        "sessionId": id,
        "startedAtMs": 1,
        "endedAtMs": null,
        "durationMs": 0
    }))
    .unwrap()
}

#[test]
fn the_curtain_comes_down_over_a_running_game_and_lifts() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _desktop = Desktop;
    let mut window = open();
    to_console(&mut window);
    let state = |window: &Window| {
        window
            .find("data-cs", "game-curtain")
            .and_then(|curtain| window.attr(curtain, "data-state"))
    };
    assert_eq!(state(&window).as_deref(), Some("open"));
    let _ = source()
        .events
        .send(Event::GameStarted(GameStarted(session("s1"))));
    window.settle_until("covered", |window| {
        state(window).as_deref() == Some("covered")
            && window
                .find("data-cs", "game-curtain")
                .is_some_and(|curtain| window.text(curtain).contains("opens the quick menu"))
    });
    // It takes the pad: back lifts it.
    window.pad(Action::Back);
    window.settle_until("lifted by back", |window| {
        state(window).as_deref() == Some("open")
    });
    // Another start brings it down; the last exit lifts it.
    let _ = source()
        .events
        .send(Event::GameStarted(GameStarted(session("s2"))));
    window.settle_until("covered again", |window| {
        state(window).as_deref() == Some("covered")
    });
    let _ = source()
        .events
        .send(Event::GameExited(GameExited(session("s1"))));
    let _ = source()
        .events
        .send(Event::GameExited(GameExited(session("s2"))));
    window.settle_until("lifted by the exit", |window| {
        state(window).as_deref() == Some("open")
    });
}
