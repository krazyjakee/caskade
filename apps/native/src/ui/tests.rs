//! The components in a real Blitz document without a window (#93), inside
//! the input layer's root as in the app: each is pressed with a mouse, the
//! keyboard (Enter, Space as the APG has them) and a pad (A, through the
//! app's events), and read as a screen reader reads it (the AccessKit tree,
//! `crate::a11y_snapshot`). Their theming (anatomy and every built-in
//! theme) is checked by the gallery's screenshots and `crate::anatomy`.

use super::*;
use crate::input::InputRoot;
use blitz_dom::{DocumentConfig, LocalName, NodeId};
use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_app::commands::input::InputActionEvent;
use caskade_input::{InputAction, Source};
use caskade_ui_input::{Action, GlyphFamily};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::sync::Mutex;
use std::time::Duration;

/// What the demo's controls did.
static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn did(what: impl Into<String>) {
    DONE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(what.into());
}

fn take_done() -> Vec<String> {
    std::mem::take(&mut *DONE.lock().unwrap_or_else(|e| e.into_inner()))
}

pub(crate) struct Window {
    doc: DioxusDocument,
}

impl Window {
    pub(crate) fn open(root: fn() -> Element) -> Self {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(root),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.settle();
        take_done();
        window
    }

    /// Renders, lays out, and lets the input layer's jobs run.
    pub(crate) fn settle(&mut self) {
        for _ in 0..40 {
            blitz_dom::Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    pub(crate) fn id(&self, id: &str) -> NodeId {
        self.doc
            .inner
            .borrow()
            .get_element_by_id(id)
            .unwrap_or_else(|| panic!("no #{id}"))
    }

    pub(crate) fn attr(&self, id: &str, name: &str) -> Option<String> {
        let node = self.id(id);
        let doc = self.doc.inner.borrow();
        doc.get_node(node)
            .and_then(|node| node.attr(LocalName::from(name)))
            .map(str::to_owned)
    }

    pub(crate) fn focus(&mut self, id: &str) {
        let node = self.id(id);
        self.doc.inner.borrow_mut().set_focus_to(node);
        self.settle();
    }

    fn key_event(&mut self, key: Key, state: KeyState) {
        let event = BlitzKeyEvent {
            key,
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

    /// A key pressed and released on the focused element.
    pub(crate) fn key(&mut self, key: Key) {
        self.key_event(key.clone(), KeyState::Pressed);
        self.settle();
        self.key_event(key, KeyState::Released);
        self.settle();
    }

    /// A key held down (not released).
    pub(crate) fn key_down(&mut self, key: Key) {
        self.key_event(key, KeyState::Pressed);
        self.settle();
    }

    pub(crate) fn key_up(&mut self, key: Key) {
        self.key_event(key, KeyState::Released);
        self.settle();
    }

    /// A pad's action, as the app's events bring it.
    pub(crate) fn pad(&mut self, action: Action) {
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

    fn middle(&self, id: &str) -> (f32, f32) {
        let node = self.id(id);
        let rect = self
            .doc
            .inner
            .borrow()
            .get_client_bounding_rect(node)
            .unwrap_or_else(|| panic!("#{id} has no box"));
        (
            (rect.x + rect.width / 2.0) as f32,
            (rect.y + rect.height / 2.0) as f32,
        )
    }

    /// The mouse moved over the middle of `id`.
    pub(crate) fn hover(&mut self, id: &str) {
        let (x, y) = self.middle(id);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        self.settle();
    }

    /// A mouse click on the middle of `id`.
    pub(crate) fn click(&mut self, id: &str) {
        let (x, y) = self.middle(id);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerDown(pointer(x, y, true)),
        );
        self.settle();
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerUp(pointer(x, y, false)),
        );
        self.settle();
    }

    /// The mouse button held down on `id` (not released).
    pub(crate) fn mouse_down(&mut self, id: &str) {
        let (x, y) = self.middle(id);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerDown(pointer(x, y, true)),
        );
        self.settle();
    }

    /// What a screen reader gets from `id`'s subtree.
    pub(crate) fn heard(&self, id: &str) -> String {
        crate::a11y_snapshot::snapshot_of(&self.doc.inner.borrow(), self.id(id))
    }
}

fn pointer(x: f32, y: f32, down: bool) -> blitz_traits::events::BlitzPointerEvent {
    use blitz_traits::events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords,
        PointerDetails,
    };
    BlitzPointerEvent {
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
    }
}

/// Pad actions reach every input root listening: one window at a time.
fn one_window() -> std::sync::MutexGuard<'static, ()> {
    crate::input::tests::ONE_WINDOW
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

const ENTER: Key = Key::Named(NamedKey::Enter);

fn space() -> Key {
    Key::Character(" ".into())
}

// ── Buttons and links ──

fn buttons() -> Element {
    rsx! {
        InputRoot {
            Button { id: "save", variant: ButtonVariant::Primary, on_press: |_| did("save"), "Save" }
            Button { id: "off", disabled: true, on_press: |_| did("off"), "Can't" }
            Button { id: "busy", pending: true, on_press: |_| did("busy"), "Saving" }
            Link { id: "home", href: "/home", on_press: |_| did("home"), current: true, "Home" }
        }
    }
}

#[test]
fn a_button_is_pressed_by_mouse_enter_space_and_a_pad() {
    let _one = one_window();
    let mut window = Window::open(buttons);
    window.click("save");
    assert_eq!(take_done(), ["save"]);
    window.focus("save");
    window.key(ENTER);
    assert_eq!(take_done(), ["save"]);
    // Space presses on release, and shows it pressed meanwhile.
    window.key_down(space());
    assert_eq!(window.attr("save", "data-pressed").as_deref(), Some("true"));
    assert!(take_done().is_empty());
    window.key_up(space());
    assert_eq!(take_done(), ["save"]);
    assert_eq!(window.attr("save", "data-pressed"), None);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["save"]);
    // Disabled and pending buttons do nothing, and say so.
    window.click("off");
    window.click("busy");
    assert!(take_done().is_empty());
    assert_eq!(window.attr("off", "data-disabled").as_deref(), Some("true"));
    assert_eq!(window.attr("busy", "data-pending").as_deref(), Some("true"));
}

#[test]
fn a_button_shows_hover_press_and_focus_as_the_webviews_attributes() {
    let _one = one_window();
    let mut window = Window::open(buttons);
    assert_eq!(window.attr("save", "data-cs").as_deref(), Some("button"));
    assert_eq!(
        window.attr("save", "data-variant").as_deref(),
        Some("primary")
    );
    assert_eq!(window.attr("save", "data-size").as_deref(), Some("md"));
    assert_eq!(window.attr("save", "data-hovered"), None);
    window.hover("save");
    assert_eq!(window.attr("save", "data-hovered").as_deref(), Some("true"));
    window.mouse_down("save");
    assert_eq!(window.attr("save", "data-pressed").as_deref(), Some("true"));
    // A click puts focus on the control (the input layer's, as Blitz
    // focuses only fields).
    window.click("save");
    assert_eq!(window.attr("save", "data-pressed"), None);
    assert_eq!(window.attr("save", "data-focused").as_deref(), Some("true"));
    window.hover("home");
    assert_eq!(window.attr("save", "data-hovered"), None);
}

#[test]
fn buttons_and_links_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(buttons);
    assert_eq!(window.heard("save"), "Button \"Save\" [focus click]\n");
    // Disabled: no actions offered.
    assert_eq!(window.heard("off"), "Button \"Can't\" disabled\n");
    assert_eq!(
        window.heard("home"),
        "Link \"Home\" current=Page [focus click]\n"
    );
}

#[test]
fn a_link_follows_on_enter_and_a_pad_but_not_space() {
    let _one = one_window();
    let mut window = Window::open(buttons);
    window.click("home");
    assert_eq!(take_done(), ["home"]);
    window.focus("home");
    window.key(ENTER);
    assert_eq!(take_done(), ["home"]);
    window.key(space());
    assert!(take_done().is_empty());
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["home"]);
}

// ── Switches and checkboxes ──

#[component]
fn Toggles() -> Element {
    let mut on = use_signal(|| false);
    let mut ticked = use_signal(|| true);
    let mut mixed = use_signal(|| true);
    rsx! {
        InputRoot {
            Switch {
                id: "wifi",
                checked: on(),
                on_change: move |value| { on.set(value); did(format!("wifi {value}")) },
                label: "Wi-Fi",
            }
            Switch { id: "locked", checked: true, readonly: true, on_change: |_| did("locked"), label: "Locked" }
            Checkbox {
                id: "hidden",
                checked: ticked(),
                on_change: move |value| { ticked.set(value); did(format!("hidden {value}")) },
                label: "Show hidden games",
            }
            Checkbox {
                id: "all",
                checked: false,
                indeterminate: mixed(),
                on_change: move |value| { mixed.set(false); did(format!("all {value}")) },
                label: "Select all",
            }
        }
    }
}

fn toggles() -> Element {
    rsx! { Toggles {} }
}

#[test]
fn a_switch_toggles_by_mouse_space_and_a_pad_but_not_enter() {
    let _one = one_window();
    let mut window = Window::open(toggles);
    window.click("wifi");
    assert_eq!(take_done(), ["wifi true"]);
    assert_eq!(
        window.attr("wifi", "data-selected").as_deref(),
        Some("true")
    );
    assert_eq!(window.attr("wifi", "aria-checked").as_deref(), Some("true"));
    window.focus("wifi");
    window.key(space());
    assert_eq!(take_done(), ["wifi false"]);
    assert_eq!(window.attr("wifi", "data-selected"), None);
    window.key(ENTER);
    assert!(take_done().is_empty());
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["wifi true"]);
    // Read-only: shown, not changed.
    window.click("locked");
    assert!(take_done().is_empty());
}

#[test]
fn a_checkbox_ticks_and_an_indeterminate_one_ticks_first() {
    let _one = one_window();
    let mut window = Window::open(toggles);
    window.focus("hidden");
    window.key(space());
    assert_eq!(take_done(), ["hidden false"]);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["hidden true"]);
    assert_eq!(window.attr("all", "aria-checked").as_deref(), Some("mixed"));
    assert_eq!(
        window.attr("all", "data-indeterminate").as_deref(),
        Some("true")
    );
    window.click("all");
    assert_eq!(take_done(), ["all true"]);
}

#[test]
fn switches_and_checkboxes_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(toggles);
    assert_eq!(
        window.heard("wifi"),
        "Switch \"Wi-Fi\" toggled=False [focus click]\n"
    );
    assert_eq!(
        window.heard("hidden"),
        "CheckBox \"Show hidden games\" toggled=True [focus click]\n"
    );
    assert_eq!(
        window.heard("all"),
        "CheckBox \"Select all\" toggled=Mixed [focus click]\n"
    );
}
