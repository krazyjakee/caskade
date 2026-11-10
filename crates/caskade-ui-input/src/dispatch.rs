//! The action dispatcher: pad actions and the keyboard map feed one
//! [`Dispatcher::dispatch`]. Ported from the webview's
//! `input/InputProvider.tsx`.
//!
//! Command bindings get each action first; otherwise directions move focus
//! spatially, `confirm` presses the focused control, `back` closes the open
//! overlay or goes back, and `page.*` scrolls. Desktop mode stays mouse and
//! keyboard first: the keyboard map is off until console mode or the first
//! pad action.

use crate::bindings::{self, Bindings};
use crate::keys::{Modifiers, is_slider, is_text_entry, key_action};
use crate::osk::types_text;
use crate::spatial::{self, Direction, FocusMark, Spatial, focusables, is_focusable, scope};
use crate::tree::{FocusTree, closest, contains, descendants};
use crate::{Action, GlyphFamily, InputSource};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// What the dispatcher does to the UI: the toolkit's side of input.
pub trait Ui: FocusTree {
    /// The element with focus, if any.
    fn focused(&self) -> Option<Self::Node>;
    /// Moves focus to `node`, shown as keyboard focus, and scrolls it into
    /// view: at once when `instant`, else smoothly unless motion is reduced.
    fn focus(&mut self, node: Self::Node, instant: bool);
    /// Takes focus off the focused element.
    fn blur(&mut self);
    /// Presses `key` (a DOM key name) on `node`; whether a handler took it.
    fn press(&mut self, node: Self::Node, key: &str) -> bool;
    /// Clicks `node`, as a pointer would.
    fn click(&mut self, node: Self::Node);
    /// The text a field holds.
    fn value(&self, node: Self::Node) -> String;
    /// `back` with no overlay and no binding: the previous view.
    fn go_back(&mut self);
    /// The scroll box around `node`, else the view's.
    fn scroller(&self, node: Option<Self::Node>) -> Option<Self::Node>;
    /// Scrolls `node` down by `by` pixels (up when negative); boxes show it
    /// at once.
    fn scroll_by(&mut self, node: Self::Node, by: f32);
    /// Opens the on-screen keyboard on `field`.
    fn open_keyboard(&mut self, field: Self::Node);
}

/// A dialog or alert dialog: it starts on its default control.
fn is_dialog<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    matches!(tree.attr(node, "role"), Some("dialog" | "alertdialog"))
}

/// Whether `node` drives `overlay` from outside it: a combobox keeps focus
/// in its input while its suggestions are open.
fn controls<T: FocusTree + ?Sized>(tree: &T, node: T::Node, overlay: T::Node) -> bool {
    ["aria-controls", "aria-activedescendant"]
        .iter()
        .any(|name| {
            tree.attr(node, name)
                .and_then(|id| by_id(tree, id))
                .is_some_and(|target| contains(tree, overlay, target))
        })
}

fn by_id<T: FocusTree + ?Sized>(tree: &T, id: &str) -> Option<T::Node> {
    descendants(tree, tree.root())
        .into_iter()
        .find(|&node| tree.attr(node, "id") == Some(id))
}

/// Putting focus back when a view comes back, or on its entry when one
/// opens: tried each frame until the view has rendered, for half a second.
#[derive(Debug, Clone)]
struct Restore {
    saved: Option<FocusMark>,
    frames: u32,
}

/// Frames to wait for a view to render before giving up on its focus.
const RESTORE_FRAMES: u32 = 30;

/// Input state for one window.
pub struct Dispatcher<N> {
    /// Console mode: navigation is always on.
    pub console: bool,
    pad_used: bool,
    source: Option<InputSource>,
    glyphs: GlyphFamily,
    /// What sent the action being handled: a pad or a tap types on the
    /// on-screen keyboard, keys type themselves.
    from: Option<InputSource>,
    pub spatial: Spatial<N>,
    bindings: Rc<RefCell<Bindings<N>>>,
    /// The last focus outside overlays: where the user is in the view.
    last_focus: Option<N>,
    /// Where `last_focus` was when it got focus, for when its view has
    /// already gone by the time the location changes.
    last_mark: Option<FocusMark>,
    /// Where focus was in each history entry, to put it back on return.
    marks: HashMap<String, FocusMark>,
    here: Option<String>,
    /// What focus was put on when a view opened, until the user moves it.
    entered: Option<N>,
    restore: Option<Restore>,
    /// A dialog that took focus itself, to move onto its default control.
    dialog: Option<N>,
}

impl<N: Copy + Eq + std::hash::Hash + std::fmt::Debug> Dispatcher<N> {
    pub fn new(console: bool) -> Self {
        Self {
            console,
            pad_used: false,
            source: None,
            glyphs: GlyphFamily::Generic,
            from: None,
            spatial: Spatial::default(),
            bindings: Rc::default(),
            last_focus: None,
            last_mark: None,
            marks: HashMap::new(),
            here: None,
            entered: None,
            restore: None,
            dialog: None,
        }
    }

    /// Spatial navigation and the keyboard map are on.
    pub fn active(&self) -> bool {
        self.console || self.pad_used
    }

    /// What sent the last action.
    pub fn source(&self) -> Option<InputSource> {
        self.source
    }

    /// The labels of the pad used last, for hints.
    pub fn glyphs(&self) -> GlyphFamily {
        self.glyphs
    }

    /// The command bindings; views add theirs here.
    pub fn bindings(&self) -> Rc<RefCell<Bindings<N>>> {
        self.bindings.clone()
    }

    /// What focus was put on when the current view opened.
    pub fn entered(&self) -> Option<N> {
        self.entered
    }

    /// Whether the action being handled types on the on-screen keyboard.
    pub fn types_on_screen(&self) -> bool {
        matches!(self.from, Some(InputSource::Pad | InputSource::Touch))
    }

    /// Opens the on-screen keyboard on `target`, a text field, when a pad
    /// or a tap sent the action being handled; whether it opened.
    pub fn keyboard<U: Ui<Node = N>>(&self, ui: &mut U, target: Option<N>) -> bool {
        let Some(target) = target else {
            return false;
        };
        if !self.types_on_screen() || !types_text(ui, target) {
            return false;
        }
        ui.open_keyboard(target);
        true
    }

    /// A tap in console mode: on a field, it opens the on-screen keyboard.
    pub fn tap<U: Ui<Node = N>>(&mut self, ui: &mut U, target: N) -> bool {
        if !self.console {
            return false;
        }
        self.from = Some(InputSource::Touch);
        let field = closest(ui, target, |at| matches!(ui.tag(at), "input" | "textarea"));
        self.keyboard(ui, field)
    }

    /// A key pressed: the action it gives, handled, while navigation is on.
    /// Whether it was taken (then the key does nothing else).
    pub fn key<U: Ui<Node = N>>(&mut self, ui: &mut U, key: &str, modifiers: Modifiers) -> bool {
        if !self.active() {
            return false;
        }
        let Some(action) = key_action(ui, ui.focused(), key, modifiers) else {
            return false;
        };
        self.dispatch(ui, action, InputSource::Keyboard, None);
        true
    }

    /// Handles `action` from `sender`: the bindings first, then the default.
    pub fn dispatch<U: Ui<Node = N>>(
        &mut self,
        ui: &mut U,
        action: Action,
        sender: InputSource,
        glyphs: Option<GlyphFamily>,
    ) {
        self.from = Some(sender);
        if sender == InputSource::Pad {
            self.pad_used = true;
            if let Some(glyphs) = glyphs {
                self.glyphs = glyphs;
            }
        }
        self.source = Some(sender);
        let root = ui.root();
        let within = scope(ui);
        let focused = ui.focused().filter(|&current| {
            current != root && (contains(ui, within, current) || controls(ui, current, within))
        });
        let overlay = (within != root).then_some(within);
        // Taken out of the registry first: a handler may change it.
        let handlers = self
            .bindings
            .borrow()
            .handlers(ui, action, focused, overlay);
        if bindings::run(&handlers) {
            return;
        }
        self.run_default(ui, action, focused);
    }

    /// Moves focus and remembers it in its sections.
    pub fn focus<U: Ui<Node = N>>(&mut self, ui: &mut U, node: N, instant: bool) {
        ui.focus(node, instant);
        self.spatial.remember(ui, node);
    }

    /// What an action does when no binding takes it.
    fn run_default<U: Ui<Node = N>>(&mut self, ui: &mut U, action: Action, focused: Option<N>) {
        if let Some(direction) = Direction::of(action) {
            let horizontal = matches!(direction, Direction::Left | Direction::Right);
            if let Some(node) = focused {
                // A combobox keeps focus in its input and moves through its
                // suggestions with the arrows (the search). Down opens them
                // once there's text; up only moves in an open list, otherwise
                // it leaves the field.
                let open = ui.attr(node, "aria-expanded") == Some("true");
                let typed = ui.tag(node) == "input" && !ui.value(node).trim().is_empty();
                if !horizontal
                    && ui.attr(node, "role") == Some("combobox")
                    && (open || (direction == Direction::Down && typed))
                {
                    let key = if direction == Direction::Down {
                        "ArrowDown"
                    } else {
                        "ArrowUp"
                    };
                    ui.press(node, key);
                    return;
                }
                if horizontal && is_slider(ui, node) {
                    let key = if direction == Direction::Left {
                        "ArrowLeft"
                    } else {
                        "ArrowRight"
                    };
                    ui.press(node, key);
                    return;
                }
            }
            let target = match focused {
                Some(node) => self.spatial.next(ui, node, direction),
                None => self.spatial.entry(ui),
            };
            if let Some(target) = target {
                self.focus(ui, target, false);
            }
            return;
        }
        match action {
            Action::Confirm => match focused {
                None => {
                    if let Some(target) = self.spatial.entry(ui) {
                        self.focus(ui, target, false);
                    }
                }
                Some(node) if is_text_entry(ui, node) => {
                    // A pad types on the on-screen keyboard, unless a
                    // suggestion is there to pick; keys press Enter.
                    let picking = ui.attr(node, "aria-expanded") == Some("true");
                    if picking || !self.keyboard(ui, Some(node)) {
                        ui.press(node, "Enter");
                    }
                }
                Some(node) => ui.click(node),
            },
            Action::Back => {
                if spatial::has_overlay(ui) {
                    let target = focused.unwrap_or_else(|| scope(ui));
                    ui.press(target, "Escape");
                } else if focused.is_some_and(|node| is_text_entry(ui, node)) {
                    ui.blur();
                } else {
                    ui.go_back();
                }
            }
            Action::PagePrev | Action::PageNext => {
                let Some(scroller) = ui.scroller(focused) else {
                    return;
                };
                let sign = if action == Action::PageNext {
                    1.0
                } else {
                    -1.0
                };
                let height = ui.rect(scroller).height();
                ui.scroll_by(scroller, sign * height * 0.9);
                let view = ui.rect(scroller);
                let visible = focusables(ui, scroller).into_iter().find(|&node| {
                    let rect = ui.rect(node);
                    rect.top >= view.top && rect.bottom <= view.bottom
                });
                if let Some(visible) = visible {
                    self.focus(ui, visible, true);
                }
            }
            _ => {}
        }
    }

    /// Focus arrived on `node`, by any means: remembered in its sections, and
    /// a dialog that took focus itself moves on to its default control.
    pub fn focused_in<U: Ui<Node = N>>(&mut self, ui: &U, node: N) {
        self.spatial.remember(ui, node);
        let in_overlay = closest(ui, node, |at| {
            is_dialog(ui, at) || ui.attr(at, "data-placement").is_some()
        });
        if in_overlay.is_none() {
            self.last_focus = Some(node);
            self.last_mark = Some(spatial::mark(ui, node));
        }
        if self.active() && is_dialog(ui, node) {
            self.dialog = Some(node);
        }
    }

    /// The view changed to history entry `key`: where focus was in the one
    /// left is kept, and focus goes back where it was in `key`, or to its
    /// entry, over the next frames.
    pub fn navigated<U: Ui<Node = N>>(&mut self, ui: &U, key: &str) {
        if self.here.as_deref() == Some(key) {
            return;
        }
        // Leaving `here`: where the user left focus, from the old view if
        // it's still on screen, else as it was when focus arrived there.
        if let Some(here) = &self.here {
            let left = match self.last_focus {
                Some(left) if contains(ui, ui.root(), left) => Some(spatial::mark(ui, left)),
                _ => self.last_mark.clone(),
            };
            if let Some(left) = left {
                self.marks.insert(here.clone(), left);
            }
        }
        self.here = Some(key.to_owned());
        let saved = self.marks.get(key).cloned();
        self.restore = (saved.is_some() || self.active()).then_some(Restore { saved, frames: 0 });
    }

    /// After each render: the pending dialog start and view focus.
    pub fn frame<U: Ui<Node = N>>(&mut self, ui: &mut U) {
        if let Some(dialog) = self.dialog.take()
            && ui.focused() == Some(dialog)
        {
            let start = descendants(ui, dialog)
                .into_iter()
                .find(|&node| ui.attr(node, "data-focus-default").is_some())
                .filter(|&preferred| is_focusable(ui, preferred))
                .or_else(|| focusables(ui, dialog).into_iter().next());
            if let Some(start) = start {
                self.focus(ui, start, false);
            }
        }
        let Some(mut restore) = self.restore.take() else {
            return;
        };
        // A dialog opened meanwhile: it has focus now, not the view.
        let current = ui.focused();
        if spatial::has_overlay(ui)
            || current.is_some_and(|node| closest(ui, node, |at| is_dialog(ui, at)).is_some())
        {
            return;
        }
        // The view already chose its focus: keep it.
        if restore.saved.is_none()
            && current.is_some()
            && current == self.entered
            && current.is_some_and(|node| contains(ui, ui.root(), node))
        {
            return;
        }
        let target = match &restore.saved {
            Some(saved) => spatial::find(ui, saved),
            None => self.spatial.entry(ui),
        };
        if let Some(target) = target {
            self.focus(ui, target, false);
            self.entered = restore.saved.is_none().then_some(target);
        } else {
            restore.frames += 1;
            if restore.frames < RESTORE_FRAMES {
                self.restore = Some(restore);
            }
        }
    }

    /// Whether a later frame still has focus to place.
    pub fn pending(&self) -> bool {
        self.restore.is_some() || self.dialog.is_some()
    }
}
