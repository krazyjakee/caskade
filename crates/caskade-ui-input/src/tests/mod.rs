//! Input tests over a hand-built tree and fake UI: a dialog opens on its
//! trigger's click and closes on Escape, a select's popover picks with a
//! click, inputs toggle and sliders step.

mod lifecycle;
mod navigation;
mod osk;
mod overlays_and_keys;

use crate::dispatch::{Dispatcher, Ui};
use crate::keys::Modifiers;
use crate::tree::test_tree::TestTree;
use crate::tree::{FocusTree, Rect};
use crate::{Action, GlyphFamily, InputSource};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub(super) type Handler = Rc<dyn Fn(&mut Fake)>;
pub(super) type KeyHandler = Rc<dyn Fn(&mut Fake, &str) -> bool>;

/// The UI the dispatcher drives in these tests.
#[derive(Default)]
pub(super) struct Fake {
    pub(super) tree: TestTree,
    pub(super) focus: Option<usize>,
    /// Controls whose layout is visible before the toolkit can focus them.
    pub(super) pending_focus: HashSet<usize>,
    pub(super) values: HashMap<usize, String>,
    pub(super) checked: HashSet<usize>,
    pub(super) backs: usize,
    /// The field the on-screen keyboard opened on.
    pub(super) keyboard: Option<usize>,
    /// The history entry shown, as the router's location.
    pub(super) location: Option<String>,
    pub(super) clicks: HashMap<usize, Handler>,
    pub(super) keys: HashMap<usize, KeyHandler>,
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
    fn visit_children(&self, node: usize, visit: &mut dyn FnMut(usize) -> bool) -> bool {
        self.tree.visit_children(node, visit)
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
        if !self.pending_focus.contains(&node) {
            self.focus = Some(node);
        }
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
pub(super) enum Driver {
    Pad,
    Keyboard,
}

pub(super) fn key_of(action: Action) -> &'static str {
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

pub(super) struct App {
    pub(super) ui: Fake,
    pub(super) input: Dispatcher<usize>,
    /// The focus the dispatcher last heard of.
    seen: Option<usize>,
    location: Option<String>,
}

impl App {
    pub(super) fn new(ui: Fake, console: bool) -> Self {
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
    pub(super) fn settle(&mut self) {
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

    pub(super) fn pad(&mut self, action: Action) {
        self.input.dispatch(
            &mut self.ui,
            action,
            InputSource::Pad,
            Some(GlyphFamily::Xbox),
        );
        self.settle();
    }

    /// A key press; whether the input layer took it.
    pub(super) fn key(&mut self, key: &str) -> bool {
        let taken = self.input.key(&mut self.ui, key, Modifiers::default());
        self.settle();
        taken
    }

    pub(super) fn press(&mut self, driver: Driver, action: Action) {
        match driver {
            Driver::Pad => self.pad(action),
            Driver::Keyboard => assert!(self.key(key_of(action)), "{action:?} not taken"),
        }
    }

    /// Focus put somewhere by the user (a click, the view).
    pub(super) fn focus(&mut self, node: usize) {
        self.ui.focus = Some(node);
        self.settle();
    }

    pub(super) fn click(&mut self, node: usize) {
        self.ui.click(node);
        self.settle();
    }

    pub(super) fn by_text(&self, text: &str) -> usize {
        self.ui.tree.by_text(text)
    }

    pub(super) fn focused(&self) -> String {
        self.ui
            .focus
            .map(|node| self.ui.text(node))
            .unwrap_or_default()
    }
}

/// Sidebar on the left; content with a shelf, a grid, a dialog, a select
/// and a text field.
pub(super) fn layout() -> Fake {
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

/// A modal dialog that takes focus on itself when opened; Escape closes it
/// and focus goes back to its trigger.
pub(super) fn open_dialog(ui: &mut Fake, trigger: usize) {
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
pub(super) fn open_select(ui: &mut Fake, trigger: usize) {
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

/// A grid of cards that open a details view, which goes back to the grid.
pub(super) fn grid_view(ui: &mut Fake, main: usize) {
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

pub(super) fn details_view(ui: &mut Fake, main: usize) {
    for child in ui.tree.children(main) {
        ui.tree.remove(child);
    }
    let back = ui.tree.button(main, "0 0 140 40", "Back to grid");
    ui.clicks
        .insert(back, Rc::new(move |ui: &mut Fake| grid_view(ui, main)));
    ui.location = Some("details".into());
}

pub(super) const DRIVERS: [Driver; 2] = [Driver::Pad, Driver::Keyboard];
