//! Choosing several games in the library and editing them at once (#99),
//! headless over the fixture library, as `selection.test.tsx` has it.

use crate::frame::tests::{ONE, Window, source};
use crate::router::Route;
use blitz_dom::NodeId;
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, Modifiers, NamedKey};

/// The library's grid, sorted by title, once its games are on screen.
fn open() -> Window {
    let mut window = Window::open();
    window.go("/library?filter=all&sort=title&layout=grid");
    window.settle_until("the library's games", |window| {
        !window.find_all("data-index", "3").is_empty()
    });
    window
}

/// The card of the game at `index`.
fn card(window: &Window, index: usize) -> NodeId {
    window
        .find_all("data-index", &index.to_string())
        .into_iter()
        .find(|&node| window.attr(node, "data-cs").as_deref() == Some("game-card"))
        .unwrap_or_else(|| panic!("no card at {index}"))
}

/// What the selection bar says it holds.
fn count(window: &Window) -> String {
    window
        .find("data-cs", "selection-bar")
        .and_then(|bar| window.find_in(bar, "data-part", "count"))
        .map(|count| window.text(count))
        .unwrap_or_default()
}

/// The selection bar's `part`.
fn bar_part(window: &Window, part: &str) -> NodeId {
    let bar = window.get("data-cs", "selection-bar");
    window
        .find_in(bar, "data-part", part)
        .unwrap_or_else(|| panic!("no {part} in the selection bar"))
}

/// The toolbar's Select (cards in selection mode have a `select` part too).
fn select_button(window: &Window) -> NodeId {
    let toolbar = window.get("data-part", "toolbar");
    window
        .find_in(toolbar, "data-part", "select")
        .expect("the toolbar's Select")
}

fn start(window: &mut Window) {
    let select = select_button(window);
    window.click(select);
    window.settle_until("the selection bar", |window| {
        window.find("data-cs", "selection-bar").is_some()
    });
}

#[test]
fn games_are_chosen_by_click_and_range_and_all_of_them() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open();
    assert!(window.find("data-cs", "selection-bar").is_none());
    start(&mut window);
    assert_eq!(
        window
            .attr(select_button(&window), "aria-pressed")
            .as_deref(),
        Some("true")
    );
    assert!(
        count(&window).contains("Choose games"),
        "{}",
        count(&window)
    );

    // A click chooses a game (no quick look), Shift+click the ones between.
    let first = card(&window, 0);
    window.click(first);
    let fourth = card(&window, 3);
    window.click_with(fourth, Modifiers::SHIFT);
    window.settle_until("four chosen", |window| {
        count(window).contains("4 games selected")
    });
    assert!(window.find("data-cs", "quick-look").is_none());
    assert!(matches!(window.route(), Route::Library(_)));
    assert_eq!(
        window.attr(card(&window, 2), "data-selected").as_deref(),
        Some("true")
    );
    assert_eq!(
        window.attr(card(&window, 2), "aria-pressed").as_deref(),
        Some("true")
    );

    // Clicking a chosen game again lets it go.
    let second = card(&window, 1);
    window.click(second);
    window.settle_until("three chosen", |window| {
        count(window).contains("3 games selected")
    });
    assert_eq!(window.attr(card(&window, 1), "data-selected"), None);

    // Every game the view finds: as many as the button says.
    let all = bar_part(&window, "select-all");
    let label = window.text(all);
    let total: usize = label
        .strip_prefix("Select all ")
        .and_then(|rest| rest.strip_suffix(" games"))
        .and_then(|number| number.replace(',', "").parse().ok())
        .unwrap_or_else(|| panic!("{label}"));
    assert!(total > 4, "{label}");
    window.click(all);
    let all_chosen = format!("{total} games selected");
    window.settle_until("all chosen", |window| count(window).contains(&all_chosen));

    // Clear keeps choosing; Done ends it.
    let clear = bar_part(&window, "clear");
    window.click(clear);
    window.settle_until("none chosen", |window| {
        count(window).contains("Choose games")
    });
    let done = bar_part(&window, "done");
    window.click(done);
    window.settle_until("the bar gone", |window| {
        window.find("data-cs", "selection-bar").is_none()
    });
    assert_eq!(window.attr(card(&window, 0), "aria-pressed"), None);

    // A pad's Back ends it too.
    start(&mut window);
    window.pad(Action::Back);
    window.settle_until("the bar gone", |window| {
        window.find("data-cs", "selection-bar").is_none()
    });
    assert!(matches!(window.route(), Route::Library(_)));
}

#[test]
fn chosen_games_take_one_edit_after_a_preview() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = open();
    start(&mut window);
    let ids: Vec<String> = (0..2)
        .map(|index| {
            let node = card(&window, index);
            window.click(node);
            window
                .attr(node, "data-focus-key")
                .and_then(|key| key.strip_prefix("game-").map(str::to_owned))
                .expect("the card's game")
        })
        .collect();
    window.settle_until("two chosen", |window| {
        count(window).contains("2 games selected")
    });
    let edit = bar_part(&window, "edit");
    window.click(edit);
    window.settle_until("the bulk editor", |window| {
        window.find("data-cs", "bulk-editor").is_some()
    });
    let apply = window.get("data-part", "apply");
    assert_eq!(window.attr(apply, "data-disabled").as_deref(), Some("true"));

    // Tick Tags and add one.
    let tags = window
        .find_all("data-part", "field")
        .into_iter()
        .find(|&field| window.attr(field, "data-field").as_deref() == Some("tags"))
        .expect("the tags field");
    let tick = window.find_in(tags, "role", "checkbox").expect("its tick");
    window.click(tick);
    window.settle_until("the tags input", |window| {
        window.find("aria-label", "Add to Tags").is_some()
    });
    let input = window
        .find_all("aria-label", "Add to Tags")
        .into_iter()
        .find(|&node| window.attr(node, "data-part").as_deref() == Some("input"))
        .expect("the tags field's input");
    window.focus(input);
    window.type_text("co-op");
    window.key(Key::Named(NamedKey::Enter));

    // Preview, then apply.
    let apply = window.get("data-part", "apply");
    window.click(apply);
    window.settle_until("the preview", |window| {
        window.find("data-step", "preview").is_some()
    });
    let results = window.text(window.get("data-part", "results"));
    assert!(results.contains("2 games change"), "{results}");
    let apply = window.get("data-part", "apply");
    window.click(apply);
    window.settle_until("saved", |window| window.find("data-step", "done").is_some());
    let results = window.text(window.get("data-part", "results"));
    assert!(results.contains("2 games saved"), "{results}");
    for id in &ids {
        let game = caskade_app::runtime::block_on(source().library.game(id))
            .unwrap()
            .expect("the game");
        assert!(
            game.game.tags.iter().any(|tag| tag == "co-op"),
            "{id}: {:?}",
            game.game.tags
        );
    }
    let apply = window.get("data-part", "apply");
    window.click(apply);
    window.settle_until("the editor closed", |window| {
        window.find("data-cs", "bulk-editor").is_none()
    });
    // Closing it keeps the games chosen.
    assert!(count(&window).contains("2 games selected"));
}
