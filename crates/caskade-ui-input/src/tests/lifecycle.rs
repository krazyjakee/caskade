use super::{App, Fake, grid_view};
use crate::Action;
use crate::tree::FocusTree;

#[test]
fn focus_goes_back_to_the_card_that_opened_a_view() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    grid_view(&mut ui, main);
    let mut app = App::new(ui, true);
    assert_eq!(app.focused(), "Card 0");
    let card = app.by_text("Card 2");
    app.focus(card);
    app.click(card);
    assert_eq!(app.focused(), "Back to grid");
    let back = app.by_text("Back to grid");
    app.click(back);
    assert_eq!(app.focused(), "Card 2");
}

#[test]
fn console_mode_turned_on_after_the_view_mounted_places_its_focus() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    grid_view(&mut ui, main);
    // The window decides on console mode once its first view is there.
    let mut app = App::new(ui, false);
    assert_eq!(app.ui.focus, None);
    app.input.set_console(true);
    app.settle();
    assert_eq!(app.focused(), "Card 0");
    // Off and on again: focus stays where it is.
    let card = app.by_text("Card 2");
    app.focus(card);
    app.input.set_console(false);
    app.input.set_console(true);
    app.settle();
    assert_eq!(app.focused(), "Card 2");
}

#[test]
fn console_mode_waits_for_the_views_controls_before_the_sidebar() {
    let mut ui = Fake::default();
    let sidebar = ui.tree.section(0, "sidebar");
    ui.tree.button(sidebar, "0 0 100 40", "Home");
    let main = ui.tree.section(0, "main");
    ui.location = Some("home".into());
    let mut app = App::new(ui, false);
    app.input.set_console(true);
    // The view's data hasn't come: nothing yet.
    app.settle();
    assert_eq!(app.ui.focus, None);
    // It has: its default.
    app.ui.tree.button(main, "200 0 140 80", "Resume");
    let preferred = app.ui.tree.button(main, "200 100 140 80", "View");
    app.ui.tree.set(preferred, "data-focus-default", "");
    app.settle();
    assert_eq!(app.focused(), "View");
}

#[test]
fn a_views_focus_waits_for_its_data_after_a_navigation() {
    let mut ui = Fake::default();
    let sidebar = ui.tree.section(0, "sidebar");
    ui.tree.button(sidebar, "0 0 100 40", "Home");
    let main = ui.tree.section(0, "main");
    grid_view(&mut ui, main);
    let mut app = App::new(ui, true);
    assert_eq!(app.focused(), "Card 0");
    // A page whose content comes later.
    for child in app.ui.tree.children(main) {
        app.ui.tree.remove(child);
    }
    app.ui.location = Some("game".into());
    app.settle();
    assert_ne!(
        app.focused(),
        "Home",
        "not the sidebar while the page loads"
    );
    app.ui.tree.button(main, "200 0 140 80", "Play");
    app.settle();
    assert_eq!(app.focused(), "Play");
}

#[test]
fn a_navigation_retries_when_the_toolkit_cannot_focus_the_new_control_yet() {
    let mut ui = Fake::default();
    let sidebar = ui.tree.section(0, "sidebar");
    let settings = ui.tree.button(sidebar, "0 0 100 40", "Settings");
    let main = ui.tree.section(0, "main");
    let play = ui.tree.button(main, "200 0 140 40", "Play");
    ui.tree.set(play, "data-focus-default", "");
    ui.location = Some("game".into());
    let mut app = App::new(ui, true);
    app.focus(settings);
    app.ui.tree.remove(play);
    let general = app.ui.tree.button(main, "200 0 140 40", "General");
    app.ui.tree.set(general, "data-focus-default", "");
    // Blitz can expose a box before marking a new node as in-document;
    // its focus call then leaves focus on the sidebar.
    app.ui.pending_focus.insert(general);
    app.ui.location = Some("settings".into());
    app.settle();
    assert_eq!(app.focused(), "Settings");
    app.ui.pending_focus.remove(&general);
    app.settle();
    assert_eq!(app.focused(), "General");
}

#[test]
fn a_views_default_retries_when_the_toolkit_cannot_focus_it_yet() {
    for after_frame_limit in [false, true] {
        let mut ui = Fake::default();
        let main = ui.tree.section(0, "main");
        ui.tree.button(main, "0 300 140 40", "View all");
        ui.location = Some("home".into());
        let mut app = App::new(ui, true);
        if after_frame_limit {
            for _ in 0..10 {
                app.settle();
            }
        }
        let resume = app.ui.tree.button(main, "0 0 200 60", "Resume");
        app.ui.tree.set(resume, "data-focus-default", "");
        app.ui.pending_focus.insert(resume);
        app.settle();
        assert_eq!(app.focused(), "View all");
        app.ui.pending_focus.remove(&resume);
        app.settle();
        assert_eq!(app.focused(), "Resume");
    }
}

#[test]
fn focus_goes_back_to_the_view_when_what_had_it_before_a_menu_went_too() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    ui.tree.button(main, "0 0 140 40", "Play");
    ui.location = Some("game".into());
    let mut app = App::new(ui, true);
    assert_eq!(app.focused(), "Play");
    // Console mode's curtain over a running game takes focus itself.
    let curtain = app.ui.tree.add(
        0,
        "div",
        &[
            ("role", "dialog"),
            ("tabindex", "-1"),
            ("data-rect", "0 0 1600 1000"),
        ],
        "",
    );
    app.focus(curtain);
    // The quick menu over it.
    let menu = app.ui.tree.add(0, "div", &[("role", "dialog")], "");
    app.ui.tree.button(menu, "600 400 200 40", "Stop game");
    app.settle();
    assert_eq!(app.focused(), "Stop game");
    // The game exits: the curtain lifts under the menu, then the menu goes.
    app.ui.tree.remove(curtain);
    app.settle();
    app.ui.tree.remove(menu);
    app.settle();
    assert_eq!(app.focused(), "Play");
}

#[test]
fn a_views_default_takes_focus_when_it_comes_after_its_other_controls() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    ui.tree.button(main, "0 300 140 40", "View all");
    ui.location = Some("home".into());
    let mut app = App::new(ui, true);
    // Home's shelves came first; its hero, the default, after them.
    assert_eq!(app.focused(), "View all");
    let resume = app.ui.tree.button(main, "0 0 200 60", "Resume");
    app.ui.tree.set(resume, "data-focus-default", "");
    app.settle();
    assert_eq!(app.focused(), "Resume");
    // Moved by the user, focus stays where they put it.
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    ui.tree.button(main, "0 300 140 40", "View all");
    let other = ui.tree.button(main, "200 300 140 40", "Other");
    ui.location = Some("home".into());
    let mut app = App::new(ui, true);
    app.focus(other);
    let resume = app.ui.tree.button(main, "0 0 200 60", "Resume");
    app.ui.tree.set(resume, "data-focus-default", "");
    app.settle();
    assert_eq!(app.focused(), "Other");
}

#[test]
fn a_views_focus_looks_again_when_the_old_view_was_still_drawn() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    let first = ui.tree.button(main, "0 0 140 80", "Card 1");
    let card = ui.tree.button(main, "150 0 140 80", "Card 2");
    ui.location = Some("home".into());
    let mut app = App::new(ui, true);
    app.pad(Action::NavRight);
    assert_eq!(app.focused(), "Card 2");
    // The location changes a frame before the old view goes.
    app.ui.location = Some("game".into());
    app.settle();
    app.ui.tree.remove(first);
    app.ui.tree.remove(card);
    let play = app.ui.tree.button(main, "0 0 140 40", "Play");
    app.ui.tree.set(play, "data-focus-default", "");
    app.settle();
    assert_eq!(app.focused(), "Play");
}

#[test]
fn a_views_default_takes_focus_when_it_becomes_pressable_later() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    let back = ui.tree.button(main, "0 0 140 40", "Back");
    let play = ui.tree.button(main, "0 60 140 40", "Play");
    ui.tree.set(play, "data-focus-default", "");
    ui.tree.set(play, "aria-disabled", "true");
    ui.location = Some("game".into());
    let mut app = App::new(ui, true);
    assert_eq!(app.ui.focus, Some(back));
    // Long after the view's frames ran out (a slow emulator check).
    for _ in 0..10 {
        app.settle();
    }
    assert_eq!(app.ui.focus, Some(back));
    // Pressable now; the frame it asks for gives it focus.
    app.ui.tree.set(play, "aria-disabled", "false");
    app.settle();
    assert_eq!(app.ui.focus, Some(play));
}
