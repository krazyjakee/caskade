//! Settings → Library Discovery (#104), ported from the webview's
//! `discovery` tests: the preferences read and save (on, schedule,
//! watching, play history, the system's checks, deep scans and their
//! folders), off dims them and stops Scan now, the status says the phase,
//! sources have names, ignored results are allowed again, hidden games are
//! shown again, and the review list accepts or ignores several results or
//! one by a right click. No test scans: a scan would import the fixture
//! machine's games into the library other tests read.

use super::{is_inside, label_of, open};
use crate::frame::tests::{ONE, Window, source};
use crate::input::InputRoot;
use crate::settings::launchers::source_name;
use caskade_app::discovery::ReviewItem;
use caskade_ui_input::Action;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use serde_json::{Value, json};
use std::sync::Mutex;

fn saved(namespace: &str) -> Value {
    caskade_app::runtime::block_on(source().settings.get(namespace))
        .unwrap()
        .value
}

struct Restore(Vec<(&'static str, Value)>);

impl Restore {
    fn new() -> Self {
        Self(
            ["core.discovery", "discovery.import"]
                .into_iter()
                .map(|namespace| (namespace, saved(namespace)))
                .collect(),
        )
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        for (namespace, value) in self.0.drain(..) {
            caskade_app::runtime::block_on(source().settings.update(namespace, move |_| value))
                .unwrap();
        }
    }
}

fn set(key: &str, value: Value) {
    let key = key.to_owned();
    caskade_app::runtime::block_on(source().settings.update("core.discovery", move |mut now| {
        now[key] = value;
        now
    }))
    .unwrap();
}

fn discovery() -> Window {
    let mut window = open("/settings?tab=library-discovery");
    window.settle_until("Library Discovery", |window| {
        window.find("data-settings", "library-discovery").is_some()
            && window
                .find_all("role", "status")
                .iter()
                .any(|&status| window.text(status).contains("imported"))
    });
    window
}

fn row(window: &Window, label: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(window, row) == label)
        .unwrap_or_else(|| panic!("no row {label}"))
}

fn switch(window: &Window, label: &str) -> blitz_dom::NodeId {
    let row = row(window, label);
    window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| is_inside(window, switch, row))
        .unwrap()
}

fn button(window: &Window, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn space(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Character(" ".into()));
}

#[test]
fn sources_have_names_and_unknown_ones_show_as_they_are() {
    assert_eq!(source_name("steam"), "Steam");
    assert_eq!(source_name("epic.app"), "Epic Games Launcher");
    assert_eq!(source_name("flatpak"), "Flatpak apps");
    assert_eq!(source_name("custom.thing"), "custom.thing");
}

#[test]
fn the_preferences_read_and_save_and_off_stops_scans() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("enabled", json!(true));
    set("watchFolders", json!(false));
    set("backgroundRefresh", json!(true));
    let mut window = discovery();
    // The status says the phase and the counts.
    let status = window
        .find_all("role", "status")
        .into_iter()
        .find(|&status| window.text(status).contains("imported"))
        .unwrap();
    assert!(
        window.text(status).contains("Idle"),
        "{}",
        window.text(status)
    );
    // Switches by Space and a pad's A.
    let watch = switch(&window, "Watch library folders");
    space(&mut window, watch);
    window.settle_until("watching", |_| {
        saved("core.discovery")["watchFolders"] == json!(true)
    });
    let startup = switch(&window, "Full scan on startup");
    let was = saved("core.discovery")["scanOnStartup"].clone();
    window.focus(startup);
    window.pad(Action::Confirm);
    window.settle_until("startup", |_| {
        saved("core.discovery")["scanOnStartup"] != was
    });
    // The refresh interval's select.
    let interval = window.get("aria-label", "Refresh interval");
    let trigger = window
        .find_all("data-part", "trigger")
        .into_iter()
        .find(|&trigger| is_inside(&window, trigger, interval) || trigger == interval)
        .unwrap_or(interval);
    window.focus(trigger);
    window.key(Key::Named(NamedKey::ArrowDown));
    window.settle_until("the list", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(Key::Named(NamedKey::End));
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("weekly", |_| {
        saved("core.discovery")["refreshInterval"] == json!("7d")
    });
    // Play history.
    let history = switch(&window, "Import existing play history");
    let was = saved("discovery.import")["seedPlaytime"].clone();
    space(&mut window, history);
    window.settle_until("history", |_| {
        saved("discovery.import")["seedPlaytime"] != was
    });
    // A deep scan.
    let custom = switch(&window, "Custom folders");
    space(&mut window, custom);
    window.settle_until("custom folders", |_| {
        saved("core.discovery")["deepScans"]["customFolders"] == json!(true)
    });
    // Off dims the rest, says why, and Scan now waits.
    let enabled = switch(&window, "Enable library discovery");
    space(&mut window, enabled);
    window.settle_until("off", |window| {
        saved("core.discovery")["enabled"] == json!(false)
            && window.find("role", "note").is_some_and(|note| {
                window.text(note).contains(
                    "Library discovery is off, so the settings below don't apply until it's on.",
                )
            })
    });
    let options = window.get("data-part", "discovery-options");
    assert_eq!(
        window.attr(options, "aria-disabled").as_deref(),
        Some("true")
    );
    let scan = button(&window, "Scan now");
    assert_eq!(window.attr(scan, "aria-disabled").as_deref(), Some("true"));
    let watch = switch(&window, "Watch library folders");
    assert_eq!(window.attr(watch, "aria-disabled").as_deref(), Some("true"));
}

/// The system's checks come from the controller for this OS; one turned
/// off is kept per OS.
#[test]
fn a_system_check_turns_off_for_this_os() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("enabled", json!(true));
    let snapshot =
        caskade_app::runtime::block_on(source().discovery.as_ref().unwrap().status()).unwrap();
    let check = snapshot
        .checks
        .first()
        .expect("the fixture OS has checks")
        .clone();
    let mut window = discovery();
    let checkbox = window
        .find_all("role", "checkbox")
        .into_iter()
        .find(|&checkbox| {
            window
                .attr(checkbox, "aria-labelledby")
                .and_then(|id| window.find("id", &id))
                .is_some_and(|label| window.text(label).trim() == check.label)
        })
        .expect("the check");
    assert_eq!(
        window.attr(checkbox, "aria-checked").as_deref(),
        Some("true")
    );
    space(&mut window, checkbox);
    window.settle_until("off", |_| {
        saved("core.discovery")["osChecks"][&snapshot.os][&check.id] == json!(false)
    });
}

/// Folders for deep scans: added by typing, listed, removed by name.
#[test]
fn folders_are_added_and_removed() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set("enabled", json!(true));
    set("scanPaths", json!([]));
    let mut window = discovery();
    let field = window
        .find_all("data-cs", "text-field")
        .into_iter()
        .find(|&field| label_of(&window, field) == "Folder to search")
        .unwrap();
    let input = window
        .find_all("data-part", "input")
        .into_iter()
        .find(|&input| is_inside(&window, input, field))
        .unwrap();
    window.focus(input);
    window.type_text("/games/old");
    let add = button(&window, "Add folder");
    window.focus(add);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("added", |window| {
        saved("core.discovery")["scanPaths"] == json!(["/games/old"])
            && window
                .text(window.get("data-part", "scan-paths"))
                .contains("/games/old")
    });
    let remove = window.get("aria-label", "Remove /games/old");
    window.focus(remove);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("removed", |_| {
        saved("core.discovery")["scanPaths"] == json!([])
    });
}

/// Ignored results list their title and source, and come back.
#[test]
fn an_ignored_result_is_allowed_again() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _restore = Restore::new();
    set(
        "ignoredEntries",
        json!([
            {"source": "steam", "key": "440", "title": "Team Fortress 2"},
            {"source": "flatpak", "key": "org.example.Old"}
        ]),
    );
    let mut window = discovery();
    let section = window.get("data-part", "ignored-results");
    let text = window.text(section);
    assert!(
        text.contains("Team Fortress 2") && text.contains("Steam"),
        "{text}"
    );
    // One without a title shows its record's key.
    assert!(
        text.contains("org.example.Old") && text.contains("Flatpak apps"),
        "{text}"
    );
    let allow = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| {
            is_inside(&window, button, section) && window.text(button).trim() == "Allow again"
        })
        .unwrap();
    window.focus(allow);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("allowed", |_| {
        saved("core.discovery")["ignoredEntries"]
            .as_array()
            .is_some_and(|entries| {
                entries.len() == 1 && entries[0]["key"] == json!("org.example.Old")
            })
    });
}

/// A hidden game is listed and shown again.
#[test]
fn a_hidden_game_is_shown_again() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let library = source().library.clone();
    let game = caskade_app::runtime::block_on(library.page(&Default::default()))
        .unwrap()
        .items
        .into_iter()
        .next()
        .unwrap();
    caskade_app::runtime::block_on(library.set_hidden(&game.id, true)).unwrap();
    struct Unhide(caskade_ui_data::Library, String);
    impl Drop for Unhide {
        fn drop(&mut self) {
            let _ = caskade_app::runtime::block_on(self.0.set_hidden(&self.1, false));
        }
    }
    let _unhide = Unhide(library.clone(), game.id.clone());
    let mut window = discovery();
    window.settle_until("the hidden game", |window| {
        window
            .find_all("data-part", "hidden-game")
            .iter()
            .any(|&row| window.text(row).contains(&game.title))
    });
    let show = window.get("aria-label", &format!("Show {} again", game.title));
    window.focus(show);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("shown", |window| {
        caskade_app::runtime::block_on(library.hidden(0, 50))
            .unwrap()
            .items
            .iter()
            .all(|hidden| hidden.id != game.id)
            && window
                .find_all("data-part", "hidden-game")
                .iter()
                .all(|&row| !window.text(row).contains(&game.title))
    });
}

// ── The review list, with results of its own ──

static DECIDED: Mutex<Vec<(Vec<String>, bool)>> = Mutex::new(Vec::new());

fn review_app() -> Element {
    let review = vec![
        ReviewItem {
            id: "a".into(),
            title: "Celeste".into(),
            sources: vec!["steam".into()],
            location: Some("/games/celeste".into()),
            evidence: vec!["An executable named like the folder".into()],
        },
        ReviewItem {
            id: "b".into(),
            title: "Hades".into(),
            sources: vec!["desktop".into(), "flatpak".into()],
            location: None,
            evidence: vec![],
        },
    ];
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            crate::settings::discovery::ReviewList {
                review,
                busy: false,
                on_decide: |decision: (Vec<String>, bool)| {
                    DECIDED.lock().unwrap_or_else(|e| e.into_inner()).push(decision)
                },
            }
        }
    }
}

fn decided() -> Vec<(Vec<String>, bool)> {
    DECIDED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[test]
fn the_review_list_accepts_several_and_ignores_one_by_a_right_click() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    DECIDED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let mut doc = DioxusDocument::new(
        VirtualDom::new(review_app),
        blitz_dom::DocumentConfig {
            viewport: Some(blitz_traits::shell::Viewport::new(
                1280,
                800,
                1.0,
                blitz_traits::shell::ColorScheme::Dark,
            )),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("the results", |window| {
        window.find_all("data-part", "review-item").len() == 2
    });
    let list = window.get("data-cs", "discovery-review");
    let text = window.text(list);
    assert!(text.contains("Sources: steam"), "{text}");
    assert!(text.contains("/games/celeste"), "{text}");
    let evidence = window.get("aria-label", "Evidence for Celeste");
    assert!(
        window
            .text(evidence)
            .contains("An executable named like the folder")
    );
    // Nothing selected: the buttons wait.
    let accept = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Accept selected (0)")
        .unwrap();
    assert_eq!(
        window.attr(accept, "aria-disabled").as_deref(),
        Some("true")
    );
    // Select all, then accept them.
    let all = window
        .find_all("role", "checkbox")
        .into_iter()
        .find(|&checkbox| {
            window
                .attr(checkbox, "aria-labelledby")
                .and_then(|id| window.find("id", &id))
                .is_some_and(|label| window.text(label).trim() == "Select all")
        })
        .unwrap();
    space(&mut window, all);
    let accept = window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Accept selected (2)")
        .expect("both selected");
    window.focus(accept);
    window.key(Key::Named(NamedKey::Enter));
    assert_eq!(decided(), [(vec!["a".to_owned(), "b".to_owned()], true)]);
    // A right click ignores one.
    let hades = window
        .find_all("data-part", "review-item")
        .into_iter()
        .find(|&item| window.text(item).contains("Hades"))
        .unwrap();
    window.right_click(hades);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
    let ignore = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains("Ignore"))
        .unwrap();
    window.focus(ignore);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("ignored", |_| decided().len() == 2);
    assert_eq!(decided()[1], (vec!["b".to_owned()], false));
}
