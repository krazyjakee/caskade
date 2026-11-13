//! The flows driven end to end in a Blitz document without a window, over
//! the fixture library (the real core and app layer, in memory), with the
//! keyboard as a user would: the button, its menu, the dialog's fields and
//! buttons. What the core ends up holding is checked through the library.

use super::add_game::AddGame;
use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use crate::input::tests::ONE_WINDOW as ONE;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::model::{LaunchSpec, ListGames};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

mod add_game_tests;
mod device_tests;
mod host_tests;
mod picker_tests;
mod share_tests;
mod storage_tests;

static SOURCE: OnceLock<DataSource> = OnceLock::new();
/// The games the flows asked to open, in order.
static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn source() -> &'static DataSource {
    SOURCE.get_or_init(|| caskade_app::runtime::block_on(DataSource::fixtures(12)).unwrap())
}

fn app() -> Element {
    use_data_source(|| source().clone());
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            AddGame {
                on_open_game: |id: String| caskade_core::lock(&OPENED).push(id),
            }
        }
    }
}

struct Window {
    doc: DioxusDocument,
}

impl Window {
    fn open() -> Self {
        source();
        caskade_core::lock(&OPENED).clear();
        Self::open_root(app, "the Add game button", "#add-game-button")
    }

    /// `root` in a window, settled until `ready` (a selector) is there.
    fn open_root(root: fn() -> Element, what: &str, ready: &str) -> Self {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(root),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.until(what, |w| w.find(ready).is_some());
        window
    }

    /// A pad's action, as the app's events bring it.
    fn pad(&mut self, action: caskade_ui_input::Action) {
        use caskade_app::commands::input::InputActionEvent;
        use caskade_input::{InputAction, Source};
        let _ = crate::shell::events().send(caskade_app::events::Event::InputActionEvent(
            InputActionEvent(InputAction {
                action,
                source: Source::Pad,
                repeat: false,
                glyphs: caskade_ui_input::GlyphFamily::Xbox,
            }),
        ));
        self.settle();
    }

    fn settle(&mut self) {
        for _ in 0..12 {
            blitz_dom::Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    /// Settles until `done`, or fails naming `what` it waited for.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        self.until_for(what, 20, done);
    }

    /// [`until`](Self::until), waiting up to `seconds`.
    fn until_for(&mut self, what: &str, seconds: u64, done: impl Fn(&Self) -> bool) {
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            self.settle();
            if done(self) {
                self.settle();
                return;
            }
        }
        panic!("never saw {what}");
    }

    fn find(&self, selector: &str) -> Option<NodeId> {
        self.doc
            .inner
            .borrow()
            .query_selector(selector)
            .ok()
            .flatten()
    }

    fn count(&self, selector: &str) -> usize {
        self.doc
            .inner
            .borrow()
            .query_selector_all(selector)
            .map(|nodes| nodes.len())
            .unwrap_or(0)
    }

    fn text(&self, selector: &str) -> String {
        let Some(node) = self.find(selector) else {
            return String::new();
        };
        let doc = self.doc.inner.borrow();
        doc.get_node(node)
            .map(|node| node.text_content())
            .unwrap_or_default()
    }

    /// The text of every element `selector` finds, in document order.
    fn texts(&self, selector: &str) -> Vec<String> {
        let doc = self.doc.inner.borrow();
        doc.query_selector_all(selector)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|&node| doc.get_node(node).map(|node| node.text_content()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn attr(&self, selector: &str, name: &str) -> Option<String> {
        let node = self.find(selector)?;
        let doc = self.doc.inner.borrow();
        doc.get_node(node)?
            .attr(blitz_dom::LocalName::from(name))
            .map(str::to_owned)
    }

    fn focus(&mut self, selector: &str) {
        let node = self
            .find(selector)
            .unwrap_or_else(|| panic!("no {selector}"));
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

    fn key(&mut self, key: Key) {
        self.key_event(key.clone(), KeyState::Pressed);
        self.settle();
        self.key_event(key, KeyState::Released);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.key_event(Key::Character(character.to_string()), KeyState::Pressed);
            self.key_event(Key::Character(character.to_string()), KeyState::Released);
        }
        self.settle();
    }

    /// Opens Add game's menu from the keyboard and chooses its `index`th item.
    fn choose(&mut self, index: usize) {
        self.focus("#add-game-button");
        self.key(Key::Named(NamedKey::ArrowDown));
        self.until("the menu", |w| w.find("[role=menu]").is_some());
        for _ in 0..index {
            self.key(Key::Named(NamedKey::ArrowDown));
        }
        self.key(Key::Named(NamedKey::Enter));
        self.until("the dialog", |w| w.find("[data-cs=manual-game]").is_some());
    }

    /// Presses a button from the keyboard.
    fn press(&mut self, selector: &str) {
        self.focus(selector);
        self.key(Key::Named(NamedKey::Enter));
    }

    fn opened(&self) -> Vec<String> {
        caskade_core::lock(&OPENED).clone()
    }
}

fn game_title(id: &str) -> Option<String> {
    let library = source().library.clone();
    let id = id.to_owned();
    caskade_app::runtime::block_on(async move { library.game(&id).await })
        .unwrap()
        .map(|game| game.game.title)
}

/// A fixture game's title, for a duplicate.
fn a_fixture_title() -> String {
    let library = source().library.clone();
    let page = caskade_app::runtime::block_on(async move {
        library
            .page(&ListGames {
                limit: 1,
                ..ListGames::default()
            })
            .await
    })
    .unwrap();
    page.items[0].title.clone()
}
