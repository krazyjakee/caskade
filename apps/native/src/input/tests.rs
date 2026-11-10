//! The input layer in a real Blitz document without a window: a sidebar, a
//! grid, a dialog and a field laid out as the webview's tests place them,
//! driven by real key events and by pad actions as the app's events bring
//! them.

use super::*;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_app::commands::input::InputActionEvent;
use caskade_input::{InputAction, Source};
use caskade_ui_input::{Action, GlyphFamily};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// One window at a time: pad actions go to every input root listening.
static ONE_WINDOW: Mutex<()> = Mutex::new(());

/// What the demo's controls did.
static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn did(what: impl Into<String>) {
    DONE.lock().unwrap().push(what.into());
}

/// Absolutely placed, as the webview's tests give jsdom boxes.
fn at(x: u32, y: u32, w: u32, h: u32) -> String {
    format!(
        "position: absolute; box-sizing: border-box; margin: 0; left: {x}px; top: {y}px; width: {w}px; height: {h}px;"
    )
}

fn app() -> Element {
    rsx! {
        InputRoot { console: true, Demo {} }
    }
}

#[component]
fn Demo() -> Element {
    let mut open = use_signal(|| false);
    let mut text = use_signal(String::new);
    let input = use_input();
    use_hook(move || input.on_back(|| did("back")));
    rsx! {
        div { "data-focus-section": "sidebar",
            for (i, label) in ["Home", "Library", "Settings"].iter().enumerate() {
                button { style: at(0, i as u32 * 50, 200, 40), "{label}" }
            }
        }
        div { "data-focus-section": "main",
            div { "data-focus-section": "grid",
                for i in 0..6u32 {
                    button {
                        style: at(300 + (i % 3) * 150, 100 + (i / 3) * 100, 140, 80),
                        onclick: move |_| did(format!("Card {i}")),
                        "Card {i}"
                    }
                }
            }
            button {
                style: at(300, 320, 140, 40),
                onclick: move |_| open.set(true),
                "Open dialog"
            }
            form { onsubmit: move |event: FormEvent| did(format!("search {}", event.value())),
                input {
                    style: at(300, 380, 300, 40),
                    aria_label: "Search",
                    value: "{text}",
                    oninput: move |event: FormEvent| text.set(event.value()),
                }
            }
        }
        if open() {
            Confirm { close: move |_| open.set(false) }
        }
    }
}

/// A native dialog: rendered unfocused; Back closes it.
#[component]
fn Confirm(close: EventHandler<()>) -> Element {
    let mut me = use_signal(|| None::<NodeId>);
    use_input_action(
        Action::Back,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            close.call(());
            true
        },
    );
    rsx! {
        div {
            role: "dialog",
            style: "position: fixed; left: 0; top: 0; width: 1600px; height: 900px;",
            onmounted: move |event| {
                me.set(event.data().downcast::<dioxus_native::NodeHandle>().map(|h| h.node_id()))
            },
            button { style: at(600, 400, 100, 40), onclick: move |_| close.call(()), "Yes" }
            button {
                style: at(720, 400, 100, 40),
                "data-focus-default": "true",
                onclick: move |_| close.call(()),
                "No"
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
                viewport: Some(Viewport::new(1600, 900, 1.0, ColorScheme::Light)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.settle();
        window
    }

    /// Renders, lays out, and lets the input layer's frames pass.
    fn settle(&mut self) {
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            blitz_dom::Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn key(&mut self, key: Key) {
        for state in [KeyState::Pressed, KeyState::Released] {
            let event = BlitzKeyEvent {
                key: key.clone(),
                code: Code::Unidentified,
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
        }
        self.settle();
    }

    fn pad(&mut self, action: Action) {
        let _ = crate::shell::events().send(caskade_app::events::Event::InputActionEvent(
            InputActionEvent(InputAction {
                action,
                source: Source::Pad,
                repeat: false,
                glyphs: GlyphFamily::Xbox,
            }),
        ));
        self.settle();
    }

    fn press(&mut self, driver: Driver, action: Action) {
        match driver {
            Driver::Pad => self.pad(action),
            Driver::Keyboard => self.key(match action {
                Action::NavUp => Key::Named(NamedKey::ArrowUp),
                Action::NavDown => Key::Named(NamedKey::ArrowDown),
                Action::NavLeft => Key::Named(NamedKey::ArrowLeft),
                Action::NavRight => Key::Named(NamedKey::ArrowRight),
                Action::Confirm => Key::Named(NamedKey::Enter),
                Action::Back => Key::Named(NamedKey::Escape),
                other => panic!("no key for {other:?}"),
            }),
        }
    }

    fn focused_node(&self) -> Option<NodeId> {
        self.doc.inner.borrow().get_focussed_node_id()
    }

    /// The focused element's text.
    fn focused(&self) -> String {
        let doc = self.doc.inner.borrow();
        doc.get_focussed_node_id()
            .and_then(|id| doc.get_node(id))
            .map(|node| node.text_content())
            .unwrap_or_default()
    }

    fn ringed(&self) -> bool {
        let doc = self.doc.inner.borrow();
        doc.get_focussed_node_id()
            .and_then(|id| doc.get_node(id))
            .is_some_and(|node| {
                node.attr(blitz_dom::LocalName::from("data-focus-visible"))
                    .is_some()
            })
    }

    fn find(&self, text: &str) -> NodeId {
        let doc = self.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if found.is_none() && node.is_element() && node.text_content() == text {
                let leaf = node
                    .children
                    .iter()
                    .all(|child| doc.get_node(*child).is_none_or(|child| !child.is_element()));
                if leaf {
                    found = Some(id);
                }
            }
        });
        found.unwrap_or_else(|| panic!("nothing reads {text:?}"))
    }

    /// A mouse click at the middle of `text`'s box.
    fn click(&mut self, text: &str) {
        let node = self.find(text);
        let rect = self
            .doc
            .inner
            .borrow()
            .get_client_bounding_rect(node)
            .unwrap();
        let (x, y) = (
            (rect.x + rect.width / 2.0) as f32,
            (rect.y + rect.height / 2.0) as f32,
        );
        for event in [mouse(x, y, true), mouse(x, y, false)] {
            blitz_dom::Document::handle_ui_event(&mut self.doc, event);
        }
        self.settle();
    }
}

fn mouse(x: f32, y: f32, down: bool) -> UiEvent {
    use blitz_traits::events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords,
        PointerDetails,
    };
    let event = BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons: if down {
            MouseEventButtons::from(MouseEventButton::Main)
        } else {
            MouseEventButtons::None
        },
        mods: Modifiers::empty(),
        details: PointerDetails::default(),
        element: Default::default(),
        active_pointers: Default::default(),
    };
    if down {
        UiEvent::PointerDown(event)
    } else {
        UiEvent::PointerUp(event)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Driver {
    Pad,
    Keyboard,
}

#[test]
fn a_pad_and_a_keyboard_each_work_the_grid_the_sidebar_and_a_dialog() {
    let _one = ONE_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    for driver in [Driver::Pad, Driver::Keyboard] {
        DONE.lock().unwrap().clear();
        let mut window = Window::open();
        // Nothing focused: the content's first control.
        window.press(driver, Action::NavDown);
        assert_eq!(window.focused(), "Card 0", "{driver:?}");
        assert!(window.ringed(), "keys and pads show the focus ring");
        window.press(driver, Action::NavRight);
        assert_eq!(window.focused(), "Card 1", "{driver:?}");
        window.press(driver, Action::NavDown);
        assert_eq!(window.focused(), "Card 4", "{driver:?}");
        // Into the sidebar and back to the card focus left.
        window.press(driver, Action::NavLeft);
        window.press(driver, Action::NavLeft);
        assert_eq!(window.focused(), "Settings");
        window.press(driver, Action::NavRight);
        assert_eq!(window.focused(), "Card 3");
        window.press(driver, Action::Confirm);
        assert_eq!(
            DONE.lock().unwrap().last().map(String::as_str),
            Some("Card 3")
        );
        // A dialog: focus goes in on its default, stays in, and comes back.
        window.press(driver, Action::NavDown);
        assert_eq!(window.focused(), "Open dialog");
        window.press(driver, Action::Confirm);
        assert_eq!(window.focused(), "No", "{driver:?}");
        window.press(driver, Action::NavLeft);
        assert_eq!(window.focused(), "Yes");
        window.press(driver, Action::NavLeft);
        assert_eq!(window.focused(), "Yes", "nothing outside the dialog");
        window.press(driver, Action::Back);
        assert_eq!(window.focused(), "Open dialog", "{driver:?}");
        // With no overlay, back goes back.
        window.press(driver, Action::Back);
        assert_eq!(
            DONE.lock().unwrap().last().map(String::as_str),
            Some("back")
        );
    }
}

#[test]
fn a_click_focuses_the_control_without_a_ring_and_a_pad_types_in_a_field() {
    let _one = ONE_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    DONE.lock().unwrap().clear();
    let mut window = Window::open();
    window.click("Card 1");
    assert_eq!(window.focused(), "Card 1");
    assert!(!window.ringed(), "a click shows no ring");
    assert_eq!(
        DONE.lock().unwrap().last().map(String::as_str),
        Some("Card 1")
    );
    // A pad on the field opens the on-screen keyboard; it types q, w and
    // submits with Start.
    window.click("Open dialog");
    window.pad(Action::Back);
    window.pad(Action::NavDown);
    let field = window.focused_node();
    assert_eq!(
        window
            .doc
            .inner
            .borrow()
            .get_node(field.unwrap())
            .and_then(|node| node.element_data())
            .map(|element| element.name.local.to_string()),
        Some("input".into())
    );
    window.pad(Action::Confirm);
    window.find("Done");
    window.pad(Action::Confirm);
    window.pad(Action::NavRight);
    window.pad(Action::Confirm);
    assert_eq!(window.focused_node(), field, "the field keeps focus");
    window.pad(Action::Menu);
    assert_eq!(
        DONE.lock().unwrap().last().map(String::as_str),
        Some("search qw")
    );
}
