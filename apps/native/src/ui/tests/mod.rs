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

mod button_tests;
mod choice_tests;
mod field_tests;
mod game_tests;
mod overlay_tests;
mod setting_row_tests;
mod showcase_tests;
mod toggle_tests;

/// What the demo's controls did.
static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn did(what: impl Into<String>) {
    caskade_core::lock(&DONE).push(what.into());
}

fn take_done() -> Vec<String> {
    std::mem::take(&mut *caskade_core::lock(&DONE))
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
        let text = match &key {
            Key::Character(text) if state == KeyState::Pressed => Some(text.clone().into()),
            _ => None,
        };
        let event = BlitzKeyEvent {
            key,
            code: Code::Unidentified,
            modifiers: Modifiers::empty(),
            location: Location::Standard,
            is_auto_repeating: false,
            is_composing: false,
            state,
            text,
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
        self.middle_of(self.id(id))
    }

    fn middle_of(&self, node: NodeId) -> (f32, f32) {
        let rect = self
            .doc
            .inner
            .borrow()
            .get_client_bounding_rect(node)
            .unwrap_or_else(|| panic!("node {node} has no box"));
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
        self.click_node(self.id(id));
    }

    /// A mouse click on the middle of `node`.
    pub(crate) fn click_node(&mut self, node: NodeId) {
        let (x, y) = self.middle_of(node);
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
    caskade_core::lock(&crate::input::tests::ONE_WINDOW)
}

const ENTER: Key = Key::Named(NamedKey::Enter);

fn space() -> Key {
    Key::Character(" ".into())
}

impl Window {
    fn child(&self, id: &str, index: usize) -> NodeId {
        let doc = self.doc.inner.borrow();
        let node = doc.get_node(self.id(id)).unwrap();
        node.children
            .iter()
            .copied()
            .filter(|child| doc.get_node(*child).is_some_and(|node| node.is_element()))
            .nth(index)
            .unwrap()
    }

    fn focused(&self) -> Option<NodeId> {
        self.doc.inner.borrow().get_focussed_node_id()
    }

    fn focused_id(&self) -> Option<String> {
        let doc = self.doc.inner.borrow();
        doc.get_focussed_node_id()
            .and_then(|node| doc.get_node(node))
            .and_then(|node| node.attr(LocalName::from("id")))
            .map(str::to_owned)
    }

    fn focus_node(&mut self, node: NodeId) {
        self.doc.inner.borrow_mut().set_focus_to(node);
        self.settle();
    }

    fn tree(&self) -> String {
        crate::a11y_snapshot::snapshot(&self.doc.inner.borrow())
    }
}

fn card_game(id: &str, title: &str) -> CardGame {
    CardGame {
        id: id.to_owned(),
        title: title.to_owned(),
        subtitle: Some("SNES".to_owned()),
        status: GameStatus::Installed,
        favorite: true,
        playtime_ms: 7_200_000,
        ..CardGame::default()
    }
}

impl Window {
    /// The id of the element with focus, or its text.
    fn focus_text(&self) -> String {
        let doc = self.doc.inner.borrow();
        doc.get_focussed_node_id()
            .and_then(|node| doc.get_node(node))
            .map(|node| node.text_content().trim().to_owned())
            .unwrap_or_default()
    }

    fn exists(&self, role: &str) -> bool {
        let doc = self.doc.inner.borrow();
        let mut found = false;
        doc.visit(|_, node| {
            found |= node.attr(LocalName::from("role")) == Some(role);
        });
        found
    }
}
