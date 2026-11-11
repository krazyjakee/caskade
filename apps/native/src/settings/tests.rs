//! Settings (#103): its sections, search, schema-driven panels, drafts,
//! reset and unsaved-changes guard; ported from the webview's
//! `sectionNav`, `settingsSearch` and `searchCoverage` tests.

use super::search::{INDEX, matches, search_settings, section_words};
use super::*;
use crate::frame::tests::{ONE, START, Window};
use crate::router::SETTINGS_TABS;
use dioxus_native::prelude::keyboard_types::Key;

// ── The tables ──

#[test]
fn the_sections_are_the_routers_in_the_lists_order() {
    let ids: Vec<&str> = SECTIONS.iter().map(|section| section.id).collect();
    assert_eq!(ids, SETTINGS_TABS);
    // Grouped in the groups' order.
    let groups: Vec<&str> = SECTIONS.iter().map(|section| section.group).collect();
    let mut seen: Vec<&str> = groups.clone();
    seen.dedup();
    let order: Vec<&str> = GROUPS.iter().map(|group| group.id).collect();
    assert_eq!(seen, order);
    for id in SETTINGS_TABS {
        assert!(!section_words(id).is_empty(), "{id} has search words");
    }
}

/// The search index is the webview's entry for entry: the same sections,
/// sub-sections and labels (so the same translations).
#[test]
fn the_search_index_is_the_webviews() {
    let source = include_str!("../../../desktop/src/views/settings/searchIndex.ts");
    let table = &source[source.find("export const settingsSearchIndex").unwrap()..];
    // Each entry: its tab, and its sub-section when it names one before
    // its label.
    let theirs: Vec<(&str, Option<&str>)> = table
        .match_indices("tab: \"")
        .map(|(at, found)| {
            let rest = &table[at + found.len()..];
            let tab = rest.split('"').next().unwrap();
            let entry = &rest[..rest.find("label:").unwrap()];
            let section = entry
                .split("section: \"")
                .nth(1)
                .map(|rest| rest.split('"').next().unwrap());
            (tab, section)
        })
        .collect();
    let ours: Vec<(&str, Option<&str>)> = INDEX
        .iter()
        .map(|entry| (entry.tab, entry.section))
        .collect();
    assert_eq!(ours, theirs);
    for entry in INDEX {
        let label = (entry.label)();
        assert!(
            source.contains(&format!("msg`{label}`"))
                || source.contains(&format!("message: \"{label}\"")),
            "{label:?} is the webview's label"
        );
    }
}

// ── Search ──

fn every() -> Vec<&'static Section> {
    SECTIONS.iter().collect()
}

fn titles(query: &str, sections: &[&Section]) -> Vec<(String, Option<String>)> {
    search_settings(query, sections)
        .into_iter()
        .map(|result| (result.title, result.within))
        .collect()
}

#[test]
fn matching_ignores_case_accents_and_order_and_needs_every_word() {
    assert!(matches("ÉCRAN", &["ecran"]));
    assert!(matches("updates game", &["Game updates"]));
    assert!(!matches("game saves", &["Game updates"]));
    assert!(!matches("  ", &["anything"]));
}

#[test]
fn finds_saves_in_devices_sections_first_then_preferences_names_before_words() {
    let found = titles("save", &every());
    assert_eq!(found[0], ("Devices".into(), None), "{found:?}");
    assert!(
        found.contains(&("Saves".into(), Some("Devices".into()))),
        "{found:?}"
    );
    // A preference named by the query comes before one found by its words.
    let named = found
        .iter()
        .position(|(title, _)| title == "Saves")
        .unwrap();
    let worded = found
        .iter()
        .position(|(title, _)| title == "Versions to keep")
        .unwrap();
    assert!(named < worded, "{found:?}");
}

#[test]
fn tells_app_updates_from_game_updates_and_opens_a_sub_section() {
    let found = titles("updates", &every());
    assert!(found.contains(&("Caskade updates".into(), Some("General".into()))));
    assert!(found.contains(&("Game updates".into(), Some("General".into()))));
    let romm = search_settings("RomM servers", &every());
    let result = romm
        .iter()
        .find(|result| result.title == "RomM servers")
        .unwrap();
    assert_eq!((result.tab, result.section), ("emulators", Some("romm")));
    assert!(search_settings("nothing matches this", &every()).is_empty());
}

#[test]
fn a_standard_account_finds_only_the_sections_it_opens() {
    let editable = caskade_ui_data::Editable::Only(vec!["appearance".into()]);
    let shown: Vec<&Section> = SECTIONS
        .iter()
        .filter(|section| opens(&editable, section.id))
        .collect();
    let ids: Vec<&str> = shown.iter().map(|section| section.id).collect();
    assert_eq!(
        ids,
        [
            "appearance",
            "devices",
            "streaming",
            "profile",
            "statistics",
            "about"
        ]
    );
    // Backup is an admin's: neither it nor its preferences are found.
    assert!(
        search_settings("backup", &shown)
            .iter()
            .all(|result| result.tab != "backup")
    );
}

// ── The view ──

fn open(href: &'static str) -> Window {
    *START.lock().unwrap_or_else(|e| e.into_inner()) = href;
    let mut window = Window::open();
    // Other tests' windows open on Home.
    *START.lock().unwrap_or_else(|e| e.into_inner()) = "/";
    window.settle_until("Settings", |window| {
        window.find("data-cs", "settings-nav").is_some()
    });
    window
}

fn selected_tab(window: &Window) -> String {
    let tab = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| window.attr(tab, "aria-selected").as_deref() == Some("true"))
        .expect("a selected section");
    window.text(tab)
}

fn tab(window: &Window, label: &str) -> blitz_dom::NodeId {
    window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| window.text(tab).trim() == label)
        .unwrap_or_else(|| panic!("no section {label}"))
}

#[test]
fn lists_every_section_in_groups_each_tab_tied_to_its_panel() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let window = open("/settings?tab=appearance");
    let nav = window.get("data-cs", "settings-nav");
    assert_eq!(
        window.attr(nav, "aria-orientation").as_deref(),
        Some("vertical")
    );
    let labels: Vec<String> = window
        .find_all("role", "tab")
        .into_iter()
        .map(|tab| window.text(tab).trim().to_owned())
        .collect();
    let expected: Vec<String> = SECTIONS.iter().map(|section| (section.label)()).collect();
    assert_eq!(labels, expected);
    let groups: Vec<String> = window
        .find_all("data-part", "group")
        .into_iter()
        .map(|group| window.text(group).trim().to_owned())
        .collect();
    assert_eq!(
        groups,
        [
            "App",
            "Library",
            "Games & play",
            "Devices & streaming",
            "People",
            "System"
        ]
    );
    assert_eq!(selected_tab(&window).trim(), "Appearance");
    let panel = window.get("role", "tabpanel");
    assert_eq!(
        window.attr(panel, "aria-labelledby").as_deref(),
        Some("settings-tab-appearance")
    );
}

#[test]
fn opens_a_section_in_place_by_click_arrows_and_a_pads_bumpers() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/settings?tab=general");
    // A click opens it in place: Back leaves Settings.
    let storage = tab(&window, "Storage");
    window.click(storage);
    window.settle_until("Storage", |window| selected_tab(window).trim() == "Storage");
    assert!(matches!(
        window.route(),
        Route::Settings { tab: "storage", .. }
    ));
    // Arrows move and open, over the group headings.
    window.focus(tab(&window, "Storage"));
    window.key(Key::Named(NamedKey::ArrowDown));
    window.settle_until("Installer", |window| {
        selected_tab(window).trim() == "Installer"
    });
    window.key(Key::Named(NamedKey::ArrowDown));
    window.settle_until("Emulation", |window| {
        selected_tab(window).trim() == "Emulation"
    });
    window.key(Key::Named(NamedKey::Home));
    window.settle_until("General", |window| selected_tab(window).trim() == "General");
    // A pad's RB and LB.
    window.pad(caskade_ui_input::Action::TabNext);
    window.settle_until("Appearance", |window| {
        selected_tab(window).trim() == "Appearance"
    });
    window.pad(caskade_ui_input::Action::TabPrev);
    window.settle_until("General", |window| selected_tab(window).trim() == "General");
}

/// The schema sections render from the core's own schemas: a row per
/// field, titled as the schema titles it.
#[test]
fn schema_sections_render_every_field_of_the_cores_schemas() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let settings = crate::frame::tests::source().settings.clone();
    let sections = settings.sections().unwrap();
    for (href, namespace) in [
        ("/settings?tab=installer", "core.installer"),
        ("/settings?tab=general", "core.updates"),
    ] {
        let schema = &sections
            .iter()
            .find(|section| section.namespace == namespace)
            .unwrap()
            .schema;
        let expected: Vec<String> = schema::fields(schema)
            .unwrap()
            .iter()
            .map(|(key, field)| schema::title(key, field))
            .collect();
        assert!(!expected.is_empty(), "{namespace} has fields");
        let mut window = open(href);
        window.settle_until(namespace, |window| {
            window.find("data-settings", namespace).is_some()
                && window.find_all("data-cs", "setting-row").len() >= expected.len()
        });
        let root = window.get("data-settings", namespace);
        let shown: Vec<String> = window
            .find_all("data-cs", "setting-row")
            .into_iter()
            .filter(|&row| is_inside(&window, row, root))
            .map(|row| label_of(&window, row))
            .collect();
        assert_eq!(shown, expected, "{namespace}");
        assert!(
            window.text(root).contains("Reset to defaults…"),
            "{namespace} offers a reset"
        );
    }
}

fn is_inside(window: &Window, node: blitz_dom::NodeId, root: blitz_dom::NodeId) -> bool {
    let doc = window.doc.inner.borrow();
    let mut at = Some(node);
    while let Some(here) = at {
        if here == root {
            return true;
        }
        at = doc.get_node(here).and_then(|node| node.parent);
    }
    false
}

fn label_of(window: &Window, row: blitz_dom::NodeId) -> String {
    let doc = window.doc.inner.borrow();
    let mut found = String::new();
    let mut stack = vec![row];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        if node.attr(blitz_dom::LocalName::from("data-part")) == Some("label") && found.is_empty() {
            found = node.text_content().trim().to_owned();
        }
        stack.extend(node.children.iter().rev().copied());
    }
    found
}

#[test]
fn search_opens_the_first_result_and_highlights_its_row() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/settings?tab=general");
    let field = window
        .find_all("aria-label", "Search settings")
        .into_iter()
        .next()
        .expect("the search field");
    window.focus(field);
    window.type_text("nothing matches this");
    window.settle_until("no results", |window| {
        window
            .find_all("role", "status")
            .iter()
            .any(|&status| window.text(status).contains("No settings match"))
    });
    // Escape clears it, and the list comes back.
    window.key(Key::Named(NamedKey::Escape));
    window.settle_until("the list", |window| {
        window.find("data-cs", "settings-nav").is_some()
    });
    // Enter opens the first result: a preference of another section.
    window.focus(field);
    window.type_text("concurrent downloads");
    window.settle_until("a result", |window| {
        window
            .find("aria-label", "Concurrent downloads in Installer")
            .is_some()
    });
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("Installer", |window| {
        selected_tab(window).trim() == "Installer"
    });
    // Its row lights up once the schema shows it, and focus goes into it.
    window.settle_until("the found row", |window| {
        window.find("data-found", "").is_some()
    });
    let row = window.get("data-found", "");
    assert_eq!(label_of(&window, row), "Concurrent downloads");
    let focused = window.focused().expect("focus");
    assert!(
        is_inside(&window, focused, row),
        "focus is in the found row"
    );
}

/// A schema field saves through the core, and Reset to defaults says what
/// goes back first and resets it.
#[test]
fn a_schema_switch_saves_and_reset_puts_it_back() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let settings = crate::frame::tests::source().settings.clone();
    let before = caskade_app::runtime::block_on(settings.get("core.updates"))
        .unwrap()
        .value;
    let mut window = open("/settings?tab=general");
    window.settle_until("game updates", |window| {
        window.find("data-settings", "core.updates").is_some()
            && !window.find_all("role", "switch").is_empty()
    });
    let root = window.get("data-settings", "core.updates");
    let switch = window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| is_inside(&window, switch, root))
        .expect("a switch in game updates");
    let was = window.attr(switch, "aria-checked");
    // Below the fold of the test's window: by keyboard, as a click there
    // misses.
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("saved", |window| {
        let after = caskade_app::runtime::block_on(settings.get("core.updates"))
            .unwrap()
            .value;
        after != before && window.attr(switch, "aria-checked") != was
    });
    // Reset to defaults lists what changes, then resets it.
    let reset = window
        .find_all("aria-label", "Reset update settings")
        .into_iter()
        .next()
        .expect("the reset button");
    window.focus(reset);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("the reset question", |window| {
        window.find("role", "alertdialog").is_some_and(|dialog| {
            window
                .text(dialog)
                .contains("These settings go back to their defaults:")
        })
    });
    let confirm = window
        .find_all("data-variant", "danger")
        .into_iter()
        .find(|&button| window.text(button).trim() == "Reset")
        .expect("Reset");
    window.focus(confirm);
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("reset", |window| {
        window.find("role", "alertdialog").is_none()
            && caskade_app::runtime::block_on(settings.get("core.updates"))
                .unwrap()
                .value
                == before
    });
}

/// Typing on Settings searches settings, not games, and a pad's Search
/// goes to Settings' search (#56).
#[test]
fn typing_and_a_pads_search_go_to_settings_search() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/settings?tab=general");
    window.focus(tab(&window, "General"));
    window.type_text("updates");
    window.settle_until("settings results", |window| {
        window
            .find("aria-label", "Game updates in General")
            .is_some()
    });
    assert!(
        window.find("data-cs", "type-search").is_none(),
        "no games overlay"
    );
    let field = window
        .find_all("aria-label", "Search settings")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(window.attr(field, "value").as_deref(), Some("updates"));
    window.key(Key::Named(NamedKey::Escape));
    window.focus(tab(&window, "General"));
    window.pad(caskade_ui_input::Action::Search);
    assert_eq!(window.focused(), Some(field));
}

// ── Unsaved changes ──

mod guard {
    use super::super::unsaved::{
        UnsavedChangesGuard, UnsavedForm, holds_close, use_unsaved_changes,
    };
    use crate::input::InputRoot;
    use crate::router::{Route, Router, RouterRoot, use_router};
    use blitz_dom::{DocumentConfig, LocalName, NodeId};
    use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use dioxus_native::DioxusDocument;
    use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
    use dioxus_native::prelude::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    thread_local! {
        static ROUTER: Cell<Option<Router>> = const { Cell::new(None) };
        static DISCARDED: Cell<u32> = const { Cell::new(0) };
        static SAVED: Cell<u32> = const { Cell::new(0) };
    }

    #[component]
    fn Form() -> Element {
        ROUTER.with(|cell| cell.set(Some(use_router())));
        use_unsaved_changes(
            true,
            UnsavedForm {
                label: "My profile".into(),
                save: Rc::new(|| {
                    Box::pin(async {
                        SAVED.with(|saved| saved.set(saved.get() + 1));
                        true
                    })
                }),
                discard: Rc::new(|| DISCARDED.with(|count| count.set(count.get() + 1))),
            },
        );
        rsx! { p { "A form with edits" } }
    }

    fn app() -> Element {
        rsx! {
            InputRoot {
                RouterRoot { start: "/settings?tab=profile",
                    UnsavedChangesGuard { Form {} }
                }
            }
        }
    }

    struct Window {
        doc: DioxusDocument,
    }

    impl Window {
        fn open() -> Self {
            let mut doc = DioxusDocument::new(
                VirtualDom::new(app),
                DocumentConfig {
                    viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                    ..Default::default()
                },
            );
            doc.initial_build();
            let mut window = Self { doc };
            window.settle();
            window
        }

        fn settle(&mut self) {
            for _ in 0..30 {
                blitz_dom::Document::poll(&mut self.doc, None);
                self.doc.inner.borrow_mut().resolve(0.0);
                std::thread::sleep(Duration::from_millis(3));
            }
        }

        fn router(&self) -> Router {
            ROUTER.with(Cell::get).expect("the form rendered")
        }

        fn dialog(&self) -> Option<String> {
            let doc = self.doc.inner.borrow();
            let mut found = None;
            doc.visit(|_, node| {
                if node.attr(LocalName::from("role")) == Some("alertdialog") {
                    found = Some(node.text_content());
                }
            });
            found
        }

        fn button(&self, text: &str) -> NodeId {
            let doc = self.doc.inner.borrow();
            let mut found = None;
            doc.visit(|id, node| {
                if node.attr(LocalName::from("data-cs")) == Some("button")
                    && node.text_content().trim() == text
                {
                    found = Some(id);
                }
            });
            found.unwrap_or_else(|| panic!("no {text} button"))
        }

        /// A pad's action, as the app's events bring it.
        fn pad(&mut self, action: caskade_ui_input::Action) {
            let _ = crate::shell::events().send(caskade_app::events::Event::InputActionEvent(
                caskade_app::commands::input::InputActionEvent(caskade_input::InputAction {
                    action,
                    source: caskade_input::Source::Pad,
                    repeat: false,
                    glyphs: caskade_ui_input::GlyphFamily::Xbox,
                }),
            ));
            self.settle();
        }

        /// Presses `text`'s button with Enter.
        fn press(&mut self, text: &str) {
            let node = self.button(text);
            self.doc.inner.borrow_mut().set_focus_to(node);
            for state in [KeyState::Pressed, KeyState::Released] {
                let event = BlitzKeyEvent {
                    key: Key::Named(NamedKey::Enter),
                    code: Code::Enter,
                    modifiers: Modifiers::empty(),
                    location: Location::Standard,
                    is_auto_repeating: false,
                    is_composing: false,
                    state,
                    text: None,
                };
                let event = match state {
                    KeyState::Pressed => UiEvent::KeyDown(event),
                    KeyState::Released => UiEvent::KeyUp(event),
                };
                blitz_dom::Document::handle_ui_event(&mut self.doc, event);
                self.settle();
            }
        }
    }

    fn on_profile(route: &Route) -> bool {
        matches!(route, Route::Settings { tab: "profile", .. })
    }

    #[test]
    fn leaving_a_section_with_unsaved_edits_asks_and_does_what_the_answer_says() {
        let _one = crate::input::tests::ONE_WINDOW
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _runtime = caskade_app::runtime::handle().enter();
        let mut window = Window::open();
        let router = window.router();
        assert!(on_profile(&router.peek()), "{:?}", router.peek());
        // Another page waits on the question; the page stays.
        router.navigate("/library");
        window.settle();
        assert!(on_profile(&router.peek()));
        let question = window.dialog().expect("the question");
        assert!(question.contains("Save your changes?"), "{question}");
        assert!(question.contains("Leaving this section without saving loses them."));
        assert!(question.contains("My profile"));
        // Stay: nothing goes.
        window.press("Stay");
        assert!(window.dialog().is_none());
        assert!(on_profile(&router.peek()));
        assert_eq!(router.held(), None);
        // Within the same section a navigation doesn't ask.
        router.replace("/settings?tab=profile");
        window.settle();
        assert!(window.dialog().is_none());
        // Discard drops the edits and goes.
        router.navigate("/settings?tab=about");
        window.settle();
        window.press("Discard");
        assert!(matches!(
            router.peek(),
            Route::Settings { tab: "about", .. }
        ));
        assert_eq!(DISCARDED.with(Cell::get), 1);
        // A pad's Back while the question is open answers Stay, once: the
        // question is Back's own target, and the page doesn't go back too.
        router.navigate("/library");
        window.settle();
        assert!(window.dialog().is_some());
        window.pad(caskade_ui_input::Action::Back);
        assert!(window.dialog().is_none(), "Back closed the question");
        assert!(
            matches!(router.peek(), Route::Settings { tab: "about", .. }),
            "and didn't go back: {:?}",
            router.peek()
        );
        assert_eq!(router.held(), None);
        // Back asks too, and Save saves, then goes.
        assert!(router.back());
        window.settle();
        assert!(window.dialog().is_some());
        window.press("Save");
        assert_eq!(SAVED.with(Cell::get), 1);
        assert!(on_profile(&router.peek()));
        // A close of the window that would quit asks first.
        assert!(holds_close());
        window.settle();
        let question = window.dialog().expect("the close question");
        assert!(
            question.contains("Closing Caskade without saving loses them."),
            "{question}"
        );
        window.press("Stay");
        assert!(window.dialog().is_none());
    }
}

mod appearance;
mod discovery;
mod general;
mod launchers;
mod storage;
mod stores;
