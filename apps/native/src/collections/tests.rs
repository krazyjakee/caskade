//! Collections (#102) in the frame over the fixture library, headless, as
//! `collections.test.tsx` has them: the list, pinned first, its tabs and a
//! collection's page with its games; making one and adding games; a
//! smart one's rules; pinning and deleting from the menus; Play Random.
//! Putting a game in a collection from its menu is the game menu's
//! (`game_menu::collections`).
//!
//! The fixture library is every test's: what a test makes, it deletes.

use crate::frame::tests::{ONE, Window, source};
use crate::router::{CollectionFilter, Route};
use blitz_dom::NodeId;
use caskade_core::model::{CollectionKind, GameFilter, GameSort, ListGames, SaveCollection};
use caskade_ui_input::Action;

/// Collections a test made, deleted when it ends, passing or not.
#[derive(Default)]
struct Made(Vec<String>);

impl Made {
    fn save(&mut self, name: &str, kind: CollectionKind) -> String {
        let saved =
            caskade_app::runtime::block_on(source().library.save_collection(&SaveCollection {
                id: None,
                name: name.to_owned(),
                description: String::new(),
                kind,
            }))
            .unwrap();
        self.0.push(saved.id.clone());
        saved.id
    }

    /// A manual collection with `games` in it.
    fn manual(&mut self, name: &str, games: &[&str]) -> String {
        let id = self.save(name, CollectionKind::Manual);
        let games: Vec<String> = games.iter().map(|&game| game.to_owned()).collect();
        caskade_app::runtime::block_on(source().library.add_to_collection(&id, &games)).unwrap();
        id
    }

    /// One the view made: deleted at the end too.
    fn adopt(&mut self, id: &str) {
        self.0.push(id.to_owned());
    }
}

impl Drop for Made {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            let _ = caskade_app::runtime::block_on(source().library.delete_collection(&id));
        }
    }
}

/// The titles of the collection cards shown, in order.
fn card_titles(window: &Window) -> Vec<String> {
    window
        .find_all("data-cs", "collection-card")
        .into_iter()
        .filter_map(|card| window.find_in(card, "data-part", "title"))
        .map(|title| window.text(title))
        .collect()
}

/// The collection card titled `title`.
fn card(window: &Window, title: &str) -> Option<NodeId> {
    window
        .find_all("data-cs", "collection-card")
        .into_iter()
        .find(|&card| {
            window
                .find_in(card, "data-part", "title")
                .is_some_and(|node| window.text(node) == title)
        })
}

/// The element with `name`=`value` whose text is `text`, under `root` if
/// given.
fn with_text(
    window: &Window,
    root: Option<NodeId>,
    name: &str,
    value: &str,
    text: &str,
) -> Option<NodeId> {
    let nodes = match root {
        Some(root) => window.find_all_in(root, name, value),
        None => window.find_all(name, value),
    };
    nodes
        .into_iter()
        .find(|&node| window.text(node).trim() == text)
}

/// The button saying `text`, under `root` if given.
fn button(window: &Window, root: Option<NodeId>, text: &str) -> NodeId {
    with_text(window, root, "type", "button", text)
        .unwrap_or_else(|| panic!("no button {text} on {}", window.href()))
}

/// The collection page's text.
fn page_text(window: &Window) -> String {
    window
        .find("data-view", "collection")
        .map(|view| window.text(view))
        .unwrap_or_default()
}

/// The game card titles under the collection page's grid.
fn game_titles(window: &Window) -> Vec<String> {
    let Some(view) = window.find("data-view", "collection") else {
        return Vec::new();
    };
    window
        .find_all_in(view, "data-cs", "game-card")
        .into_iter()
        .filter_map(|card| window.find_in(card, "data-part", "title"))
        .map(|title| window.text(title))
        .collect()
}

#[test]
fn lists_collections_pinned_first_and_opens_one_with_its_games() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut made = Made::default();
    let weekend = made.manual("Weekend Picks", &["fixture-1", "fixture-2", "fixture-3"]);
    let favourites = made.save(
        "Favourites",
        CollectionKind::Smart {
            query: ListGames {
                filter: GameFilter::Favorites,
                ..ListGames::default()
            },
        },
    );
    caskade_app::runtime::block_on(source().library.set_collection_pinned(&favourites, true))
        .unwrap();

    let mut window = Window::open();
    window.go("/collections");
    window.settle_until("the collections", |window| {
        let titles = card_titles(window);
        titles.contains(&"Favourites".to_owned()) && titles.contains(&"Weekend Picks".to_owned())
    });
    let titles = card_titles(&window);
    let at = |title: &str| titles.iter().position(|shown| shown == title).unwrap();
    assert!(at("Favourites") < at("Weekend Picks"), "{titles:?}");
    let pinned = card(&window, "Favourites").unwrap();
    assert_eq!(window.attr(pinned, "data-kind").as_deref(), Some("smart"));
    assert!(window.find_in(pinned, "data-part", "pin").is_some());
    let weekend_card = card(&window, "Weekend Picks").unwrap();
    assert!(window.find_in(weekend_card, "data-part", "pin").is_none());
    let count = window.find_in(weekend_card, "data-part", "count").unwrap();
    assert_eq!(window.text(count), "3");

    // The Manual tab leaves the smart one out.
    let manual = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| window.text(tab).starts_with("Manual"))
        .expect("the Manual tab");
    window.click(manual);
    assert_eq!(
        window.route(),
        Route::Collections {
            filter: CollectionFilter::Manual
        }
    );
    window.settle_until("the manual ones", |window| {
        !card_titles(window).contains(&"Favourites".to_owned())
    });
    assert!(card_titles(&window).contains(&"Weekend Picks".to_owned()));
    // LB and RB step through the tabs: from Manual round to All.
    window.pad(Action::TabNext);
    assert_eq!(
        window.route(),
        Route::Collections {
            filter: CollectionFilter::All
        }
    );
    window.pad(Action::TabPrev);
    assert_eq!(
        window.route(),
        Route::Collections {
            filter: CollectionFilter::Manual
        }
    );

    // A card opens its collection: its name, how many games, the games.
    let weekend_card = card(&window, "Weekend Picks").unwrap();
    window.click(weekend_card);
    assert_eq!(
        window.route(),
        Route::Collection {
            id: weekend.clone()
        }
    );
    window.settle_until("its games", |window| {
        game_titles(window).contains(&"Fixture Game 0002".to_owned())
    });
    let said = page_text(&window);
    assert!(said.contains("Weekend Picks"), "{said}");
    assert!(said.contains("Games (3)"), "{said}");
    assert!(window.find("data-part", "rules").is_none());
}

#[test]
fn makes_a_manual_collection_and_adds_games_to_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut made = Made::default();
    let mut window = Window::open();
    window.go("/collections");
    window.settle_until("the collections", |window| {
        window.find("data-part", "primary-action").is_some()
    });
    let primary = window.get("data-part", "primary-action");
    let new = button(&window, Some(primary), "New Collection");
    window.click(new);
    window.settle_until("the collection dialog", |window| {
        window.find("data-cs", "collection-dialog").is_some()
    });
    let dialog = window.get("data-cs", "collection-dialog");
    let name = window.find_in(dialog, "data-part", "input").expect("Name");
    window.focus(name);
    window.type_text("Couch Co-op");
    let create = window.find_in(dialog, "data-part", "save").expect("Create");
    window.click(create);
    window.settle_until("the new collection", |window| {
        matches!(window.route(), Route::Collection { .. })
    });
    let Route::Collection { id } = window.route() else {
        unreachable!()
    };
    made.adopt(&id);
    window.settle_until("its page", |window| {
        page_text(window).contains("This collection is empty")
    });
    assert!(page_text(&window).contains("Couch Co-op"));

    let add = button(&window, window.find("data-view", "collection"), "Add Games");
    window.click(add);
    window.settle_until("the picker", |window| {
        window.find("role", "dialog").is_some()
    });
    let picker = window.get("role", "dialog");
    assert!(window.text(picker).contains("Add games to Couch Co-op"));
    let search = window
        .find_in(picker, "data-part", "input")
        .expect("the search");
    window.focus(search);
    window.type_text("Fixture Game 0001");
    let ticked = |window: &Window| {
        let picker = window.get("role", "dialog");
        with_text(
            window,
            Some(picker),
            "role",
            "checkbox",
            "Fixture Game 0001",
        )
        .and_then(|checkbox| window.attr(checkbox, "aria-checked"))
    };
    window.settle_until("the game", |window| ticked(window).is_some());
    assert_eq!(ticked(&window).as_deref(), Some("false"));
    let checkbox = with_text(
        &window,
        Some(picker),
        "role",
        "checkbox",
        "Fixture Game 0001",
    )
    .unwrap();
    window.click(checkbox);
    window.settle_until("it ticked", |window| {
        ticked(window).as_deref() == Some("true")
    });
    let picker = window.get("role", "dialog");
    window.click(button(&window, Some(picker), "Done"));
    window.settle_until("one game", |window| page_text(window).contains("Games (1)"));
    let members = caskade_app::runtime::block_on(source().library.collection(&id))
        .unwrap()
        .unwrap()
        .game_ids;
    assert_eq!(members, ["fixture-0"]);
}

#[test]
fn a_smart_collections_page_shows_its_rules_and_games() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut made = Made::default();
    let query = ListGames {
        filter: GameFilter::Installed,
        sort: GameSort::Title,
        ..ListGames::default()
    };
    let installed = caskade_app::runtime::block_on(source().library.page(&ListGames {
        limit: 1,
        ..query.clone()
    }))
    .unwrap()
    .total;
    let id = made.save("Ready to Play", CollectionKind::Smart { query });
    let mut window = Window::open();
    window.go(&Route::Collection { id }.href());
    window.settle_until("its page", |window| {
        window.find("data-part", "rules").is_some()
    });
    let view = window.get("data-view", "collection");
    assert_eq!(window.attr(view, "data-kind").as_deref(), Some("smart"));
    let rules = window.get("data-part", "rules");
    assert!(
        window.text(rules).contains("Installed"),
        "{}",
        window.text(rules)
    );
    let heading = format!("Games ({installed})");
    window.settle_until("its count", |window| page_text(window).contains(&heading));
    let said = page_text(&window);
    assert!(said.contains("Sorted by title"), "{said}");
    assert!(said.contains("Live"), "{said}");
    assert!(said.contains("Edit Rules"), "{said}");
}

#[test]
fn pins_a_collection_from_its_cards_menu() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut made = Made::default();
    let id = made.manual("Pin Me", &["fixture-1"]);
    let mut window = Window::open();
    window.go("/collections");
    window.settle_until("the card", |window| card(window, "Pin Me").is_some());
    window.right_click(card(&window, "Pin Me").unwrap());
    window.settle_until("its menu", |window| {
        with_text(window, None, "role", "menuitem", "Pin to the top").is_some()
    });
    let pin = with_text(&window, None, "role", "menuitem", "Pin to the top").unwrap();
    window.click(pin);
    window.settle_until("the pin", |window| {
        card(window, "Pin Me")
            .is_some_and(|card| window.find_in(card, "data-part", "pin").is_some())
    });
    let pinned = caskade_app::runtime::block_on(source().library.collections())
        .unwrap()
        .into_iter()
        .find(|collection| collection.id == id)
        .unwrap()
        .pinned;
    assert!(pinned);
}

#[test]
fn deletes_a_collection_from_its_menu() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut made = Made::default();
    let id = made.manual("Doomed Picks", &["fixture-1"]);
    let mut window = Window::open();
    window.go(&Route::Collection { id: id.clone() }.href());
    window.settle_until("its page", |window| {
        page_text(window).contains("Doomed Picks")
    });
    let more = window.get("aria-label", "More");
    window.click(more);
    window.settle_until("its menu", |window| {
        with_text(window, None, "role", "menuitem", "Delete…").is_some()
    });
    let delete = with_text(&window, None, "role", "menuitem", "Delete…").unwrap();
    window.click(delete);
    window.settle_until("the confirmation", |window| {
        window.find("role", "alertdialog").is_some()
    });
    let confirm = window.get("role", "alertdialog");
    assert!(window.text(confirm).contains("Delete Doomed Picks?"));
    window.click(button(&window, Some(confirm), "Delete"));
    window.settle_until("the list", |window| {
        window.route()
            == Route::Collections {
                filter: CollectionFilter::All,
            }
    });
    window.settle_until("the list without it", |window| {
        window.find("data-view", "collections").is_some()
            && window
                .find_all("role", "status")
                .into_iter()
                .all(|status| !window.text(status).contains("Loading collections"))
            && !card_titles(window).contains(&"Doomed Picks".to_owned())
    });
    let gone = caskade_app::runtime::block_on(source().library.collection(&id)).unwrap();
    assert!(gone.is_none());
}

#[test]
fn plays_a_random_installed_game_from_the_collection() {
    use crate::game::launch_tests::{Script, launches, script};
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _script = script(Script::default());
    let mut made = Made::default();
    // Fixture 0 isn't installed: Play Random leaves it. The emulated
    // fixtures can launch.
    let id = made.manual(
        "Random Picks",
        &["fixture-0", "fixture-emulated-0-0", "fixture-emulated-0-1"],
    );
    let mut window = Window::open();
    window.go(&Route::Collection { id }.href());
    window.settle_until("its games", |window| {
        page_text(window).contains("Games (3)")
    });
    let play = window.get("data-part", "primary-action");
    let play = button(&window, Some(play), "Play Random");
    assert_eq!(
        window.attr(play, "data-focus-default").as_deref(),
        Some("true")
    );
    window.click(play);
    window.settle_until("the launch", |_| launches() >= 1);
}
