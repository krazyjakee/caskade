//! A game's page (#100) in the frame over the fixture library, headless,
//! per kind of game, as `GameView.test.tsx` has them: an installed game,
//! one that isn't, and an emulated one; the way back; and what isn't there.

use super::launch_tests::{Script, launches, script, started};
use crate::frame::tests::{ONE, Window};
use crate::router::Route;

pub(crate) mod page_data_tests;

/// `href`'s page, once its title shows.
fn open(href: &str) -> Window {
    let mut window = Window::open();
    window.go(href);
    window.settle_until("the game's page", |window| {
        window.find("data-view", "game").is_some()
            || window
                .find_all("role", "status")
                .into_iter()
                .any(|node| window.text(node).contains("Game not found"))
            || window.find("data-part", "back").is_some()
    });
    window.settle_until("its title", |window| {
        window.find("data-part", "title").is_some() || window.find("data-part", "back").is_some()
    });
    window
}

fn text(window: &Window, part: &str) -> String {
    window
        .find_all("data-part", part)
        .into_iter()
        .map(|node| window.text(node))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn play_button(window: &Window) -> blitz_dom::NodeId {
    let hero = window.get("data-cs", "hero");
    window.find_in(hero, "data-part", "play").expect("Play")
}

#[test]
fn an_installed_game_shows_what_it_is_and_its_release() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    // Fixture Game 0002: installed, Adventure, PC, released 2024.
    let window = open("/games/fixture-1");
    assert_eq!(
        window
            .attr(window.get("data-view", "game"), "data-status")
            .as_deref(),
        Some("installed")
    );
    assert_eq!(text(&window, "title"), "Fixture Game 0002");
    let meta = text(&window, "meta");
    assert!(meta.contains("Adventure · PC · 1.0 GB · 2024"), "{meta}");
    assert!(text(&window, "description").contains("Original development fixture."));
    assert!(text(&window, "tags").contains("Fixture"));
    // Its one release, from the fixture launcher, with nothing to launch:
    // Play stays, disabled.
    let entry = text(&window, "entry");
    assert!(entry.contains("Installed"), "{entry}");
    assert_eq!(
        window
            .attr(play_button(&window), "data-disabled")
            .as_deref(),
        Some("true")
    );
    // One release: no picker, no Default badge.
    assert!(window.find("data-part", "release").is_none());
    assert!(window.find("data-part", "default").is_none());
    assert!(text(&window, "sessions").is_empty());
}

#[test]
fn a_game_that_isnt_installed_says_its_release_is_partly_there() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    // Fixture Game 0001: not installed.
    let window = open("/games/fixture-0");
    assert_eq!(
        window
            .attr(window.get("data-view", "game"), "data-status")
            .as_deref(),
        Some("not-installed")
    );
    let entry = text(&window, "entry");
    assert!(entry.contains("Partly installed"), "{entry}");
    assert_eq!(
        window
            .attr(play_button(&window), "data-disabled")
            .as_deref(),
        Some("true")
    );
}

#[test]
fn an_emulated_game_plays_through_play() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/games/fixture-emulated-0-0");
    assert_eq!(
        window
            .attr(window.get("data-view", "game"), "data-status")
            .as_deref(),
        Some("emulated")
    );
    let _script = script(Script {
        outcomes: [started("fixture-emulated-0-0")].into(),
        ..Script::default()
    });
    let play = play_button(&window);
    assert_eq!(window.attr(play, "data-disabled"), None);
    window.click(play);
    window.settle_until("the launch", |_| launches() == 1);
}

#[test]
fn back_goes_where_the_game_was_opened_from() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    // Opened from a link to it: back to the Library, in its place.
    let mut window = open("/games/fixture-1");
    let back = window.get("data-part", "back");
    assert!(
        window.text(back).contains("Back to Library"),
        "{}",
        window.text(back)
    );
    window.click(back);
    window.settle_until("the Library", |window| {
        matches!(window.route(), Route::Library(_))
    });
    // Opened from another game's page: named after that game, and a step
    // back in history to it.
    window.go("/games/fixture-1");
    window.settle_until("the first game", |window| {
        window.find("data-part", "title").is_some()
    });
    window.go("/games/fixture-2?from=%2Fgames%2Ffixture-1");
    window.settle_until("the second game", |window| {
        window
            .find("data-part", "title")
            .is_some_and(|title| window.text(title) == "Fixture Game 0003")
    });
    window.settle_until("the other game's name", |window| {
        window
            .find("data-part", "back")
            .is_some_and(|back| window.text(back).contains("Back to Fixture Game 0002"))
    });
    let back = window.get("data-part", "back");
    window.click(back);
    window.settle_until(
        "the first game again",
        |window| matches!(window.route(), Route::Game { ref id, .. } if id == "fixture-1"),
    );
}

#[test]
fn a_game_that_isnt_there_says_so() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = Window::open();
    window.go("/games/no-such-game");
    window.settle_until("not found", |window| {
        window
            .find("role", "status")
            .is_none_or(|status| !window.text(status).contains("Loading"))
            && window.find("data-part", "back").is_some()
    });
    assert!(window.find("data-view", "game").is_none());
}

#[test]
fn a_right_click_on_the_hero_opens_the_games_menu() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open("/games/fixture-1");
    let hero = window.get("data-cs", "hero");
    window.right_click(hero);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
}
