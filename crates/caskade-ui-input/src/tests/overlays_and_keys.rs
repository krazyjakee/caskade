use super::{App, DRIVERS, Fake, layout};
use crate::Action;
use crate::bindings::Binding;
use crate::keys::Modifiers;
use crate::tree::FocusTree;
use std::cell::Cell;
use std::rc::Rc;

/// Counts a binding's runs.
fn counter() -> (Rc<Cell<u32>>, impl Fn() -> bool + 'static) {
    let count = Rc::new(Cell::new(0));
    let runs = count.clone();
    (count, move || {
        runs.set(runs.get() + 1);
        true
    })
}

fn hints(app: &App) -> String {
    let within = crate::spatial::scope(&app.ui);
    let overlay = (within != app.ui.root()).then_some(within);
    app.input
        .bindings()
        .borrow()
        .active(&app.ui, app.ui.focus, overlay)
        .iter()
        .map(|hint| format!("{}={}", action_name(hint.action), hint.label))
        .collect::<Vec<_>>()
        .join(",")
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::TabNext => "tab.next",
        Action::Details => "details",
        _ => "other",
    }
}

#[test]
fn opens_and_closes_a_dialog_staying_inside_it_while_open() {
    for driver in DRIVERS {
        let mut app = App::new(layout(), true);
        let open = app.by_text("Open dialog");
        app.focus(open);
        app.press(driver, Action::Confirm);
        // Focus starts on the dialog's first control, not the dialog itself.
        assert_eq!(app.focused(), "Yes", "{driver:?}");
        app.press(driver, Action::NavRight);
        assert_eq!(app.focused(), "No");
        // Nothing outside the dialog is reachable.
        app.press(driver, Action::NavLeft);
        app.press(driver, Action::NavLeft);
        assert_eq!(app.focused(), "Yes");
        app.press(driver, Action::Back);
        assert!(!crate::spatial::has_overlay(&app.ui));
        assert_eq!(app.ui.backs, 0);
        assert_eq!(app.focused(), "Open dialog");
        // With no overlay, back goes back.
        app.press(driver, Action::Back);
        assert_eq!(app.ui.backs, 1, "{driver:?}");
    }
}

#[test]
fn opens_a_select_with_confirm_picks_with_the_pad_and_closes_with_back() {
    for driver in DRIVERS {
        let mut app = App::new(layout(), true);
        let trigger = app.by_text("Title");
        app.focus(trigger);
        app.press(driver, Action::Confirm);
        let option = app.ui.focus.unwrap();
        assert_eq!(app.ui.attr(option, "role"), Some("option"), "{driver:?}");
        app.press(driver, Action::NavDown);
        assert_eq!(app.focused(), "Recent");
        app.press(driver, Action::Confirm);
        assert!(!crate::spatial::has_overlay(&app.ui));
        assert_eq!(app.ui.text(trigger), "Recent");
        // Open again and leave with back: nothing changes.
        app.focus(trigger);
        app.press(driver, Action::Confirm);
        assert!(crate::spatial::has_overlay(&app.ui));
        app.press(driver, Action::Back);
        assert!(!crate::spatial::has_overlay(&app.ui));
        assert_eq!(app.ui.text(trigger), "Recent", "{driver:?}");
    }
}

#[test]
fn text_fields_keep_their_typing_keys_up_and_down_leave_and_escape_steps_out() {
    let mut app = App::new(layout(), true);
    let field = app
        .ui
        .tree
        .elements
        .iter()
        .position(|e| e.tag == "input")
        .unwrap();
    app.focus(field);
    for key in [
        "a",
        "ArrowLeft",
        "ArrowRight",
        "Enter",
        "[",
        "i",
        "Backspace",
    ] {
        assert!(!app.key(key), "{key} was taken");
        assert_eq!(app.ui.focus, Some(field));
    }
    assert!(app.key("Escape"));
    assert_ne!(app.ui.focus, Some(field));
    app.focus(field);
    assert!(app.key("ArrowUp"));
    assert_eq!(app.focused(), "Open dialog");
}

#[test]
fn desktop_mode_leaves_the_keyboard_alone_until_a_pad_is_used() {
    let mut app = App::new(layout(), false);
    let card = app.by_text("Card 0");
    app.focus(card);
    assert!(!app.key("ArrowRight"));
    assert_eq!(app.focused(), "Card 0");
    app.pad(Action::NavRight);
    assert_eq!(app.focused(), "Card 1");
    // After a pad action the keyboard map is on too.
    assert!(app.key("ArrowRight"));
    assert_eq!(app.focused(), "Card 2");
}

#[test]
fn modifiers_leave_keys_alone() {
    let mut app = App::new(layout(), true);
    let card = app.by_text("Card 0");
    app.focus(card);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    assert!(!app.input.key(&mut app.ui, "ArrowRight", ctrl));
    assert_eq!(app.focused(), "Card 0");
}

#[test]
fn the_innermost_binding_wins_and_the_active_ones_are_listed() {
    let mut ui = Fake::default();
    let shelf = ui.tree.section(0, "shelf");
    let inside = ui.tree.button(shelf, "0 0 100 40", "In shelf");
    let outside = ui.tree.button(0, "0 100 100 40", "Outside");
    let mut app = App::new(ui, true);
    let (on_view, view) = counter();
    let (on_shelf, shelf_run) = counter();
    {
        let bindings = app.input.bindings();
        let mut bindings = bindings.borrow_mut();
        bindings.add(Binding::new(Action::TabNext, view).label("Next tab"));
        bindings.add(
            Binding::new(Action::TabNext, shelf_run)
                .label("Next page")
                .owner(shelf),
        );
        bindings.add(Binding::new(Action::Details, || true).label("Details"));
    }
    app.focus(inside);
    app.pad(Action::TabNext);
    assert_eq!((on_shelf.get(), on_view.get()), (1, 0));
    assert_eq!(hints(&app), "tab.next=Next page,details=Details");
    app.focus(outside);
    app.pad(Action::TabNext);
    assert_eq!(on_view.get(), 1);
    assert_eq!(hints(&app), "details=Details,tab.next=Next tab");
}

#[test]
fn the_views_bindings_are_ignored_while_an_overlay_is_open() {
    let mut app = App::new(layout(), true);
    let (on_back, back) = counter();
    app.input
        .bindings()
        .borrow_mut()
        .add(Binding::new(Action::Back, back));
    let open = app.by_text("Open dialog");
    app.focus(open);
    app.pad(Action::Confirm);
    assert!(crate::spatial::has_overlay(&app.ui));
    app.pad(Action::Back);
    assert!(!crate::spatial::has_overlay(&app.ui));
    assert_eq!(on_back.get(), 0);
    app.pad(Action::Back);
    assert_eq!(on_back.get(), 1);
}

#[test]
fn a_binding_that_declines_lets_the_next_one_run() {
    let mut app = App::new(layout(), true);
    let (declined, _) = counter();
    let seen = declined.clone();
    let (taken, take) = counter();
    {
        let bindings = app.input.bindings();
        let mut bindings = bindings.borrow_mut();
        bindings.add(Binding::new(Action::Menu, take));
        bindings.add(Binding::new(Action::Menu, move || {
            seen.set(seen.get() + 1);
            false
        }));
    }
    app.pad(Action::Menu);
    assert_eq!((declined.get(), taken.get()), (1, 1));
}

#[test]
fn binding_update_tracks_changes_and_preserves_registration_order() {
    let mut ui = Fake::default();
    let shelf = ui.tree.section(0, "shelf");
    let inside = ui.tree.button(shelf, "0 0 100 40", "In shelf");
    let mut app = App::new(ui, true);
    let (first_count, first_run) = counter();
    let (second_count, second_run) = counter();
    let (first_id, second_id) = {
        let bindings = app.input.bindings();
        let mut bindings = bindings.borrow_mut();
        let rev0 = bindings.revision();
        let first_id = bindings.add(Binding::new(Action::TabNext, first_run).label("First"));
        assert_eq!(bindings.revision(), rev0 + 1);
        // No-op update does not bump revision.
        assert!(!bindings.update(first_id, None, Some("First".into()), false));
        assert_eq!(bindings.revision(), rev0 + 1);
        // Changing fields bumps revision once.
        assert!(bindings.update(first_id, Some(shelf), Some("First updated".into()), true));
        assert_eq!(bindings.revision(), rev0 + 2);
        let second_id = bindings.add(
            Binding::new(Action::TabNext, second_run)
                .label("Second")
                .owner(shelf),
        );
        (first_id, second_id)
    };
    app.focus(inside);
    // Same owner depth: newer binding (second_id) wins deterministically.
    app.pad(Action::TabNext);
    assert_eq!((first_count.get(), second_count.get()), (0, 1));
    assert_eq!(hints(&app), "tab.next=Second");

    // Removing second_id falls back to first_id; repeated removal is a no-op.
    {
        let bindings = app.input.bindings();
        let mut bindings = bindings.borrow_mut();
        let rev = bindings.revision();
        bindings.remove(second_id);
        assert_eq!(bindings.revision(), rev + 1);
        bindings.remove(second_id);
        assert_eq!(bindings.revision(), rev + 1);
        assert!(!bindings.update(second_id, None, None, false));
        assert_eq!(bindings.revision(), rev + 1);
    }
    app.pad(Action::TabNext);
    assert_eq!((first_count.get(), second_count.get()), (1, 1));
    assert_eq!(hints(&app), "tab.next=First updated");
    app.input.bindings().borrow_mut().remove(first_id);
}

#[test]
fn an_overlay_that_opens_without_focus_gets_it_and_gives_it_back_when_it_closes() {
    let mut ui = Fake::default();
    let main = ui.tree.section(0, "main");
    let open = ui.tree.button(main, "0 0 140 40", "Open");
    ui.clicks.insert(
        open,
        Rc::new(|ui: &mut Fake| {
            // The native UI's dialogs: rendered, not focused.
            let dialog = ui.tree.add(0, "div", &[("role", "dialog")], "");
            ui.tree.button(dialog, "600 400 100 40", "Cancel");
            let delete = ui.tree.button(dialog, "720 400 100 40", "Delete");
            ui.tree.set(delete, "data-focus-default", "");
            let close = Rc::new(move |ui: &mut Fake| ui.tree.remove(dialog));
            ui.clicks.insert(delete, close.clone());
            ui.clicks.insert(ui.tree.by_text("Cancel"), close);
        }),
    );
    let mut app = App::new(ui, false);
    app.focus(open);
    app.click(open);
    assert_eq!(app.focused(), "Delete");
    app.pad(Action::NavLeft);
    assert_eq!(app.focused(), "Cancel");
    app.pad(Action::Confirm);
    assert!(!crate::spatial::has_overlay(&app.ui));
    assert_eq!(app.focused(), "Open");
}

#[test]
fn typing_outside_a_field_a_menu_or_a_dialog_starts_a_search() {
    use crate::keys::starts_typing;
    let mut app = App::new(layout(), false);
    let none = Modifiers::default();
    let card = app.by_text("Card 0");
    let field = app
        .ui
        .tree
        .elements
        .iter()
        .position(|e| e.tag == "input")
        .unwrap();
    assert_eq!(
        starts_typing(&app.ui, Some(card), "F", none),
        Some("F".into())
    );
    assert_eq!(starts_typing(&app.ui, None, "8", none), Some("8".into()));
    // `/` opens it empty.
    assert_eq!(
        starts_typing(&app.ui, Some(card), "/", none),
        Some(String::new())
    );
    for key in [" ", "ArrowUp", "Enter", "F3"] {
        assert_eq!(starts_typing(&app.ui, Some(card), key, none), None, "{key}");
    }
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    assert_eq!(starts_typing(&app.ui, Some(card), "f", ctrl), None);
    assert_eq!(
        starts_typing(&app.ui, Some(field), "f", none),
        None,
        "a field types"
    );
    // Not over a dialog or a popover.
    let open = app.by_text("Open dialog");
    app.focus(open);
    app.pad(Action::Confirm);
    let yes = app.ui.focus.unwrap();
    assert_eq!(starts_typing(&app.ui, Some(yes), "f", none), None);
}
