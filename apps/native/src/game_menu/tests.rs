//! Games' menus and quick looks (#99) in the frame over the fixture
//! library, headless, as `menus.test.tsx` and `quickLook.test.tsx` have
//! them, and the menu's items worked out from what a game has.

use super::entries::{EntryView, added_by_caskade, located, playable};
use super::menu::{Facts, Target, items};
use crate::frame::tests::{ONE, Window};
use crate::router::Route;
use crate::ui::MenuEntry;
use caskade_core::model::PlaySession;
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use std::collections::HashSet;

fn entry(id: &str, source: &str, installed: bool, location: Option<&str>) -> EntryView {
    EntryView {
        id: id.into(),
        source: source.into(),
        installed,
        location: location.map(str::to_owned),
        can_launch: installed,
        emulated: false,
        stream: false,
    }
}

fn labels(menu: &[MenuEntry]) -> Vec<String> {
    menu.iter()
        .flat_map(|entry| match entry {
            MenuEntry::Item(item) => vec![item.id.clone()],
            MenuEntry::Section { items, .. } => items.iter().map(|item| item.id.clone()).collect(),
            MenuEntry::Separator => vec!["—".into()],
        })
        .collect()
}

fn target(entry_id: Option<&str>) -> Target {
    Target {
        game_id: "g".into(),
        title: "Tidewater".into(),
        entry_id: entry_id.map(str::to_owned),
    }
}

#[test]
fn play_starts_the_default_entry_when_it_can_else_the_first_that_can() {
    let entries = vec![
        entry("a", "steam", false, None),
        entry("b", "caskade-storage", true, Some("/games/b")),
        entry("c", "manual", true, Some("/games/c")),
    ];
    let none = HashSet::new();
    assert_eq!(playable(&entries, &none, Some("c")).unwrap().id, "c");
    assert_eq!(playable(&entries, &none, Some("a")).unwrap().id, "b");
    let away: HashSet<String> = ["b".to_owned()].into();
    assert_eq!(playable(&entries, &away, None).unwrap().id, "c");
    // One folder item per folder.
    let shared = vec![
        entry("x", "lutris", true, Some("/games/x")),
        entry("y", "manual", true, Some("/games/x")),
    ];
    assert_eq!(located(&shared).len(), 1);
    assert!(added_by_caskade(&entries[1..]));
    assert!(!added_by_caskade(&entries));
}

#[test]
fn an_entry_rows_menu_offers_play_its_folder_move_and_uninstall() {
    let facts = Facts {
        entries: vec![
            entry(
                "managed",
                "caskade-storage",
                true,
                Some("Tidewater in game storage"),
            ),
            entry("other", "steam", true, Some("/steam/Tidewater")),
        ],
        storage: true,
        ..Facts::default()
    };
    // The webview's `menus.test.tsx`: exactly these for a row.
    assert_eq!(
        labels(&items(&target(Some("managed")), &facts)),
        [
            "play",
            "—",
            "reveal:managed",
            "copy-path:managed",
            "move",
            "uninstall:managed"
        ]
    );
}

#[test]
fn a_running_game_offers_stop_and_its_files_wait_until_it_quits() {
    let session: PlaySession = serde_json::from_value(serde_json::json!({
        "id": "s", "gameId": "g", "entryId": "managed", "startedAtMs": 0,
        "endedAtMs": null, "durationMs": 0, "heartbeatAtMs": 0
    }))
    .unwrap_or_else(|error| panic!("a session: {error}"));
    let sessions = [session];
    let facts = Facts {
        entries: vec![entry("managed", "caskade-storage", true, Some("here"))],
        sessions: &sessions,
        storage: true,
        ..Facts::default()
    };
    let menu = items(&target(None), &facts);
    let ids = labels(&menu);
    assert_eq!(ids[0], "stop");
    let disabled: Vec<String> = menu
        .iter()
        .flat_map(|entry| match entry {
            MenuEntry::Section { items, .. } => items
                .iter()
                .filter(|item| item.disabled)
                .map(|item| item.id.clone())
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    assert!(disabled.contains(&"move".into()), "{disabled:?}");
    assert!(disabled.contains(&"uninstall:managed".into()));
    assert!(disabled.contains(&"remove".into()));
}

/// The library's grid, with its first game's card.
fn library() -> Window {
    let mut window = Window::open();
    window.go("/library?filter=all&sort=title&layout=grid");
    window.settle_until("a card", |window| {
        !window.find_all("data-index", "0").is_empty()
    });
    window
}

#[test]
fn a_click_on_a_card_opens_its_quick_look_beside_the_library() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = library();
    let card = window.get("data-index", "0");
    window.click(card);
    window.settle_until("the quick look", |window| {
        window.find("data-cs", "quick-look").is_some()
    });
    assert!(
        matches!(window.route(), Route::Library(_)),
        "it stays on the library"
    );
    let look = window.get("data-cs", "quick-look");
    let text = window.text(look);
    assert!(
        text.contains("Play") && text.contains("View more"),
        "{text}"
    );
    // Another card shows that one, in the one panel.
    let second = window.get("data-index", "1");
    window.click(second);
    window.settle();
    assert_eq!(window.find_all("data-cs", "quick-look").len(), 1);
    // Close closes it.
    let look = window.get("data-cs", "quick-look");
    let close = window.find_in(look, "data-part", "close").expect("Close");
    window.click(close);
    assert!(window.find("data-cs", "quick-look").is_none());
    // View more opens the game's page.
    let card = window.get("data-index", "0");
    window.click(card);
    window.settle_until("the quick look", |window| {
        window.find("data-cs", "quick-look").is_some()
    });
    let more = window.get("data-part", "open");
    window.click(more);
    assert!(
        matches!(window.route(), Route::Game { .. }),
        "{}",
        window.href()
    );
    assert!(
        window.find("data-cs", "quick-look").is_none(),
        "a page change closes it"
    );
}

#[test]
fn a_right_click_opens_a_games_menu_whose_actions_work() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = library();
    let card = window.get("data-index", "0");
    window.right_click(card);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
    let menu = window.get("role", "menu");
    let text = window.text(menu);
    for item in ["Play", "Open game details", "Add to collection…"] {
        assert!(text.contains(item), "{item} in {text}");
    }
    let favourite = if text.contains("Add to favourites") {
        "Add to favourites"
    } else {
        "Remove from favourites"
    };
    let item = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains(favourite))
        .unwrap();
    window.click(item);
    // Reopened, it says the other.
    let card = window.get("data-index", "0");
    window.right_click(card);
    window.settle_until("the favourite changed", |window| {
        window.find("role", "menu").is_some_and(|menu| {
            let text = window.text(menu);
            text.contains("Add to favourites") != (favourite == "Add to favourites")
        })
    });
    // Open game details: its page.
    let details = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains("Open game details"))
        .unwrap();
    window.click(details);
    assert!(
        matches!(window.route(), Route::Game { .. }),
        "{}",
        window.href()
    );
}

#[test]
fn add_to_collection_makes_a_collection_with_the_game_in_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = library();
    let card = window.get("data-index", "0");
    window.right_click(card);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
    let item = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains("Add to collection…"))
        .unwrap();
    window.click(item);
    window.settle_until("the picker", |window| {
        window.find("data-cs", "collection-picker").is_some()
    });
    let picker = window.get("data-cs", "collection-picker");
    let field = window
        .find_in(picker, "data-part", "input")
        .expect("the name field");
    window.focus(field);
    window.type_text("Weekend");
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("the new collection, ticked", |window| {
        window
            .find("data-cs", "collection-picker")
            .is_some_and(|picker| window.text(picker).contains("Weekend"))
            && window
                .find_all("role", "checkbox")
                .into_iter()
                .any(|checkbox| window.attr(checkbox, "aria-checked").as_deref() == Some("true"))
    });
}

#[test]
fn a_pad_opens_the_focused_games_menu_and_quick_look() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = library();
    let card = window.get("data-index", "0");
    window.focus(card);
    window.pad(Action::Menu);
    window.settle_until("the menu", |window| window.find("role", "menu").is_some());
    window.pad(Action::Back);
    window.settle_until("the menu closed", |window| {
        window.find("role", "menu").is_none()
    });
    let card = window.get("data-index", "0");
    window.focus(card);
    window.pad(Action::Details);
    window.settle_until("the quick look", |window| {
        window.find("data-cs", "quick-look").is_some()
    });
}

#[test]
fn a_double_click_plays_and_says_why_it_didnt_start() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = library();
    let card = window.get("data-index", "0");
    window.double_click(card);
    // Fixtures mode has no launch service: it says so, and stays on the
    // library.
    window.settle_until("why it didn't start", |window| {
        window
            .find("role", "alertdialog")
            .is_some_and(|dialog| window.text(dialog).contains("didn't start"))
    });
    assert!(matches!(window.route(), Route::Library(_)));
}
