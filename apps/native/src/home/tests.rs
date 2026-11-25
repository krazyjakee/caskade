//! Home (#98) in the frame over the fixture library, headless: the hero is
//! the game played last and leaves Recently Played, Resume · View details
//! opens its page, the new games' shelves and their View all, and where a
//! game was last played.

use super::{played_on_label, released};
use crate::frame::tests::{ONE, Window};
use crate::router::Route;
use caskade_core::model::{GameSummary, PlayedOn};

fn home() -> Window {
    let mut window = Window::open();
    window.go("/");
    window.settle_until("Home's shelves", |window| {
        window.find("data-view", "home").is_some()
            && window.find("data-focus-section", "shelf-added").is_some()
            && window.find("data-cs", "hero").is_some()
    });
    window
}

/// The card titles on shelf `id`.
fn titles(window: &Window, id: &str) -> Vec<String> {
    let shelf = window.get("data-focus-section", &format!("shelf-{id}"));
    window
        .find_all_in(shelf, "data-cs", "game-card")
        .into_iter()
        .filter_map(|card| window.find_in(card, "data-part", "title"))
        .map(|title| window.text(title))
        .collect()
}

#[test]
fn the_game_played_last_is_the_hero_and_leaves_recently_played() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = home();
    let said = window.view_text();
    assert!(said.contains("Welcome back"), "{said}");
    assert!(said.contains("0 running"), "{said}");
    let hero = window.get("data-cs", "hero");
    let eyebrow = window.find_in(hero, "data-part", "eyebrow").unwrap();
    assert_eq!(window.text(eyebrow), "Continue Playing");
    let title = window.text(window.find_in(hero, "data-part", "title").unwrap());
    // The fixtures' Game Boy Advance game was played a day ago: last.
    assert_eq!(title, "Advance Fixture 1");
    // Played here: no other device named.
    assert!(window.find_in(hero, "data-part", "played-on").is_none());
    window.settle_until("Recently Played", |window| {
        !titles(window, "recent").is_empty()
    });
    let recent = titles(&window, "recent");
    assert!(!recent.contains(&title), "{recent:?}");
    assert!(
        recent.contains(&"System Fixture 1".to_owned()),
        "{recent:?}"
    );
    // Play is the primary focused action; View details opens the game page.
    let play = window.find_in(hero, "data-part", "primary-action").unwrap();
    assert_eq!(window.text(play).trim(), "Play");
    assert_eq!(
        window.attr(play, "data-focus-default").as_deref(),
        Some("true")
    );
    let details = window.find_in(hero, "data-part", "details").unwrap();
    assert_eq!(window.text(details).trim(), "View details");
    window.click(details);
    match window.route() {
        Route::Game { id, from } => {
            assert_eq!(id, "fixture-emulated-4-0");
            assert_eq!(from.as_deref(), Some("/"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_new_games_shelves_show_the_last_added_and_the_released_games() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = home();
    window.settle_until("Recently Released", |window| {
        window
            .find("data-focus-section", "shelf-released")
            .is_some()
            && !titles(window, "released").is_empty()
    });
    assert_eq!(titles(&window, "added").len(), 12);
    // Every fourth plain fixture and the emulated ones have no release
    // date: they stay off.
    let released = titles(&window, "released");
    assert_eq!(released.len(), 12, "{released:?}");
    for title in &released {
        let number: usize = title
            .strip_prefix("Fixture Game ")
            .and_then(|number| number.parse().ok())
            .unwrap_or_else(|| panic!("{title} has no release date"));
        assert_ne!((number - 1) % 4, 0, "{title} has no release date");
    }
    let shelf = window.get("data-focus-section", "shelf-released");
    let view_all = window.find_in(shelf, "data-part", "view-all").unwrap();
    assert_eq!(window.text(view_all), "View all");
    window.click(view_all);
    assert!(
        window
            .href()
            .starts_with("/library?filter=all&sort=released"),
        "{}",
        window.href()
    );
}

fn summary(id: &str, release_date: Option<&str>) -> GameSummary {
    GameSummary {
        admin_pin_needed: false,
        id: id.to_owned(),
        title: id.to_owned(),
        sort_title: id.to_owned(),
        description: String::new(),
        genres: Vec::new(),
        tags: Vec::new(),
        release_date: release_date.map(str::to_owned),
        platforms: Vec::new(),
        favorite: false,
        hidden: false,
        rating: None,
        cover: None,
        icon: None,
        installed: true,
        remote_device_ids: Vec::new(),
        playtime_ms: 0,
        last_played_at_ms: None,
        last_played_on: None,
    }
}

#[test]
fn recently_released_leaves_out_the_undated_and_the_unreleased() {
    let games = [
        summary("a", Some("2001-03-21")),
        summary("b", None),
        summary("c", Some("2099-01-01")),
        summary("d", Some("2019-06-27")),
        summary("e", Some("2026-11-11")),
    ];
    let shown: Vec<String> = released(&games, "2026-11-11")
        .into_iter()
        .map(|game| game.id)
        .collect();
    assert_eq!(shown, ["a", "d", "e"]);
}

#[test]
fn another_device_is_named_when_paired_and_nothing_for_this_one() {
    assert_eq!(played_on_label(None), None);
    let paired = PlayedOn {
        device: "den".into(),
        name: Some("Den".into()),
    };
    assert_eq!(
        played_on_label(Some(&paired)).as_deref(),
        Some("Last played on Den")
    );
    let unpaired = PlayedOn {
        device: "handheld".into(),
        name: Some("  ".into()),
    };
    assert_eq!(
        played_on_label(Some(&unpaired)).as_deref(),
        Some("Last played on another device")
    );
}

#[test]
fn lt_and_rt_page_focus_along_a_shelf() {
    use caskade_ui_input::Action;
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = home();
    let shelf = window.get("data-focus-section", "shelf-added");
    let cards = window.find_all_in(shelf, "data-cs", "game-card");
    assert_eq!(cards.len(), 12);
    window.focus(cards[0]);
    window.pad(Action::PageNext);
    let at = |window: &Window| {
        let focused = window.focused();
        cards.iter().position(|&card| Some(card) == focused)
    };
    let next = at(&window).expect("a card focused");
    assert!(next > 1, "a page along, not one card: {next}");
    window.pad(Action::PagePrev);
    assert_eq!(at(&window), Some(0));
}

/// Home by keys alone (#153), in desktop mode with no pad: Tab reaches the
/// hero's Resume · View details and Enter opens the game; Tab reaches its
/// way back and Enter comes back to Home; Tab reaches a new game's card on
/// a shelf and Enter opens it, then a shelf's View all and Enter opens the
/// library.
#[test]
fn home_is_driven_by_keys_alone() {
    use crate::frame::pad_tests::tab_to;
    use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = home();
    let inside = |window: &Window, node: blitz_dom::NodeId, root: blitz_dom::NodeId| {
        let doc = window.doc.inner.borrow();
        std::iter::successors(Some(node), |&id| doc.get_node(id).and_then(|n| n.parent))
            .any(|id| id == root)
    };
    tab_to(&mut window, "View details", |window, node| {
        window.attr(node, "data-part").as_deref() == Some("details")
            && window
                .find("data-cs", "hero")
                .is_some_and(|hero| inside(window, node, hero))
    });
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("the hero's game", |window| {
        matches!(window.route(), Route::Game { ref from, .. } if from.as_deref() == Some("/"))
    });
    window.settle_until("its way back", |window| {
        window.find("data-part", "back").is_some()
    });
    tab_to(&mut window, "Back to Home", |window, node| {
        window.attr(node, "data-part").as_deref() == Some("back")
    });
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("Home again", |window| {
        window.route() == Route::Home && window.find("data-focus-section", "shelf-added").is_some()
    });
    tab_to(
        &mut window,
        "a card on New in your library",
        |window, node| {
            window
                .find("data-focus-section", "shelf-added")
                .is_some_and(|shelf| inside(window, node, shelf))
                && window
                    .find_all("data-cs", "game-card")
                    .into_iter()
                    .any(|card| inside(window, node, card))
        },
    );
    // Enter on a card opens its quick look, as a click does.
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("the card's quick look", |window| {
        window.find("data-cs", "quick-look").is_some()
    });
    window.key(Key::Named(NamedKey::Escape));
    window.settle_until("the quick look closed", |window| {
        window.find("data-cs", "quick-look").is_none()
    });
    window.settle_until("Home's shelves again", |window| {
        window
            .find("data-focus-section", "shelf-released")
            .is_some()
    });
    tab_to(&mut window, "a shelf's View all", |window, node| {
        window.attr(node, "data-part").as_deref() == Some("view-all")
    });
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("the library", |window| {
        matches!(window.route(), Route::Library(_))
    });
}

#[test]
fn pad_down_from_hero_play_reaches_recently_played_view_all() {
    use caskade_ui_input::Action;
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = home();
    let hero = window.get("data-cs", "hero");
    let play = window.find_in(hero, "data-part", "primary-action").unwrap();
    window.focus(play);
    assert_eq!(window.focused(), Some(play));
    window.pad(Action::NavDown);
    let focused = window.focused().expect("something focused after NavDown");
    assert_eq!(
        window.text(focused).trim(),
        "View all",
        "focused was {:?}",
        window.text(focused)
    );
}
