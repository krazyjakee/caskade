//! The frame in a Blitz document without a window (#96), over the fixture
//! library, inside the input layer as in the app: every page of the
//! webview opens, the sidebar navigates by mouse, keyboard and pad, Back
//! goes back, the search suggests and opens the library, typing outside a
//! field opens the search overlay, and the status bar says what runs.

use crate::data::{DataSource, use_data_source};
use crate::input::InputRoot;
use crate::router::{Route, Router};
use blitz_dom::{DocumentConfig, LocalName, NodeId};
use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_app::commands::input::InputActionEvent;
use caskade_input::{InputAction, Source};
use caskade_ui_input::{Action, GlyphFamily};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::cell::Cell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

thread_local! {
    /// The frame's navigation, as its shell last rendered.
    pub(super) static ROUTER: Cell<Option<Router>> = const { Cell::new(None) };
}

static SOURCE: OnceLock<DataSource> = OnceLock::new();
/// When the first window opened: animations' clock.
static CLOCK: OnceLock<Instant> = OnceLock::new();

/// The fixture data every window reads (Settings' tests check its saves).
pub(crate) fn source() -> &'static DataSource {
    SOURCE.get_or_init(|| caskade_app::runtime::block_on(DataSource::fixtures(40)).unwrap())
}

/// Pad actions reach every input root listening: one window at a time.
pub(crate) use crate::input::tests::ONE_WINDOW as ONE;

/// The page the next window opens on.
pub(crate) static START: std::sync::Mutex<&str> = std::sync::Mutex::new("/");

fn app() -> Element {
    use_data_source(|| SOURCE.get().expect("the fixture library").clone());
    // Files dropped on the library go to a fake install.
    use_context_provider(crate::library::view_tests::fake_drop_install);
    rsx! {
        // The app's stylesheet: the frame lays out as in the app.
        style { {include_str!("../../assets/app.css")} }
        InputRoot {
            super::Frame { start: *START.lock().unwrap_or_else(|e| e.into_inner()) }
        }
    }
}

/// A window over the fixture library with the frame, headless.
pub(crate) struct Window {
    pub(crate) doc: DioxusDocument,
}

impl Window {
    pub(crate) fn open() -> Self {
        source();
        let mut doc = DioxusDocument::new(
            VirtualDom::new(app),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.settle_until("the sidebar", |window| {
            window.find("data-cs", "sidebar").is_some()
        });
        window
    }

    pub(crate) fn settle(&mut self) {
        for _ in 0..30 {
            blitz_dom::Document::poll(&mut self.doc, None);
            // At the time that's passed, so entrances (`animate-enter`)
            // finish as in the app; at 0 they'd stay where they start.
            let now = CLOCK.get_or_init(Instant::now).elapsed().as_secs_f64();
            self.doc.inner.borrow_mut().resolve(now);
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    pub(crate) fn settle_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            self.settle();
            if done(self) {
                self.settle();
                return;
            }
        }
        let body = self
            .doc
            .inner
            .borrow()
            .find_body_node()
            .map(|body| body.text_content())
            .unwrap_or_default();
        panic!("never: {what}; the window says: {body}");
    }

    pub(crate) fn router(&self) -> Router {
        ROUTER.with(Cell::get).expect("the shell rendered")
    }

    pub(crate) fn href(&self) -> String {
        ROUTER
            .with(Cell::get)
            .map(|router| self.doc.vdom.in_runtime(|| router.peek().href()))
            .unwrap_or_default()
    }

    pub(crate) fn route(&self) -> Route {
        let router = self.router();
        self.doc.vdom.in_runtime(|| router.peek())
    }

    pub(crate) fn go(&mut self, href: &str) {
        let router = self.router();
        self.doc.vdom.in_runtime(|| router.navigate(href));
        self.settle();
    }

    /// Every element whose `name` attribute is `value`.
    pub(crate) fn find_all(&self, name: &str, value: &str) -> Vec<NodeId> {
        let doc = self.doc.inner.borrow();
        let mut found = Vec::new();
        doc.visit(|id, node| {
            if node.attr(LocalName::from(name)) == Some(value) {
                found.push(id);
            }
        });
        found
    }

    /// The first element under `root` whose `name` attribute is `value`.
    pub(crate) fn find_in(&self, root: NodeId, name: &str, value: &str) -> Option<NodeId> {
        let doc = self.doc.inner.borrow();
        self.find_all(name, value).into_iter().find(|&node| {
            std::iter::successors(Some(node), |&id| doc.get_node(id).and_then(|n| n.parent))
                .any(|id| id == root)
        })
    }

    pub(crate) fn find(&self, name: &str, value: &str) -> Option<NodeId> {
        self.find_all(name, value).into_iter().next()
    }

    pub(crate) fn get(&self, name: &str, value: &str) -> NodeId {
        self.find(name, value)
            .unwrap_or_else(|| panic!("no [{name}={value}] on {}", self.href()))
    }

    pub(crate) fn text(&self, node: NodeId) -> String {
        self.doc
            .inner
            .borrow()
            .get_node(node)
            .map(|node| node.text_content())
            .unwrap_or_default()
    }

    pub(crate) fn attr(&self, node: NodeId, name: &str) -> Option<String> {
        self.doc
            .inner
            .borrow()
            .get_node(node)?
            .attr(LocalName::from(name))
            .map(str::to_owned)
    }

    /// The link in the sidebar whose label is `label`.
    pub(crate) fn nav_link(&self, label: &str) -> NodeId {
        let links = [
            self.find_all("data-part", "nav-item"),
            self.find_all("data-part", "sub-item"),
        ]
        .concat();
        links
            .into_iter()
            .find(|&link| self.text(link).contains(label))
            .unwrap_or_else(|| panic!("no sidebar link {label}"))
    }

    pub(crate) fn middle(&self, node: NodeId) -> (f32, f32) {
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

    pub(crate) fn click(&mut self, node: NodeId) {
        let (x, y) = self.middle(node);
        for event in [
            UiEvent::PointerMove(pointer(x, y, false)),
            UiEvent::PointerDown(pointer(x, y, true)),
            UiEvent::PointerUp(pointer(x, y, false)),
        ] {
            blitz_dom::Document::handle_ui_event(&mut self.doc, event);
            self.settle();
        }
    }

    /// Two clicks in quick succession on `node`'s middle.
    pub(crate) fn double_click(&mut self, node: NodeId) {
        let (x, y) = self.middle(node);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        for _ in 0..2 {
            blitz_dom::Document::handle_ui_event(
                &mut self.doc,
                UiEvent::PointerDown(pointer(x, y, true)),
            );
            blitz_dom::Document::handle_ui_event(
                &mut self.doc,
                UiEvent::PointerUp(pointer(x, y, false)),
            );
        }
        self.settle();
    }

    /// A right click at `node`'s middle.
    pub(crate) fn right_click(&mut self, node: NodeId) {
        let (x, y) = self.middle(node);
        self.right_click_at(x, y);
    }

    pub(crate) fn right_click_at(&mut self, x: f32, y: f32) {
        use blitz_traits::events::{MouseEventButton, MouseEventButtons};
        let mut down = pointer(x, y, true);
        down.button = MouseEventButton::Secondary;
        down.buttons = MouseEventButtons::from(MouseEventButton::Secondary);
        let mut up = pointer(x, y, false);
        up.button = MouseEventButton::Secondary;
        for event in [
            UiEvent::PointerMove(pointer(x, y, false)),
            UiEvent::PointerDown(down),
            UiEvent::PointerUp(up),
        ] {
            blitz_dom::Document::handle_ui_event(&mut self.doc, event);
            self.settle();
        }
    }

    pub(crate) fn focus(&mut self, node: NodeId) {
        self.doc.inner.borrow_mut().set_focus_to(node);
        self.settle();
    }

    pub(crate) fn focused(&self) -> Option<NodeId> {
        self.doc.inner.borrow().get_focussed_node_id()
    }

    pub(crate) fn key_with(&mut self, key: Key, modifiers: Modifiers) {
        for state in [KeyState::Pressed, KeyState::Released] {
            let text = match (&key, state) {
                (Key::Character(text), KeyState::Pressed) if modifiers.is_empty() => {
                    Some(text.clone().into())
                }
                _ => None,
            };
            let event = BlitzKeyEvent {
                key: key.clone(),
                code: Code::Unidentified,
                modifiers,
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
            self.settle();
        }
    }

    pub(crate) fn key(&mut self, key: Key) {
        self.key_with(key, Modifiers::empty());
    }

    pub(crate) fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Character(c.to_string()));
        }
    }

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

    /// The page's heading, or what the main view says.
    pub(crate) fn view_text(&self) -> String {
        self.text(self.get("data-cs", "view"))
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

#[test]
fn every_page_of_the_webview_opens() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    for (href, shows) in [
        ("/", "Home"),
        ("/store?tab=upcoming", "Store"),
        ("/games/abc?from=%2F", "Game"),
        ("/store/games/1001", "Game"),
        ("/collections", "Collections"),
        ("/collections/weekend", "Collections"),
        ("/settings?tab=devices", "Settings"),
        ("/activity?tab=notifications", "No notifications yet."),
        ("/activity?tab=jobs", "Jobs"),
        ("/plugins/dev.me/x", "Not found"),
        ("/nowhere", "Not found"),
    ] {
        window.go(href);
        window.settle_until(href, |window| window.view_text().contains(shows));
    }
    // The library shows its games.
    window.go("/library?filter=all");
    window.settle_until("the library's games", |window| {
        window.find("data-cs", "virtual-library").is_some()
            && !window.find_all("data-index", "0").is_empty()
    });
}

#[test]
fn the_sidebar_navigates_by_mouse_with_the_librarys_filters_and_counts() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    let store = window.nav_link("Store");
    window.click(store);
    assert!(
        matches!(window.route(), Route::Store(_)),
        "{}",
        window.href()
    );
    let store = window.nav_link("Store");
    assert_eq!(window.attr(store, "aria-current").as_deref(), Some("page"));
    assert_eq!(window.attr(store, "data-status").as_deref(), Some("active"));

    // The library opens as it was left: first, on what was played last.
    let library = window.nav_link("Library");
    window.click(library);
    assert!(
        matches!(window.route(), Route::Library(ref library) if library.filter == caskade_core::model::GameFilter::Recent && library.opening),
        "{}",
        window.href()
    );
    // Its filters show under it, with their counts.
    window.settle_until("the counts", |window| {
        let recent = window.nav_link("Recently Played");
        window.find_all("data-part", "count").iter().any(|&count| {
            window
                .text(count)
                .trim()
                .parse::<u64>()
                .is_ok_and(|n| n > 0)
        }) && window.attr(recent, "data-status").as_deref() == Some("active")
    });
    let favorites = window.nav_link("Favorites");
    window.click(favorites);
    assert!(
        matches!(window.route(), Route::Library(ref library) if library.filter == caskade_core::model::GameFilter::Favorites),
        "{}",
        window.href()
    );
    // Elsewhere the filters go.
    let home = window.nav_link("Home");
    window.click(home);
    assert_eq!(window.route(), Route::Home);
    assert!(window.find("data-part", "sub-item").is_none());
    // Back goes back.
    window.pad(Action::Back);
    assert!(
        matches!(window.route(), Route::Library(_)),
        "{}",
        window.href()
    );
}

#[test]
fn the_sidebar_navigates_by_keyboard_and_pad() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    // Enter on a focused link.
    let settings = window.nav_link("Settings");
    window.focus(settings);
    window.key(Key::Named(NamedKey::Enter));
    assert!(
        matches!(window.route(), Route::Settings { .. }),
        "{}",
        window.href()
    );
    // A pad: focus moves between the links, A opens one, B comes back.
    let collections = window.nav_link("Collections");
    window.focus(collections);
    window.pad(Action::NavUp);
    let focused = window.focused().expect("focus");
    assert!(
        window.text(focused).contains("Store"),
        "{}",
        window.text(focused)
    );
    window.pad(Action::Confirm);
    assert!(
        matches!(window.route(), Route::Store(_)),
        "{}",
        window.href()
    );
    window.pad(Action::Back);
    assert!(
        matches!(window.route(), Route::Settings { .. }),
        "{}",
        window.href()
    );
}

#[test]
fn the_search_suggests_games_and_opens_the_library_on_the_text() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    // Ctrl K focuses it from anywhere.
    let root = window.get("data-cs", "app-shell");
    window.focus(root);
    window.key_with(Key::Character("k".into()), Modifiers::CONTROL);
    let input = window.get("role", "combobox");
    assert_eq!(window.focused(), Some(input));
    window.type_text("fixture");
    window.settle_until("suggestions", |window| {
        !window.find_all("data-part", "suggestion").is_empty()
    });
    let see_all = window.get("data-part", "see-all");
    assert!(
        window.text(see_all).contains("fixture"),
        "{}",
        window.text(see_all)
    );
    // Enter without choosing a suggestion: the library, searched.
    window.key(Key::Named(NamedKey::Enter));
    match window.route() {
        Route::Library(library) => {
            assert_eq!(library.q.as_deref(), Some("fixture"));
            assert_eq!(library.filter, caskade_core::model::GameFilter::All);
        }
        other => panic!("{other:?}"),
    }
    // The library's search shows in the box.
    let doc = window.doc.inner.borrow();
    let value = doc
        .get_node(input)
        .and_then(|node| node.element_data())
        .and_then(|element| element.text_input_data())
        .map(|field| field.editor.raw_text().to_owned());
    assert_eq!(value.as_deref(), Some("fixture"));
    drop(doc);
    // An arrow and Enter open a suggestion's game.
    window.focus(input);
    window.type_text(" ");
    window.key(Key::Named(NamedKey::Backspace));
    window.settle_until("suggestions again", |window| {
        !window.find_all("data-part", "suggestion").is_empty()
    });
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::Enter));
    assert!(
        matches!(window.route(), Route::Game { .. }),
        "{}",
        window.href()
    );
}

#[test]
fn typing_outside_a_field_opens_the_search_overlay() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    let root = window.get("data-cs", "app-shell");
    window.focus(root);
    window.key(Key::Character("f".into()));
    let overlay = window.get("data-cs", "type-search");
    window.type_text("ixture");
    window.settle_until("the count", |window| {
        window
            .find("data-part", "count")
            .is_some_and(|count| window.text(count).contains("game"))
    });
    let _ = overlay;
    // Enter: the library on the text, the overlay gone.
    window.key(Key::Named(NamedKey::Enter));
    assert!(window.find("data-cs", "type-search").is_none());
    match window.route() {
        Route::Library(library) => assert_eq!(library.q.as_deref(), Some("fixture")),
        other => panic!("{other:?}"),
    }
    // In the library, Escape puts the earlier search back.
    window.focus(window.get("data-cs", "app-shell"));
    window.key(Key::Character("z".into()));
    window.settle_until("filtered in place", |window| {
        matches!(window.route(), Route::Library(ref library) if library.q.as_deref() == Some("z"))
    });
    window.key(Key::Named(NamedKey::Escape));
    match window.route() {
        Route::Library(library) => assert_eq!(library.q.as_deref(), Some("fixture")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_status_bar_says_whats_running_and_the_version() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let window = Window::open();
    let bar = window.get("data-cs", "status-bar");
    let version = window.get("data-part", "version");
    assert!(window.text(version).contains(env!("CARGO_PKG_VERSION")));
    // Fixtures mode has no services: nothing runs.
    assert!(
        window.text(bar).starts_with("Ready"),
        "{}",
        window.text(bar)
    );
    assert!(window.find("data-part", "spinner").is_none());
}

#[test]
fn a_press_moves_the_window_only_from_the_title_bars_own_surface() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let window = Window::open();
    let doc = window.doc.inner.borrow();
    let region =
        |name: &str, value: &str| super::window::in_drag_region(&doc, window.get(name, value));
    assert!(region("data-cs", "topbar"));
    assert!(region("data-cs", "status-bar"));
    assert!(region("data-part", "brand"));
    // Controls keep their clicks; the search opts its padding out.
    assert!(!region("role", "combobox"));
    assert!(!region("data-cs", "search"));
    assert!(!region("data-part", "nav-item"));
    assert!(!region("data-cs", "view"));
}

#[test]
fn the_activity_label_says_the_running_job_and_what_waits() {
    use super::status_bar::activity_label;
    use caskade_core::install::{JobPhase, JobSnapshot, JobStatus};
    let job = |status: JobStatus, title: &str| {
        let mut job: JobSnapshot = serde_json::from_value(serde_json::json!({
            "id": title, "revision": 1, "kind": "install", "status": "queued",
            "title": title, "input": "/tmp/setup.exe", "warnings": [],
            "candidates": [], "dependencies": [], "createdAtMs": 0, "updatedAtMs": 0
        }))
        .unwrap_or_else(|error| panic!("a job snapshot: {error}"));
        job.status = status;
        job.phase = Some(JobPhase::Download);
        job
    };
    let task = |label: &str, queued: bool| {
        serde_json::from_value::<caskade_core::jobs::Task>(serde_json::json!({
            "id": 1, "label": label, "queued": queued
        }))
        .unwrap_or_else(|error| panic!("a task: {error}"))
    };
    assert_eq!(activity_label(&[], &[], false, false), "Ready");
    assert_eq!(
        activity_label(&[], &[], false, true),
        "Checking background tasks…"
    );
    assert_eq!(
        activity_label(&[], &[], true, false),
        "Background status unavailable"
    );
    assert_eq!(
        activity_label(
            &[],
            &[job(JobStatus::Downloading, "Tidewater")],
            false,
            false
        ),
        "Downloading: Tidewater"
    );
    assert_eq!(
        activity_label(
            &[task("Scanning Steam", false), task("Metadata", true)],
            &[],
            false,
            false
        ),
        "Scanning Steam · 1 queued"
    );
    assert_eq!(
        activity_label(
            &[],
            &[job(JobStatus::NeedsUser, "Lantern Keep")],
            false,
            false
        ),
        "1 task needs attention"
    );
}

#[test]
fn a_batchs_time_range_stays_true_later() {
    use super::notifications::time_range;
    let at = |hour: u32, minute: u32| {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 10, 28, hour, minute, 0)
            .unwrap()
            .timestamp_millis()
    };
    let one = time_range(at(14, 5), at(14, 5));
    assert!(one.contains("2026") && !one.contains('–'), "{one}");
    let span = time_range(at(14, 5), at(15, 30));
    assert_eq!(span.matches("2026").count(), 1, "{span}");
    assert!(span.contains('–'), "{span}");
    let days = time_range(at(14, 5), at(14, 5) + 2 * 86_400_000);
    assert_eq!(days.matches("2026").count(), 2, "{days}");
}

#[test]
fn search_matches_ignore_case_and_accents() {
    use super::search::find_folded;
    assert_eq!(find_folded("Pokémon Red", "pokemon"), Some((0, 8)));
    assert_eq!(find_folded("Super POKÉMON", "émon"), Some((9, 14)));
    assert_eq!(find_folded("Tidewater", "zelda"), None);
    assert_eq!(find_folded("Tidewater", "  "), None);
}

#[test]
fn a_right_click_on_the_title_bar_offers_the_windows_menu() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    // On a control, no: the bell keeps its click.
    let bell = window.get("data-part", "activity");
    window.right_click(bell);
    assert!(window.find("role", "menu").is_none());
    // On the bar's own surface: minimize, maximize and close, where Caskade
    // draws the title bar.
    let search = window
        .doc
        .inner
        .borrow()
        .get_client_bounding_rect(window.get("data-cs", "search"))
        .unwrap();
    let (_, y) = window.middle(window.get("data-cs", "topbar"));
    window.right_click_at((search.x + search.width) as f32 + 40.0, y);
    let menu = window.find("role", "menu");
    if super::window::CUSTOM_FRAME {
        let menu = menu.expect("the window's menu");
        let text = window.text(menu);
        for item in ["Minimize", "Maximize", "Close"] {
            assert!(text.contains(item), "{text}");
        }
    } else {
        assert!(menu.is_none());
    }
}
