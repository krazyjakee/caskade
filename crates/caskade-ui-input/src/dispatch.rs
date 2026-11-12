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

/// An action on its way: the bindings to try, then the default.
pub struct Routed<N> {
    pub action: Action,
    /// The focused element the action is for, if any.
    pub focused: Option<N>,
    /// The bindings for it, the one to try first first.
    pub handlers: Vec<bindings::Run>,
}

/// Putting focus back when a view comes back, or on its entry when one
/// opens: tried each frame until the view has rendered, for half a second.
#[derive(Debug, Clone)]
struct Restore {
    saved: Option<FocusMark>,
    frames: u32,
    /// Only when nothing has focus (console mode turned on).
    idle: bool,
    /// Focus went on the view's first control before its default came
    /// (its data still loading): the default takes it when it comes, if
    /// focus hasn't moved.
    placed: bool,
    /// Frames it has stayed on the view's default.
    stable: u32,
}

/// Frames to wait for a view to render before giving up on its focus.
const RESTORE_FRAMES: u32 = 30;
/// How many frames a view's focus waits for the view's own controls (its
/// data may come well after it mounts) before taking anything in the
/// window.
const VIEW_FRAMES: u32 = 300;
/// Frames focus stays on a view's default before its placing is done.
const STABLE_FRAMES: u32 = 6;

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
    /// The view's focus went elsewhere than its default (not there yet, or
    /// not pressable yet): a later frame gives it to the default if focus
    /// hasn't moved. No frames are kept running for it.
    awaiting: bool,
    restore: Option<Restore>,
    /// A dialog that took focus itself, to move onto its default control.
    dialog: Option<N>,
    /// Open overlays, outermost first, with where focus was before each
    /// opened.
    overlays: Vec<(N, Option<N>)>,
    /// An open overlay focus hasn't been put in yet (it has no box yet).
    entering: bool,
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
            awaiting: false,
            restore: None,
            dialog: None,
            overlays: Vec::new(),
            entering: false,
        }
    }

    /// Spatial navigation and the keyboard map are on.
    pub fn active(&self) -> bool {
        self.console || self.pad_used
    }

    /// Console mode on or off. Turned on with navigation off, the view's
    /// focus is placed as a navigation would place it (once the window
    /// starts in console mode, its first view mounted before), unless
    /// something has focus already.
    pub fn set_console(&mut self, console: bool) {
        let starting = console && !self.active();
        self.console = console;
        if starting && self.restore.is_none() {
            self.restore = Some(Restore {
                saved: None,
                frames: 0,
                idle: true,
                placed: false,
                stable: 0,
            });
        }
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
        let routed = self.route(ui, action, sender, glyphs);
        if bindings::run(&routed.handlers) {
            return;
        }
        self.fallback(ui, &routed);
    }

    /// The first half of [`Self::dispatch`], for a UI whose binding
    /// handlers must run with nothing borrowed: the bindings to try, in
    /// order. If none takes the action, [`Self::fallback`] does the default.
    pub fn route<U: Ui<Node = N>>(
        &mut self,
        ui: &U,
        action: Action,
        sender: InputSource,
        glyphs: Option<GlyphFamily>,
    ) -> Routed<N> {
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
        Routed {
            action,
            focused,
            handlers,
        }
    }

    /// What `routed`'s action does when no binding took it.
    pub fn fallback<U: Ui<Node = N>>(&mut self, ui: &mut U, routed: &Routed<N>) {
        // A handler may have moved focus or removed the element.
        let focused = routed.focused.filter(|&node| contains(ui, ui.root(), node));
        self.run_default(ui, routed.action, focused);
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
        self.awaiting = false;
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
        self.restore = (saved.is_some() || self.active()).then_some(Restore {
            saved,
            frames: 0,
            idle: false,
            placed: false,
            stable: 0,
        });
    }

    /// After each render: the pending dialog start and view focus.
    pub fn frame<U: Ui<Node = N>>(&mut self, ui: &mut U) {
        self.follow_overlays(ui);
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
            self.default_when_ready(ui);
            return;
        };
        // A dialog opened meanwhile: it has focus now, not the view. (Focus
        // on something gone from the window is none.)
        let root = ui.root();
        let current = ui.focused().filter(|&node| contains(ui, root, node));
        if spatial::has_overlay(ui)
            || current.is_some_and(|node| closest(ui, node, |at| is_dialog(ui, at)).is_some())
        {
            return;
        }
        if restore.placed {
            if current.is_some() {
                // Moved since: the user's.
                if current != self.entered {
                    return;
                }
                restore.frames += 1;
                match self.spatial.view_default(ui) {
                    Some(preferred) if Some(preferred) != current => {
                        self.focus(ui, preferred, false);
                        self.entered = Some(preferred);
                        restore.stable = 0;
                    }
                    // On the view's default: done once it has stayed there
                    // (the old view can still be on screen for a frame).
                    Some(_) => restore.stable += 1,
                    None => {}
                }
                if restore.stable < STABLE_FRAMES && restore.frames <= VIEW_FRAMES {
                    self.restore = Some(restore);
                } else if restore.stable < STABLE_FRAMES {
                    // Out of frames before the default could take focus:
                    // it takes it on a later frame (one it asks for).
                    self.awaiting = true;
                }
                return;
            }
            // What it was placed on went (the view it was in unmounted):
            // look again.
            restore.placed = false;
        }
        if restore.idle && current.is_some() {
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
            None => match self.spatial.view_entry(ui) {
                Some(None) if restore.frames < VIEW_FRAMES => None,
                Some(Some(found)) => Some(found),
                _ => self.spatial.entry(ui),
            },
        };
        if let Some(target) = target {
            self.focus(ui, target, false);
            self.entered = restore.saved.is_none().then_some(target);
            // Until it has stayed on the view's default: the default may
            // still come, or the view it went into may still be the old one.
            if restore.saved.is_none() {
                restore.placed = true;
                restore.stable = 0;
                self.restore = Some(restore);
            }
        } else {
            restore.frames += 1;
            // A view's focus has one last frame to fall back on the window.
            let keep = if restore.saved.is_some() {
                restore.frames < RESTORE_FRAMES
            } else {
                restore.frames <= VIEW_FRAMES
            };
            if keep {
                self.restore = Some(restore);
            }
        }
    }

    /// The view's default takes focus from where the view put it, once it
    /// can, unless the user moved it since.
    fn default_when_ready<U: Ui<Node = N>>(&mut self, ui: &mut U) {
        if !self.awaiting {
            return;
        }
        let root = ui.root();
        let current = ui.focused().filter(|&node| contains(ui, root, node));
        if current.is_none() || current != self.entered {
            self.awaiting = false;
            return;
        }
        if let Some(preferred) = self.spatial.view_default(ui) {
            if Some(preferred) != current {
                self.focus(ui, preferred, false);
                self.entered = Some(preferred);
            }
            self.awaiting = false;
        }
    }

    /// Whether a view's focus is still waiting for the view's controls:
    /// frames go on longer for it.
    pub fn waiting_for_view(&self) -> bool {
        self.restore
            .as_ref()
            .is_some_and(|restore| restore.saved.is_none())
    }

    /// Whether a later frame still has focus to place.
    pub fn pending(&self) -> bool {
        self.restore.is_some() || self.dialog.is_some() || self.entering
    }

    /// Focus goes into an overlay that opens without taking it, and back to
    /// where it was when the overlay closes, as a modal's focus scope does.
    fn follow_overlays<U: Ui<Node = N>>(&mut self, ui: &mut U) {
        let root = ui.root();
        let within = scope(ui);
        // Closed: gone from the tree, or no longer an open overlay.
        let mut closed = false;
        while let Some(&(overlay, back)) = self.overlays.last() {
            let open =
                contains(ui, root, overlay) && (overlay == within || contains(ui, overlay, within));
            if open {
                break;
            }
            self.overlays.pop();
            closed = true;
            let lost = ui
                .focused()
                .is_none_or(|node| !contains(ui, root, node) || contains(ui, overlay, node));
            if lost && let Some(back) = back.filter(|&back| is_focusable(ui, back)) {
                self.focus(ui, back, true);
            }
        }
        // Everything closed and what had focus before went too (the quick
        // menu over console mode's curtain, which lifted under it): the
        // view's focus, as after a navigation.
        if closed
            && within == root
            && self.active()
            && self.restore.is_none()
            && ui.focused().is_none_or(|node| !contains(ui, root, node))
        {
            self.restore = Some(Restore {
                saved: None,
                frames: 0,
                idle: true,
                placed: false,
                stable: 0,
            });
        }
        self.entering = false;
        if within == root {
            return;
        }
        let current = ui.focused();
        let inside =
            current.is_some_and(|node| contains(ui, within, node) || controls(ui, node, within));
        if self.overlays.last().map(|&(overlay, _)| overlay) != Some(within) {
            self.overlays.push((within, current.filter(|_| !inside)));
        }
        if inside {
            return;
        }
        match self.spatial.start_in(ui, within) {
            Some(start) => self.focus(ui, start, false),
            // Not laid out yet: the next frame tries again.
            None => self.entering = true,
        }
    }
}
