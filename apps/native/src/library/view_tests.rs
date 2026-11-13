//! The library view's actions (#99) in the frame over the fixture library,
//! headless, as `views/library/LibraryView.test.tsx` has them: sorting from
//! the menu and from the list's column headers, grid or list, LB and RB
//! through the filters, the search's chip, the alphabet bar, and what it
//! says with nothing to show.

use crate::frame::tests::{ONE, Window};
use crate::router::Route;
use caskade_core::model::{GameFilter, GameSort};
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};

fn library(window: &Window) -> crate::router::LibraryRoute {
    match window.route() {
        Route::Library(library) => library,
        other => panic!("not the library: {other:?}"),
    }
}

/// The library at `href`, once its games are on screen.
fn open(href: &str) -> Window {
    let mut window = Window::open();
    window.go(href);
    window.settle_until("the library's games", |window| {
        window.find("data-view", "library").is_some()
            && (!window.find_all("data-index", "0").is_empty()
                || window.find("data-part", "empty").is_some())
    });
    window
}

#[test]
fn the_sort_menu_and_the_lists_headers_sort_the_library() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/library?filter=all&sort=recent&layout=grid");
    // The menu, by keyboard: Recently played is first, Title next.
    let trigger = window.get("data-part", "sort");
    window.focus(trigger);
    window.key(Key::Character(" ".into()));
    window.settle_until("the sort menu", |window| {
        window.find("role", "listbox").is_some()
    });
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(Key::Named(NamedKey::Enter));
    assert_eq!(library(&window).sort, GameSort::Title);

    // The list, whose column headers sort it.
    let list = window
        .find_all("data-part", "layout")
        .into_iter()
        .find(|&link| window.attr(link, "aria-label").as_deref() == Some("list view"))
        .expect("the list view link");
    window.click(list);
    assert_eq!(library(&window).layout, crate::library::Layout::List);
    window.settle_until("the list's header", |window| {
        window.find("data-part", "table-header").is_some()
    });
    let headers = window.find_all("role", "columnheader");
    let title = headers
        .iter()
        .copied()
        .find(|&header| window.text(header).contains("Title"))
        .unwrap();
    assert_eq!(
        window.attr(title, "aria-sort").as_deref(),
        Some("ascending")
    );
    let playtime = headers
        .into_iter()
        .find(|&header| window.text(header).contains("Playtime"))
        .unwrap();
    let link = window
        .doc
        .inner
        .borrow()
        .get_node(playtime)
        .unwrap()
        .children[0];
    window.click(link);
    assert_eq!(library(&window).sort, GameSort::Playtime);
    window.settle_until("Playtime sorted", |window| {
        window
            .find_all("role", "columnheader")
            .into_iter()
            .any(|header| {
                window.text(header).contains("Playtime")
                    && window.attr(header, "aria-sort").as_deref() == Some("descending")
            })
    });
}

#[test]
fn lb_and_rb_step_through_the_filters_and_wrap() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/library?filter=recent");
    window.pad(Action::TabNext);
    assert_eq!(library(&window).filter, GameFilter::Installed);
    window.pad(Action::TabPrev);
    window.pad(Action::TabPrev);
    assert_eq!(library(&window).filter, GameFilter::All);
    // In place: Back leaves the library, not the last filter.
    let before = window.href();
    assert!(before.contains("filter=all"), "{before}");
}

#[test]
fn a_search_shows_as_a_chip_that_clears_it() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/library?filter=all&q=fixture");
    let chip = window.get("data-part", "clear-search");
    assert!(window.text(chip).contains("Results for “fixture”"));
    window.click(chip);
    assert_eq!(library(&window).q, None);
    assert!(window.find("data-part", "clear-search").is_none());
}

#[test]
fn a_long_library_sorted_by_title_has_the_alphabet_bar() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/library?filter=all&sort=title");
    window.settle_until("the alphabet bar", |window| {
        window.find("data-cs", "alphabet-bar").is_some()
    });
    // Not sorted by title: none.
    window.go("/library?filter=all&sort=recent");
    window.settle_until("no alphabet bar", |window| {
        window.find("data-cs", "alphabet-bar").is_none()
    });
}

#[test]
fn an_empty_filter_says_so_and_offers_every_game() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/library?filter=all&q=nothing-is-called-this");
    let empty = window.get("data-part", "empty");
    assert!(
        window
            .text(empty)
            .contains("No games match your search in this filter."),
        "{}",
        window.text(empty)
    );
    window.go("/library?filter=web&sort=title&layout=list");
    window.settle_until("the empty web tab", |window| {
        window
            .find("data-part", "empty")
            .is_some_and(|empty| window.text(empty).contains("No web games yet."))
    });
    let all = {
        let doc = window.doc.inner.borrow();
        let mut links = Vec::new();
        doc.visit(|id, node| {
            if node
                .element_data()
                .is_some_and(|element| &*element.name.local == "a")
            {
                links.push(id);
            }
        });
        links
    }
    .into_iter()
    .find(|&link| window.text(link).contains("Show all games"))
    .expect("Show all games");
    window.click(all);
    let shown = library(&window);
    assert_eq!(shown.filter, GameFilter::All);
    // It keeps the sort and layout.
    assert_eq!(shown.sort, GameSort::Title);
    assert_eq!(shown.layout, crate::library::Layout::List);
}

/// The paths dropped files asked to install, in the order asked.
static DROPPED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Installs every dropped file but a `.bad` one, which it says it can't.
pub(crate) fn fake_drop_install() -> super::DropInstall {
    super::DropInstall(std::rc::Rc::new(|path: String| {
        Box::pin(async move {
            if path.ends_with(".bad") {
                return Err(format!("can't add {path}"));
            }
            caskade_core::lock(&DROPPED).push(path);
            Ok(())
        })
    }))
}

#[test]
fn files_dropped_on_the_library_ask_to_install_and_say_why_not() {
    use crate::drops::{DropEvent, drops};
    use std::path::PathBuf;
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    caskade_core::lock(&DROPPED).clear();
    let mut window = open("/library?filter=all&sort=title&layout=grid");
    // Dragged over: the hint.
    let _ = drops().send(DropEvent::Over);
    window.settle_until("the hint", |window| {
        window.find("data-part", "drop-hint").is_some()
    });
    let hint = window.get("data-part", "drop-hint");
    assert!(
        window
            .text(hint)
            .contains("Drop to install into your library")
    );
    let _ = drops().send(DropEvent::Left);
    window.settle_until("no hint", |window| {
        window.find("data-part", "drop-hint").is_none()
    });
    // Dropped: each asks to install; one that can't says why.
    let _ = drops().send(DropEvent::Over);
    let _ = drops().send(DropEvent::Dropped(vec![
        PathBuf::from("/games/Setup.exe"),
        PathBuf::from("/games/broken.bad"),
    ]));
    window.settle_until("why one can't", |window| {
        window.find("data-part", "drop-error").is_some()
    });
    assert!(window.find("data-part", "drop-hint").is_none());
    assert_eq!(*caskade_core::lock(&DROPPED), ["/games/Setup.exe"]);
    let error = window.get("data-part", "drop-error");
    assert_eq!(window.attr(error, "role").as_deref(), Some("alert"));
    assert!(window.text(error).contains("can't add /games/broken.bad"));
    let dismiss = window
        .find_in(error, "aria-label", "Dismiss")
        .expect("Dismiss");
    window.click(dismiss);
    assert!(window.find("data-part", "drop-error").is_none());
}
