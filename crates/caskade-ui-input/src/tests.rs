//! The webview's input tests (`input.test.tsx`, `keyboard.test.tsx`),
//! ported over a hand-built tree with the same boxes, and a fake UI that
//! acts as the webview's controls do: a dialog opens on its trigger's click
//! and closes on Escape, a select's popover picks with a click, inputs
//! toggle and sliders step.

use crate::bindings::Binding;
use crate::dispatch::{Dispatcher, Ui};
use crate::keys::Modifiers;
use crate::osk::{Field, Keyboard, Layer, Outcome, Shift};
use crate::tree::test_tree::TestTree;
use crate::tree::{FocusTree, Rect};
use crate::{Action, GlyphFamily, InputSource};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&mut Fake)>;
type KeyHandler = Rc<dyn Fn(&mut Fake, &str) -> bool>;

/// The UI the dispatcher drives in these tests.
#[derive(Default)]
struct Fake {
    tree: TestTree,
    focus: Option<usize>,
    values: HashMap<usize, String>,
    checked: HashSet<usize>,
    backs: usize,
    /// The field the on-screen keyboard opened on.
    keyboard: Option<usize>,
    /// The history entry shown, as the router's location.
    location: Option<String>,
    clicks: HashMap<usize, Handler>,
    keys: HashMap<usize, KeyHandler>,
}

impl FocusTree for Fake {
    type Node = usize;
    fn root(&self) -> usize {
        self.tree.root()
    }
    fn parent(&self, node: usize) -> Option<usize> {
        self.tree.parent(node)
    }
    fn children(&self, node: usize) -> Vec<usize> {
        self.tree.children(node)
    }
    fn tag(&self, node: usize) -> &str {
        self.tree.tag(node)
    }
    fn attr(&self, node: usize, name: &str) -> Option<&str> {
        self.tree.attr(node, name)
    }
    fn rect(&self, node: usize) -> Rect {
        self.tree.rect(node)
    }
    fn text(&self, node: usize) -> String {
        self.tree.text(node)
    }
}

impl Fake {
    /// The handler on `node` or the nearest ancestor with one: events
    /// bubble.
    fn bubble<H: Clone>(&self, map: &HashMap<usize, H>, node: usize) -> Vec<H> {
        let mut found = Vec::new();
        let mut at = Some(node);
        while let Some(here) = at {
            if let Some(handler) = map.get(&here) {
                found.push(handler.clone());
            }
            at = self.tree.parent(here);
        }
        found
    }
}

impl Ui for Fake {
    fn focused(&self) -> Option<usize> {
        self.focus
    }
    fn focus(&mut self, node: usize, _instant: bool) {
        self.focus = Some(node);
    }
    fn blur(&mut self) {
        self.focus = None;
        self.keyboard = None;
    }
    fn press(&mut self, node: usize, key: &str) -> bool {
        for handler in self.bubble(&self.keys, node) {
            if handler(self, key) {
                return true;
            }
        }
        if self.tree.attr(node, "type") == Some("range") {
            let value: i32 = self.values.get(&node).map_or(50, |v| v.parse().unwrap());
            let step = match key {
                "ArrowRight" => 1,
                "ArrowLeft" => -1,
                _ => return false,
            };
            self.values.insert(node, (value + step).to_string());
            return true;
        }
        false
    }
    fn click(&mut self, node: usize) {
        match self.tree.attr(node, "type") {
            Some("checkbox") if !self.checked.remove(&node) => {
                self.checked.insert(node);
            }
            Some("radio") => {
                let name = self.tree.attr(node, "name").map(str::to_owned);
                self.checked
                    .retain(|&other| self.tree.attr(other, "name").map(str::to_owned) != name);
                self.checked.insert(node);
            }
            _ => {}
        }
        if let Some(handler) = self.bubble(&self.clicks, node).first() {
            handler(self);
        }
    }
    fn value(&self, node: usize) -> String {
        self.values.get(&node).cloned().unwrap_or_default()
    }
    fn go_back(&mut self) {
        self.backs += 1;
    }
    fn scroller(&self, _node: Option<usize>) -> Option<usize> {
        None
    }
    fn scroll_by(&mut self, _node: usize, _by: f32) {}
    fn open_keyboard(&mut self, field: usize) {
        self.keyboard = Some(field);
    }
}

/// How a test drives the app: a pad, or the keyboard map's keys.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Driver {
    Pad,
    Keyboard,
}

fn key_of(action: Action) -> &'static str {
    match action {
        Action::NavUp => "ArrowUp",
        Action::NavDown => "ArrowDown",
        Action::NavLeft => "ArrowLeft",
        Action::NavRight => "ArrowRight",
        Action::Confirm => "Enter",
        Action::Back => "Escape",
        Action::TabNext => "]",
        other => panic!("no key for {other:?}"),
    }
}

struct App {
    ui: Fake,
    input: Dispatcher<usize>,
    /// The focus the dispatcher last heard of.
    seen: Option<usize>,
    location: Option<String>,
}

impl App {
    fn new(ui: Fake, console: bool) -> Self {
        let mut app = Self {
            ui,
            input: Dispatcher::new(console),
            seen: None,
            location: None,
        };
        app.settle();
        app
    }

    /// What the window does between events: focus changes reach the
    /// dispatcher, the location's too, then a frame passes.
    fn settle(&mut self) {
        for _ in 0..40 {
            if self.ui.location != self.location {
                self.location = self.ui.location.clone();
                if let Some(key) = &self.location {
                    self.input.navigated(&self.ui, key);
                }
            }
            if self.ui.focus != self.seen {
                self.seen = self.ui.focus;
                if let Some(node) = self.seen {
                    self.input.focused_in(&self.ui, node);
                }
            }
            self.input.frame(&mut self.ui);
            if self.ui.focus == self.seen && !self.input.pending() {
                return;
            }
        }
    }

    fn pad(&mut self, action: Action) {
        self.input.dispatch(
            &mut self.ui,
            action,
            InputSource::Pad,
            Some(GlyphFamily::Xbox),
        );
        self.settle();
    }

    /// A key press; whether the input layer took it.
    fn key(&mut self, key: &str) -> bool {
        let taken = self.input.key(&mut self.ui, key, Modifiers::default());
        self.settle();
        taken
    }

    fn press(&mut self, driver: Driver, action: Action) {
        match driver {
            Driver::Pad => self.pad(action),
            Driver::Keyboard => assert!(self.key(key_of(action)), "{action:?} not taken"),
        }
    }

    /// Focus put somewhere by the user (a click, the view).
    fn focus(&mut self, node: usize) {
        self.ui.focus = Some(node);
        self.settle();
    }

    fn click(&mut self, node: usize) {
        self.ui.click(node);
        self.settle();
    }

    fn by_text(&self, text: &str) -> usize {
        self.ui.tree.by_text(text)
    }

    fn focused(&self) -> String {
        self.ui
            .focus
            .map(|node| self.ui.text(node))
            .unwrap_or_default()
    }
}

/// Sidebar on the left; content with a shelf, a grid, a dialog, a select
/// and a text field.
fn layout() -> Fake {
    let mut ui = Fake::default();
    let t = &mut ui.tree;
    let sidebar = t.section(0, "sidebar");
    for (i, label) in ["Home", "Library", "Settings"].iter().enumerate() {
        t.button(sidebar, &format!("0 {} 200 40", i * 50), label);
    }
    let main = t.section(0, "main");
    let shelf = t.section(main, "shelf");
    for (i, label) in ["A", "B", "C"].iter().enumerate() {
        t.button(
            shelf,
            &format!("{} 0 140 80", 300 + i * 150),
            &format!("Shelf {label}"),
        );
    }
    let grid = t.section(main, "grid");
    for i in 0..6 {
        let rect = format!("{} {} 140 80", 300 + (i % 3) * 150, 100 + (i / 3) * 100);
        t.button(grid, &rect, &format!("Card {i}"));
    }
    let open = t.button(main, "300 320 140 40", "Open dialog");
    let sort = t.button(main, "460 320 140 40", "Title");
    t.add(
        main,
        "input",
        &[("data-rect", "300 380 300 40"), ("aria-label", "Search")],
        "",
    );
    ui.clicks
        .insert(open, Rc::new(move |ui: &mut Fake| open_dialog(ui, open)));
    ui.clicks
        .insert(sort, Rc::new(move |ui: &mut Fake| open_select(ui, sort)));
    ui
}

/// A modal dialog, focused on itself as React Aria opens it; Escape closes
/// it and focus goes back to its trigger.
fn open_dialog(ui: &mut Fake, trigger: usize) {
    let dialog = ui.tree.add(
        0,
        "section",
        &[
            ("role", "dialog"),
            ("tabindex", "-1"),
            ("aria-label", "Confirm"),
        ],
        "",
    );
    ui.tree.button(dialog, "600 400 100 40", "Yes");
    ui.tree.button(dialog, "720 400 100 40", "No");
    ui.keys.insert(
        dialog,
        Rc::new(move |ui: &mut Fake, key: &str| {
            if key != "Escape" {
                return false;
            }
            ui.tree.remove(dialog);
            ui.focus = Some(trigger);
            true
        }),
    );
    ui.focus = Some(dialog);
}

/// A select's popover: its options stack, the selected one takes focus, a
/// click picks and Escape leaves.
fn open_select(ui: &mut Fake, trigger: usize) {
    let popover = ui.tree.add(0, "div", &[("data-placement", "bottom")], "");
    let list = ui.tree.add(popover, "div", &[("role", "listbox")], "");
    let chosen = ui.tree.text(trigger);
    for (i, label) in ["Title", "Recent", "Playtime"].iter().enumerate() {
        let rect = format!("600 {} 200 36", 300 + i * 40);
        let option = ui.tree.add(
            list,
            "div",
            &[("role", "option"), ("data-rect", &rect)],
            label,
        );
        let value = label.to_string();
        ui.clicks.insert(
            option,
            Rc::new(move |ui: &mut Fake| {
                ui.tree.elements[trigger].text = value.clone();
                ui.tree.remove(popover);
                ui.focus = Some(trigger);
            }),
        );
        if *label == chosen {
            ui.focus = Some(option);
        }
    }
    ui.keys.insert(
        popover,
        Rc::new(move |ui: &mut Fake, key: &str| {
            if key != "Escape" {
                return false;
            }
            ui.tree.remove(popover);
            ui.focus = Some(trigger);
            true
        }),
    );
}

const DRIVERS: [Driver; 2] = [Driver::Pad, Driver::Keyboard];

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

/// A grid of cards that open a details view, which goes back to the grid.
fn grid_view(ui: &mut Fake, main: usize) {
    for child in ui.tree.children(main) {
        ui.tree.remove(child);
    }
    for i in 0..3 {
        let card = ui
            .tree
            .button(main, &format!("{} 0 140 80", i * 150), &format!("Card {i}"));
        ui.clicks
            .insert(card, Rc::new(move |ui: &mut Fake| details_view(ui, main)));
    }
    ui.location = Some("grid".into());
}

fn details_view(ui: &mut Fake, main: usize) {
    for child in ui.tree.children(main) {
        ui.tree.remove(child);
    }
    let back = ui.tree.button(main, "0 0 140 40", "Back to grid");
    ui.clicks
        .insert(back, Rc::new(move |ui: &mut Fake| grid_view(ui, main)));
    ui.location = Some("details".into());
}

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

/// A field in console mode, as `keyboard.test.tsx` mounts it.
fn field_app() -> (App, usize) {
    let mut ui = Fake::default();
    let form = ui.tree.add(0, "form", &[], "");
    let field = ui
        .tree
        .add(form, "input", &[("data-rect", "0 0 300 40")], "");
    let mut app = App::new(ui, true);
    app.focus(field);
    (app, field)
}

#[test]
fn a_on_a_field_opens_the_keyboard_keys_press_enter_without_it() {
    let (mut app, field) = field_app();
    // A key's Enter is the field's own (a native submit), not ours.
    assert!(!app.key("Enter"));
    assert_eq!(app.ui.keyboard, None);
    app.pad(Action::Confirm);
    assert_eq!(app.ui.keyboard, Some(field));
}

#[test]
fn a_tap_in_console_mode_opens_the_keyboard() {
    let (mut app, field) = field_app();
    assert!(app.input.tap(&mut app.ui, field));
    assert_eq!(app.ui.keyboard, Some(field));
    let mut desktop = App::new(Fake::default(), false);
    let field = desktop
        .ui
        .tree
        .add(0, "input", &[("data-rect", "0 0 300 40")], "");
    assert!(!desktop.input.tap(&mut desktop.ui, field));
}

#[test]
fn a_read_only_field_or_a_checkbox_gets_no_keyboard() {
    let mut ui = Fake::default();
    let locked = ui.tree.add(
        0,
        "input",
        &[("data-rect", "0 0 300 40"), ("readonly", "")],
        "",
    );
    let mut app = App::new(ui, true);
    app.focus(locked);
    app.pad(Action::Confirm);
    assert_eq!(app.ui.keyboard, None);
    assert!(!crate::osk::types_text(&app.ui, locked));
}

/// The keyboard over a field, driven by pad actions.
struct Typing {
    keyboard: Keyboard,
    field: Field,
}

impl Typing {
    fn new(locale: &str) -> Self {
        Self {
            keyboard: Keyboard::new(locale),
            field: Field::new(""),
        }
    }

    fn pad(&mut self, action: Action) -> Option<Outcome> {
        self.keyboard.action(action, &mut self.field)
    }

    fn tap_shift(&mut self) {
        let rows = self.keyboard.rows();
        let col = rows[3]
            .iter()
            .position(|key| key.name() == "shift")
            .unwrap();
        self.keyboard.tap(3, col, &mut self.field);
    }

    fn row(&self, row: usize) -> String {
        self.keyboard.rows()[row]
            .iter()
            .map(|key| key.name())
            .collect()
    }
}

#[test]
fn the_pad_types_deletes_spaces_and_moves_the_cursor() {
    let mut typing = Typing::new("en");
    // It starts on q.
    typing.pad(Action::Confirm);
    typing.pad(Action::NavRight);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw");
    // Up from w is 2: the columns line up.
    typing.pad(Action::NavUp);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw2");
    typing.pad(Action::Details);
    assert_eq!(typing.field.value, "qw");
    typing.pad(Action::Search);
    assert_eq!(typing.field.value, "qw ");
    // RB: symbols, on the same key position (@ above w's column).
    typing.pad(Action::TabNext);
    assert_eq!(typing.keyboard.layer, Layer::Symbols);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw @");
    // LT moves the cursor back; typing goes in there.
    typing.pad(Action::PagePrev);
    typing.pad(Action::PagePrev);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw@ @");
}

#[test]
fn shift_types_one_capital_twice_locks_and_start_submits() {
    let mut typing = Typing::new("en");
    typing.tap_shift();
    assert_eq!(typing.keyboard.shift, Shift::Once);
    // Up from shift to s, then up again to w.
    typing.pad(Action::NavUp);
    typing.pad(Action::NavUp);
    // Once in capitals, then lower case again.
    typing.pad(Action::Confirm);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "Ww");
    assert_eq!(typing.keyboard.shift, Shift::Off);
    typing.tap_shift();
    typing.tap_shift();
    assert_eq!(typing.keyboard.shift, Shift::Lock);
    // The tap moved the pad's place to shift; back up to w.
    typing.pad(Action::NavUp);
    typing.pad(Action::NavUp);
    typing.pad(Action::Confirm);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "WwWW");
    assert_eq!(typing.pad(Action::Menu), Some(Outcome::Submit));
}

#[test]
fn b_closes_the_keyboard_and_keeps_the_text() {
    let mut typing = Typing::new("en");
    typing.pad(Action::Confirm);
    assert_eq!(typing.pad(Action::Back), Some(Outcome::Close));
    assert_eq!(typing.field.value, "q");
    // The guide button is the app's, not the keyboard's.
    assert_eq!(typing.pad(Action::Guide), None);
}

#[test]
fn french_is_azerty_and_a_pad_types_its_accents() {
    let mut typing = Typing::new("fr");
    assert_eq!(typing.row(1), "azertyuiopéè");
    assert_eq!(typing.row(2), "qsdfghjklmàç");
    // It starts on a; left wraps round to è, then é.
    typing.pad(Action::Confirm);
    typing.pad(Action::NavLeft);
    typing.pad(Action::Confirm);
    typing.pad(Action::NavLeft);
    typing.pad(Action::Confirm);
    // Down from é is à, and ç is beside it.
    typing.pad(Action::NavDown);
    typing.pad(Action::Confirm);
    typing.pad(Action::NavRight);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "aèéàç");
    // Shift gives the capital: from shift up to s, z, then left to é.
    typing.tap_shift();
    typing.pad(Action::NavUp);
    typing.pad(Action::NavUp);
    for _ in 0..3 {
        typing.pad(Action::NavLeft);
    }
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "aèéàçÉ");
    // The other accents are a row of the symbols.
    typing.pad(Action::TabNext);
    assert_eq!(typing.row(3), "âêîôûëïüùœ:_");
}

#[test]
fn german_is_qwertz_and_swedish_has_its_letters() {
    let german = Typing::new("de-AT");
    assert_eq!(german.row(1), "qwertzuiopü-");
    assert_eq!(german.row(2), "asdfghjklöä.");
    assert_eq!(german.row(3), "shiftyxcvbnm,ß'");
    let mut swedish = Typing::new("sv");
    assert_eq!(swedish.row(1), "qwertyuiopå-");
    // From q, left twice wraps round to å; down to ä, left to ö.
    swedish.pad(Action::NavLeft);
    swedish.pad(Action::NavLeft);
    swedish.pad(Action::Confirm);
    swedish.pad(Action::NavDown);
    swedish.pad(Action::Confirm);
    swedish.pad(Action::NavLeft);
    swedish.pad(Action::Confirm);
    assert_eq!(swedish.field.value, "åäö");
}

#[test]
fn a_language_without_a_layout_gets_english() {
    assert_eq!(Typing::new("ja").row(1), "qwertyuiop-_");
}

#[test]
fn paste_makes_one_line_for_a_one_line_field() {
    assert_eq!(crate::osk::pasted("\nhttps://a\n", false), "https://a");
    assert_eq!(crate::osk::pasted("a  \r\n  b\nc", false), "a b c");
    assert_eq!(crate::osk::pasted("a  b", false), "a  b");
    assert_eq!(crate::osk::pasted("a\nb", true), "a\nb");
    let mut field = Field::new("ab");
    field.move_caret(-1);
    field.insert(&crate::osk::pasted("x\ny", false));
    assert_eq!((field.value.as_str(), field.start), ("ax yb", 4));
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
