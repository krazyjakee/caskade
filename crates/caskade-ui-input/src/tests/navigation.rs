use super::{App, DRIVERS, Fake, layout};
use crate::Action;
use crate::dispatch::Ui;

#[test]
fn moves_between_sidebar_and_content_through_the_shelf_and_the_grid() {
    for driver in DRIVERS {
        let mut app = App::new(layout(), true);
        // Nothing focused: the content's first focusable.
        app.press(driver, Action::NavDown);
        assert_eq!(app.focused(), "Shelf A", "{driver:?}");
        app.press(driver, Action::NavRight);
        assert_eq!(app.focused(), "Shelf B");
        app.press(driver, Action::NavDown);
        assert_eq!(app.focused(), "Card 1");
        app.press(driver, Action::NavRight);
        app.press(driver, Action::NavDown);
        assert_eq!(app.focused(), "Card 5");
        // Along the row and into the sidebar; coming back, the content
        // returns to the card focus left it on, not the nearest one.
        app.press(driver, Action::NavLeft);
        app.press(driver, Action::NavLeft);
        assert_eq!(app.focused(), "Card 3");
        app.press(driver, Action::NavLeft);
        assert_eq!(app.focused(), "Settings");
        app.press(driver, Action::NavUp);
        app.press(driver, Action::NavUp);
        assert_eq!(app.focused(), "Home");
        app.press(driver, Action::NavRight);
        assert_eq!(app.focused(), "Card 3", "{driver:?}");
    }
}

#[test]
fn goes_across_a_list_row_to_its_button_not_to_a_filter_above_or_the_next_rows_wider_title() {
    let mut ui = Fake::default();
    ui.tree.button(0, "140 -60 120 40", "Installed");
    let game = ui.tree.button(0, "0 0 120 40", "Game 1");
    ui.tree.button(0, "1500 0 40 40", "Actions 1");
    ui.tree.button(0, "0 44 124 40", "Game 2");
    ui.tree.button(0, "1500 44 40 40", "Actions 2");
    let mut app = App::new(ui, true);
    app.focus(game);
    app.pad(Action::NavRight);
    assert_eq!(app.focused(), "Actions 1");
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "Actions 2");
    app.pad(Action::NavLeft);
    assert_eq!(app.focused(), "Game 2");
}

#[test]
fn custom_elements_are_focus_stops() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    let tile = ui.tree.add(
        main,
        "div",
        &[
            ("data-rect", "0 0 100 100"),
            ("tabindex", "0"),
            ("data-focus-key", "tile"),
        ],
        "tile",
    );
    let mut app = App::new(ui, true);
    app.pad(Action::NavDown);
    assert_eq!(app.ui.focus, Some(tile));
    let mark = crate::spatial::mark(&app.ui, tile);
    assert_eq!(mark.key.as_deref(), Some("tile"));
    assert_eq!(crate::spatial::find(&app.ui, &mark), Some(tile));
}

#[test]
fn form_controls_toggle_slide_and_choose_with_actions_only() {
    let mut ui = Fake::default();
    let t = &mut ui.tree;
    let main = t.section(0, "main");
    let label = t.add(main, "label", &[("data-rect", "0 0 200 40")], "Effects");
    let effects = t.add(
        label,
        "input",
        &[("type", "checkbox"), ("role", "switch")],
        "",
    );
    let label = t.add(main, "label", &[("data-rect", "0 50 200 40")], "Titles");
    let titles = t.add(label, "input", &[("type", "checkbox")], "");
    let track = t.add(main, "div", &[("data-rect", "0 100 200 40")], "");
    let thumb = t.add(track, "div", &[("data-rect", "90 100 20 40")], "");
    let scale = t.add(
        thumb,
        "input",
        &[("type", "range"), ("aria-label", "Scale")],
        "",
    );
    let group = t.add(main, "div", &[("role", "radiogroup")], "");
    let label = t.add(group, "label", &[("data-rect", "0 150 100 40")], "Compact");
    let compact = t.add(
        label,
        "input",
        &[("type", "radio"), ("name", "density")],
        "",
    );
    let label = t.add(group, "label", &[("data-rect", "110 150 100 40")], "Comfy");
    let comfy = t.add(
        label,
        "input",
        &[("type", "radio"), ("name", "density")],
        "",
    );
    ui.checked.insert(compact);
    let mut app = App::new(ui, true);
    app.pad(Action::NavDown);
    assert_eq!(app.ui.focus, Some(effects));
    app.pad(Action::Confirm);
    assert!(app.ui.checked.contains(&effects));
    app.pad(Action::NavDown);
    assert_eq!(app.ui.focus, Some(titles));
    app.pad(Action::Confirm);
    assert!(app.ui.checked.contains(&titles));
    app.pad(Action::NavDown);
    assert_eq!(app.ui.focus, Some(scale));
    app.pad(Action::NavRight);
    assert_eq!(app.ui.value(scale), "51");
    app.pad(Action::NavLeft);
    app.pad(Action::NavLeft);
    assert_eq!(app.ui.value(scale), "49");
    app.pad(Action::NavDown);
    assert_eq!(app.ui.focus, Some(compact));
    app.pad(Action::NavRight);
    app.pad(Action::Confirm);
    assert!(app.ui.checked.contains(&comfy));
    assert!(!app.ui.checked.contains(&compact));
}

#[test]
fn up_and_down_stay_in_the_area_focus_is_in_and_left_and_right_cross_over() {
    let mut ui = Fake::default();
    let sidebar = ui.tree.section(0, "sidebar");
    ui.tree.button(sidebar, "20 100 260 50", "Home");
    ui.tree.button(sidebar, "20 750 260 60", "Desktop mode");
    let main = ui.tree.section(0, "main");
    // At the content's left edge, the sidebar's last item is nearer below
    // than the switch at the end of the next row.
    let reset = ui.tree.button(main, "340 334 150 40", "Reset");
    ui.tree.button(main, "780 534 40 20", "Switch");
    let mut app = App::new(ui, true);
    app.focus(reset);
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "Switch");
    // With nothing further down in the content, down crosses over.
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "Desktop mode");
    app.pad(Action::NavRight);
    assert_eq!(app.focused(), "Switch");
}

#[test]
fn down_from_the_hero_goes_to_the_shelf_below_it_not_a_card_in_its_column_further_down() {
    let mut ui = Fake::default();
    let t = &mut ui.tree;
    let main = t.section(0, "main");
    let hero = t.button(main, "560 380 260 56", "Resume · View details");
    // Recently Played, a shelf across the view: its View all at the right,
    // one card at the left.
    let recent = t.add(
        main,
        "div",
        &[
            ("data-focus-section", "shelf-recent"),
            ("data-rect", "340 470 1200 350"),
        ],
        "",
    );
    t.button(recent, "1400 480 120 30", "View all →");
    t.button(recent, "340 530 200 280", "Pocket Quest");
    // Recently Added: its second card is under the hero's button.
    let added = t.add(
        main,
        "div",
        &[
            ("data-focus-section", "shelf-added"),
            ("data-rect", "340 850 1200 350"),
        ],
        "",
    );
    t.button(added, "1400 860 120 30", "View all → added");
    t.button(added, "340 910 200 280", "Pocket Quest added");
    let couch = t.button(added, "560 910 200 280", "Couch Runner");
    let mut app = App::new(ui, true);
    app.focus(hero);
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "View all →");
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "Pocket Quest");
    // Up is the same: the line just above, its shelf's View all.
    app.focus(couch);
    app.pad(Action::NavUp);
    assert_eq!(app.focused(), "View all → added");
}

#[test]
fn down_from_the_top_of_a_settings_panel_stays_in_it_not_on_the_tabs_beside_it() {
    let mut ui = Fake::default();
    let t = &mut ui.tree;
    let main = t.section(0, "main");
    // The tabs, a strip at the side, start lower than the panel's top.
    let tabs = t.add(
        main,
        "div",
        &[
            ("data-focus-section", "settings-nav"),
            ("data-rect", "340 140 260 800"),
        ],
        "",
    );
    t.button(tabs, "340 160 260 48", "General");
    t.button(tabs, "340 212 260 48", "Library Discovery");
    // The panel's full-width top row, then two columns of rows.
    let reset = t.button(main, "1380 100 160 32", "Reset to defaults");
    let row = t.add(
        main,
        "div",
        &[("data-cs", "setting-row"), ("data-rect", "640 230 440 64")],
        "",
    );
    t.button(row, "1020 250 44 24", "Launch on system startup");
    let row = t.add(
        main,
        "div",
        &[("data-cs", "setting-row"), ("data-rect", "1100 230 440 64")],
        "",
    );
    t.button(row, "1380 250 140 32", "When a game launches");
    let mut app = App::new(ui, true);
    app.focus(reset);
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "When a game launches");
    // Left along the row is the other column's switch, before the tabs.
    app.pad(Action::NavLeft);
    assert_eq!(app.focused(), "Launch on system startup");
}

#[test]
fn a_settings_row_is_one_stop_going_up_and_down_wherever_its_control_sits() {
    let mut ui = Fake::default();
    let t = &mut ui.tree;
    let main = t.section(0, "main");
    let reset = t.button(main, "340 334 150 40", "Reset");
    let row = t.add(
        main,
        "div",
        &[("data-cs", "setting-row"), ("data-rect", "340 500 520 60")],
        "",
    );
    t.button(row, "800 515 40 20", "Launch on startup");
    let row = t.add(
        main,
        "div",
        &[("data-cs", "setting-row"), ("data-rect", "340 580 520 60")],
        "",
    );
    t.button(row, "600 595 120 30", "Open to");
    t.button(row, "740 595 100 30", "Second");
    let mut app = App::new(ui, true);
    app.focus(reset);
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "Launch on startup");
    app.pad(Action::NavDown);
    assert_eq!(app.focused(), "Open to");
    // Within a row, the controls' own places.
    app.pad(Action::NavRight);
    assert_eq!(app.focused(), "Second");
    app.pad(Action::NavUp);
    assert_eq!(app.focused(), "Launch on startup");
}

#[test]
fn hidden_disabled_and_skipped_elements_are_not_stops() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    let first = ui.tree.button(main, "0 0 100 40", "First");
    let off = ui.tree.button(main, "0 50 100 40", "Disabled");
    ui.tree.set(off, "disabled", "");
    let gone = ui.tree.add(main, "div", &[("hidden", "")], "");
    ui.tree.button(gone, "0 100 100 40", "Hidden");
    let skip = ui.tree.button(main, "0 150 100 40", "Scroll box");
    ui.tree.set(skip, "data-spatial", "skip");
    ui.tree.add(
        main,
        "div",
        &[("tabindex", "-1"), ("data-rect", "0 200 100 40")],
        "Target",
    );
    let last = ui.tree.button(main, "0 250 100 40", "Last");
    let mut app = App::new(ui, true);
    app.focus(first);
    app.pad(Action::NavDown);
    assert_eq!(app.ui.focus, Some(last));
}

#[test]
fn focusables_and_scope_prune_hidden_inert_aria_hidden_and_detached_subtrees() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    let visible = ui.tree.button(main, "0 0 100 40", "Visible");

    let inert_box = ui.tree.add(main, "div", &[("inert", "")], "");
    let inert_inner = ui.tree.add(inert_box, "div", &[], "");
    ui.tree.button(inert_inner, "0 50 100 40", "Inert child");
    ui.tree.add(inert_inner, "div", &[("role", "dialog")], "");

    let aria_hidden = ui.tree.add(main, "div", &[("aria-hidden", "true")], "");
    let aria_inner = ui
        .tree
        .add(aria_hidden, "div", &[("aria-hidden", "false")], "");
    ui.tree.button(aria_inner, "0 100 100 40", "Aria hidden");

    let detached = ui.tree.add(main, "div", &[], "");
    ui.tree.button(detached, "0 150 100 40", "Detached");
    ui.tree.remove(detached);

    assert_eq!(crate::spatial::focusables(&ui, main), vec![visible]);
    assert_eq!(
        crate::spatial::focusables(&ui, inert_box),
        Vec::<usize>::new()
    );
    assert_eq!(
        crate::spatial::focusables(&ui, inert_inner),
        Vec::<usize>::new()
    );
    assert_eq!(
        crate::spatial::focusables(&ui, aria_inner),
        Vec::<usize>::new()
    );
    assert_eq!(
        crate::spatial::focusables(&ui, detached),
        Vec::<usize>::new()
    );
    // An inert overlay does not become the active scope.
    assert_eq!(crate::spatial::scope(&ui), 0);
}
