//! The virtual grid and list over 10,000 fixture games (the real core and
//! app layer, in memory), in a Blitz document without a window, scrolled
//! with the wheel from top to bottom: the cards mounted stay bounded, and
//! the games shown are the ones the scroll position says.

use super::window::Frame;
use super::*;
use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use blitz_dom::{DocumentConfig, LocalName, NodeId};
use blitz_traits::events::{
    BlitzWheelDelta, BlitzWheelEvent, MouseEventButtons, PointerCoords, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_core::model::{GameFilter, GameSort, ListGames};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::Modifiers;
use dioxus_native::prelude::*;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Plain fixture games; fixtures mode adds its emulated ones.
const GAMES: usize = 10_000;

static SOURCE: OnceLock<DataSource> = OnceLock::new();

/// How many games the fixture library has: 10,000 and the emulated ones.
fn total() -> usize {
    let source = SOURCE.get().expect("the fixture library");
    let library = source.library.clone();
    let page = caskade_app::runtime::block_on(async move {
        library
            .page(&ListGames {
                limit: 1,
                ..ListGames::default()
            })
            .await
    })
    .unwrap();
    usize::try_from(page.total).unwrap()
}
static LAYOUT: Mutex<Layout> = Mutex::new(Layout::Grid);
/// One window at a time: pad actions go to every input root listening
/// (`input::tests`'), and the layout here is shared.
use crate::input::tests::ONE_WINDOW as ONE;

fn app() -> Element {
    use_data_source(|| SOURCE.get().expect("the fixture library").clone());
    let layout = *LAYOUT.lock().unwrap_or_else(|e| e.into_inner());
    let query = ListGames {
        filter: GameFilter::All,
        sort: GameSort::Title,
        ..ListGames::default()
    };
    rsx! {
        InputRoot {
            div { style: "display: flex; flex-direction: column; height: 800px;",
                VirtualLibrary { query, layout }
            }
        }
    }
}

struct Window {
    doc: DioxusDocument,
}

impl Window {
    fn open(layout: Layout) -> Self {
        let source = SOURCE
            .get_or_init(|| caskade_app::runtime::block_on(DataSource::fixtures(GAMES)).unwrap());
        let _ = source;
        *LAYOUT.lock().unwrap_or_else(|e| e.into_inner()) = layout;
        let mut doc = DioxusDocument::new(
            VirtualDom::new(app),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Light)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.settle_until(|window| window.count("data-index") > 0);
        window
    }

    fn settle(&mut self, time: Duration) {
        let until = Instant::now() + time;
        while Instant::now() < until {
            blitz_dom::Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    fn settle_until(&mut self, done: impl Fn(&Self) -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            self.settle(Duration::from_millis(20));
            if done(self) {
                // A last round for what followed.
                self.settle(Duration::from_millis(60));
                return;
            }
        }
        let indexes = self.indexes();
        panic!(
            "never settled: scroll {} px, {} games mounted ({:?}..{:?}), {} placeholders",
            self.scroll_top(),
            indexes.len(),
            indexes.first(),
            indexes.last(),
            self.count("role")
        );
    }

    fn nodes_with(&self, attribute: &str) -> Vec<NodeId> {
        let doc = self.doc.inner.borrow();
        let mut found = Vec::new();
        doc.visit(|id, node| {
            if node.attr(LocalName::from(attribute)).is_some() {
                found.push(id);
            }
        });
        found
    }

    fn count(&self, attribute: &str) -> usize {
        self.nodes_with(attribute).len()
    }

    /// The indexes of the games mounted.
    fn indexes(&self) -> Vec<usize> {
        let doc = self.doc.inner.borrow();
        let mut indexes: Vec<usize> = self
            .nodes_with("data-index")
            .into_iter()
            .filter_map(|id| {
                doc.get_node(id)?
                    .attr(LocalName::from("data-index"))?
                    .parse()
                    .ok()
            })
            .collect();
        indexes.sort_unstable();
        indexes
    }

    fn scroller(&self) -> NodeId {
        let doc = self.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("data-cs")) == Some("virtual-library") {
                found = Some(id);
            }
        });
        found.expect("the grid")
    }

    fn scroll_top(&self) -> f64 {
        let doc = self.doc.inner.borrow();
        doc.get_node(self.scroller()).unwrap().scroll_offset().y
    }

    /// The wheel over the grid, `by` pixels down. The wheel goes to what
    /// the pointer is over: it moves there first.
    fn wheel(&mut self, by: f64) {
        let (x, y) = (600.0, 400.0);
        let pointer = blitz_traits::events::BlitzPointerEvent {
            id: blitz_traits::events::BlitzPointerId::Mouse,
            is_primary: true,
            coords: PointerCoords {
                page_x: x,
                page_y: y,
                screen_x: x,
                screen_y: y,
                client_x: x,
                client_y: y,
            },
            button: Default::default(),
            buttons: MouseEventButtons::None,
            mods: Modifiers::empty(),
            details: Default::default(),
            element: Default::default(),
            active_pointers: Default::default(),
        };
        blitz_dom::Document::handle_ui_event(&mut self.doc, UiEvent::PointerMove(pointer));
        let event = BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(0.0, -by),
            coords: PointerCoords {
                page_x: x,
                page_y: y,
                screen_x: x,
                screen_y: y,
                client_x: x,
                client_y: y,
            },
            buttons: MouseEventButtons::None,
            mods: Modifiers::empty(),
            element: Default::default(),
        };
        blitz_dom::Document::handle_ui_event(&mut self.doc, UiEvent::Wheel(event));
    }
}

/// Scrolls from top to bottom; the most games mounted at once, and checks
/// that the games mounted are around the scroll position.
fn scroll_through(window: &mut Window, layout: Layout) -> usize {
    let mut most = window.count("data-index");
    let step = 6000.0;
    let mut last_top = -1.0;
    loop {
        window.wheel(step);
        window.settle(Duration::from_millis(30));
        let top = window.scroll_top();
        let frame = Frame {
            layout,
            total: total(),
            width: 1280.0,
            height: 800.0,
            scroll_top: top,
        };
        // Mounted once its page has loaded.
        window.settle_until(|window| {
            let shown = window.indexes();
            let first = frame.first_shown();
            shown.contains(&first)
        });
        let indexes = window.indexes();
        most = most.max(indexes.len());
        if top <= last_top {
            break;
        }
        last_top = top;
    }
    assert!(last_top > 0.0, "the wheel scrolled");
    most
}

#[test]
fn scrolling_ten_thousand_games_keeps_the_mounted_cards_bounded() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open(Layout::Grid);
    let games = total();
    assert!(games >= GAMES);
    let grid = Frame {
        layout: Layout::Grid,
        total: games,
        width: 1280.0,
        height: 800.0,
        scroll_top: 0.0,
    };
    // The scroll area is the window's height, not its content's (#81).
    let scroller = window.scroller();
    let height = window
        .doc
        .inner
        .borrow()
        .get_client_bounding_rect(scroller)
        .unwrap()
        .height;
    assert!(height <= 800.0, "the scroller is {height} px tall");
    let columns = grid.columns();
    let most = scroll_through(&mut window, Layout::Grid);
    // The rows on screen (800 / 312: four at most), one each side, and the
    // focused card's row wherever it is.
    assert!(most <= 7 * columns, "{most} cards mounted at once");
    assert_eq!(
        *window.indexes().last().unwrap(),
        games - 1,
        "the end shows"
    );
}

#[test]
fn scrolling_the_list_keeps_its_rows_bounded() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open(Layout::List);
    let most = scroll_through(&mut window, Layout::List);
    // 768 px of 44 px rows: 18 or 19 on screen, six each side, and the
    // focused row wherever it is.
    assert!(most <= 19 + 12 + 1, "{most} rows mounted at once");
}

impl Window {
    /// A pad action, as the app's events bring it.
    fn pad(&mut self, action: caskade_ui_input::Action) {
        use caskade_app::commands::input::InputActionEvent;
        use caskade_input::{GlyphFamily, InputAction, Source};
        let _ = crate::shell::events().send(caskade_app::events::Event::InputActionEvent(
            InputActionEvent(InputAction {
                action,
                source: Source::Pad,
                repeat: false,
                glyphs: GlyphFamily::Xbox,
            }),
        ));
        self.settle(Duration::from_millis(500));
    }

    /// The game focus is on.
    fn focused_index(&self) -> Option<usize> {
        let doc = self.doc.inner.borrow();
        let node = doc.get_node(doc.get_focussed_node_id()?)?;
        node.attr(LocalName::from("data-index"))?.parse().ok()
    }
}

#[test]
fn a_pad_pages_through_the_grid_and_focus_follows() {
    use caskade_ui_input::Action;
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open(Layout::Grid);
    // A pad starts on the first game.
    window.pad(Action::NavDown);
    assert_eq!(window.focused_index(), Some(0));
    // RT: a screen of whole rows further, scrolled to and focused.
    let frame = Frame {
        layout: Layout::Grid,
        total: total(),
        width: 1280.0,
        height: 800.0,
        scroll_top: 0.0,
    };
    let page = frame.per_page();
    window.pad(Action::PageNext);
    window.settle_until(|window| window.focused_index() == Some(page));
    assert_eq!(window.scroll_top(), frame.top_of(page));
    window.pad(Action::PageNext);
    window.settle_until(|window| window.focused_index() == Some(2 * page));
    // LT: back again.
    window.pad(Action::PagePrev);
    window.settle_until(|window| window.focused_index() == Some(page));
}
