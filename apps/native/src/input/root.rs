//! The input root: one element around the views that takes the keyboard
//! map's keys, puts focus where a click asks, and runs the dispatcher
//! (`caskade_ui_input::dispatch`) for keys and pads, one action at a time,
//! on Dioxus's executor.
//!
//! Each action goes through the document in two steps: the bindings for it
//! are found with the document borrowed, then run with nothing borrowed
//! (they're app code); if none takes it, the default runs with the document
//! borrowed again. What the default asks of app code (a click's listeners, a
//! focus change's) runs last, as an [`Effect`].

use super::doc::{BlitzUi, Effect};
use super::guide::{Guide, GuideMenu, NativeGuide};
use super::keyboard::OnScreenKeyboard;
use blitz_dom::{BaseDocument, NodeId};
use caskade_ui_input::bindings::{ActiveBinding, Binding, BindingId, Bindings};
use caskade_ui_input::dispatch::Dispatcher;
use caskade_ui_input::keys::{Modifiers as KeyModifiers, key_action, starts_typing};
use caskade_ui_input::spatial;
use caskade_ui_input::{Action, GlyphFamily, InputSource};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::dioxus_core::{ElementId, Runtime, ScopeId};
use dioxus_native::prelude::dioxus_html::PlatformEventData;
use dioxus_native::prelude::*;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Something for the dispatcher to do, in order.
#[derive(Debug, Clone)]
pub enum Job {
    Action(Action, InputSource, Option<GlyphFamily>),
    /// After a render: an overlay to enter, a view's focus to place.
    Frame,
    /// The view changed to a history entry.
    Navigated(String),
    /// A click landed on `hit`: focus goes to the control there (Blitz
    /// clears it on a click anywhere but a field), and in console mode a tap
    /// on a field opens the on-screen keyboard.
    Clicked {
        hit: Option<NodeId>,
        touch: bool,
    },
    /// Focus goes to `node` (shown as keyboard focus): a view put it there.
    Focus(NodeId),
}

/// Frames are checked for at least this long after an action or a click:
/// a dialog it opens has rendered by then.
const SETTLE_FRAMES: u32 = 6;
/// And at most this long while the dispatcher still has focus to place.
const MAX_FRAMES: u32 = 30;
const FRAME: Duration = Duration::from_millis(16);
/// The longest a pad action waits for the window's layout to catch up.
const STALE_WAIT: Duration = Duration::from_millis(250);

/// The window's input state, for views: the context [`InputRoot`] provides.
#[derive(Clone)]
pub struct Input {
    inner: Rc<Inner>,
    /// Bumped after each action, so hints read again.
    changed: Signal<u64>,
    /// The field the on-screen keyboard types into.
    keyboard: Signal<Option<NodeId>>,
    /// Bumped when the input layer scrolls a box: Blitz sends no scroll
    /// event for a scroll made from code, so views that follow scrolling
    /// (a long list's window) read this too.
    scrolled: Signal<u64>,
    /// Bumped once the window's new size or scale is laid out (#146).
    resized: Signal<u64>,
}

/// What opening a game's details does.
type OpenDetails = Rc<dyn Fn(String)>;
/// What typing outside a field does: the search, with what was typed.
type StartSearch = Rc<dyn Fn(String)>;

struct Inner {
    /// The input root owns its checks, even when a child starts them.
    scope: ScopeId,
    dispatcher: RefCell<Dispatcher<NodeId>>,
    /// The root's mounted element, for the document.
    root: RefCell<Option<NodeHandle>>,
    jobs: mpsc::UnboundedSender<Job>,
    /// The focus the dispatcher last heard of.
    seen: Cell<Option<NodeId>>,
    /// The input layer is running listeners itself: the root's own click
    /// listener isn't hearing a click from the user.
    synthetic: Cell<bool>,
    /// Real key and pointer events the root heard, for the e2e report.
    keys: Cell<u64>,
    pointers: Cell<u64>,
    /// Pad actions heard, and how many of them were a held direction's
    /// repeats, for the e2e report.
    pad_actions: Cell<u64>,
    repeats: Cell<u64>,
    /// Actions handled (after any wait for the layout).
    handled: Cell<u64>,
    frames: Cell<bool>,
    back: RefCell<Option<Rc<dyn Fn()>>>,
    details: RefCell<Option<OpenDetails>>,
    typing: RefCell<Option<StartSearch>>,
    /// What Ctrl K (⌘ K) does: the search's focus.
    search_key: RefCell<Option<Rc<dyn Fn()>>>,
    /// What Ctrl V (⌘ V) outside a text field does (#132).
    paste_key: RefCell<Option<Rc<dyn Fn()>>>,
}

impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// The value of an event no listener reads (a focus change's).
fn no_data() -> Rc<dyn Any> {
    Rc::new(PlatformEventData::new(Box::new(())))
}

impl Input {
    fn new(console: bool, jobs: mpsc::UnboundedSender<Job>) -> Self {
        Self {
            inner: Rc::new(Inner {
                scope: Runtime::current().current_scope_id(),
                dispatcher: RefCell::new(Dispatcher::new(console)),
                root: RefCell::new(None),
                jobs,
                seen: Cell::new(None),
                synthetic: Cell::new(false),
                keys: Cell::new(0),
                pointers: Cell::new(0),
                pad_actions: Cell::new(0),
                repeats: Cell::new(0),
                handled: Cell::new(0),
                frames: Cell::new(false),
                back: RefCell::new(None),
                details: RefCell::new(None),
                typing: RefCell::new(None),
                search_key: RefCell::new(None),
                paste_key: RefCell::new(None),
            }),
            changed: Signal::new(0),
            keyboard: Signal::new(None),
            scrolled: Signal::new(0),
            resized: Signal::new(0),
        }
    }

    /// Sends `action` as `source` sent it.
    pub fn send(&self, action: Action, source: InputSource, glyphs: Option<GlyphFamily>) {
        let _ = self.inner.jobs.send(Job::Action(action, source, glyphs));
    }

    /// The view changed to history entry `key`: focus goes back where it
    /// was there, or to the view's entry.
    pub fn navigated(&self, key: impl Into<String>) {
        let _ = self.inner.jobs.send(Job::Navigated(key.into()));
    }

    /// What `back` does with no overlay and no binding: the previous view.
    pub fn on_back(&self, back: impl Fn() + 'static) {
        *self.inner.back.borrow_mut() = Some(Rc::new(back));
    }

    /// What opening a game's details does: the router's way there.
    pub fn on_details(&self, details: impl Fn(String) + 'static) {
        *self.inner.details.borrow_mut() = Some(Rc::new(details));
    }

    /// What typing outside a field, a menu or a dialog does: the shell's
    /// search opens with the character typed (empty for `/`).
    pub fn on_type(&self, start: impl Fn(String) + 'static) {
        *self.inner.typing.borrow_mut() = Some(Rc::new(start));
    }

    /// What Ctrl K (⌘ K on macOS) does from anywhere: the top bar's
    /// search takes focus.
    pub fn on_search_key(&self, focus: impl Fn() + 'static) {
        *self.inner.search_key.borrow_mut() = Some(Rc::new(focus));
    }

    /// Ctrl K or ⌘ K: whether it went to the search.
    fn search_key(&self, event: &KeyboardEvent) -> bool {
        let held = event.modifiers();
        let shortcut = (held.ctrl() || held.meta())
            && !held.alt()
            && event.key().to_string().eq_ignore_ascii_case("k");
        let focus = self.inner.search_key.borrow().clone();
        match focus {
            Some(focus) if shortcut => {
                focus();
                true
            }
            _ => false,
        }
    }

    /// What Ctrl V (⌘ V) does outside a text field (#132): a paste there
    /// stays the field's.
    pub fn on_paste_key(&self, paste: impl Fn() + 'static) {
        *self.inner.paste_key.borrow_mut() = Some(Rc::new(paste));
    }

    /// Ctrl V or ⌘ V outside a text field: whether it was taken.
    fn paste_key(&self, event: &KeyboardEvent) -> bool {
        let held = event.modifiers();
        let shortcut = (held.ctrl() || held.meta())
            && !held.alt()
            && !held.shift()
            && event.key().to_string().eq_ignore_ascii_case("v");
        if !shortcut || event.is_composing() {
            return false;
        }
        let Some(paste) = self.inner.paste_key.borrow().clone() else {
            return false;
        };
        let in_field = self.with_ui(|ui, _| {
            caskade_ui_input::dispatch::Ui::focused(ui).is_some_and(|node| editable(ui, node))
        });
        if in_field != Some(false) {
            return false;
        }
        paste();
        true
    }

    /// A key the keyboard map left alone: whether it started a search.
    fn typing(&self, event: &KeyboardEvent) -> bool {
        let start = self.inner.typing.borrow().clone();
        let Some(start) = start else {
            return false;
        };
        if event.is_composing() {
            return false;
        }
        let key = event.key().to_string();
        let held = event.modifiers();
        let modifiers = KeyModifiers {
            ctrl: held.ctrl(),
            meta: held.meta(),
            alt: held.alt(),
            shift: held.shift(),
        };
        let initial = self.with_ui(|ui, _| {
            let target = caskade_ui_input::dispatch::Ui::focused(ui);
            starts_typing(ui, target, &key, modifiers)
        });
        let Some(Some(initial)) = initial else {
            return false;
        };
        start(initial);
        true
    }

    /// Opens a game's details page.
    pub fn open_details(&self, game_id: String) {
        let details = self.inner.details.borrow().clone();
        match details {
            Some(details) => details(game_id),
            None => tracing::debug!(game_id, "no details view yet"),
        }
    }

    /// Something drawn from async code (a menu, a dialog) may need focus:
    /// the next frames look.
    pub fn settle(&self) {
        self.frames();
    }

    /// Console mode: navigation is always on.
    pub fn set_console(&self, console: bool) {
        self.inner.dispatcher.borrow_mut().set_console(console);
        // Its focus goes in over the next frames.
        self.frames();
    }

    /// Whether spatial navigation and the keyboard map are on.
    pub fn active(&self) -> bool {
        self.changed.read();
        self.inner.dispatcher.borrow().active()
    }

    /// The labels of the pad used last, for hints.
    pub fn glyphs(&self) -> GlyphFamily {
        self.changed.read();
        self.inner.dispatcher.borrow().glyphs()
    }

    /// What sent the last action.
    pub fn source(&self) -> Option<InputSource> {
        self.changed.read();
        self.inner.dispatcher.borrow().source()
    }

    /// What each action would do now, for the hint bar.
    pub fn hints(&self) -> Vec<ActiveBinding> {
        self.changed.read();
        self.with_ui(|ui, dispatcher| {
            let within = spatial::scope(ui);
            let overlay = (within != caskade_ui_input::FocusTree::root(ui)).then_some(within);
            let focused = caskade_ui_input::dispatch::Ui::focused(ui);
            dispatcher.bindings().borrow().active(ui, focused, overlay)
        })
        .unwrap_or_default()
    }

    /// A view scrolled a box itself: what follows scrolling hears of it, as
    /// Blitz sends no scroll event for a scroll made from code.
    pub fn scrolled_by_code(&self) {
        let mut scrolled = self.scrolled;
        scrolled += 1;
    }

    /// Puts focus on `node`, as keys or a pad would: remembered in its
    /// sections, with the ring, scrolled into view.
    pub fn focus(&self, node: NodeId) {
        let _ = self.inner.jobs.send(Job::Focus(node));
    }

    /// Changes each time the input layer (or a view) scrolls a box.
    pub fn scrolled(&self) -> u64 {
        *self.scrolled.read()
    }

    /// Changes each time the window's new size or scale has been laid out
    /// (#146): views that size themselves from their boxes (the library's
    /// columns, a shelf's cards) measure again then. Not at the resize
    /// itself: until the renderer lays the window out, boxes are the old
    /// size.
    pub fn resized(&self) -> u64 {
        *self.resized.read()
    }

    pub fn bindings(&self) -> Rc<RefCell<Bindings<NodeId>>> {
        self.inner.dispatcher.borrow().bindings()
    }

    /// The field the on-screen keyboard is open on.
    pub fn keyboard(&self) -> Option<NodeId> {
        *self.keyboard.read()
    }

    /// Opens the on-screen keyboard on `field`, which has focus: a pad
    /// types there (the search, from the pad's Search).
    pub fn open_keyboard(&self, field: NodeId) {
        let mut keyboard = self.keyboard;
        keyboard.set(Some(field));
    }

    pub fn close_keyboard(&self) {
        let mut keyboard = self.keyboard;
        keyboard.set(None);
    }

    /// Runs `work` over the document and the dispatcher, if the root is up.
    fn with_ui<T>(
        &self,
        work: impl FnOnce(&mut BlitzUi<'_>, &mut Dispatcher<NodeId>) -> T,
    ) -> Option<T> {
        let root = self.inner.root.borrow().clone()?;
        let mut doc = root.doc_mut();
        let mut ui = BlitzUi::new(&mut doc, Some(root.node_id()));
        let mut dispatcher = self.inner.dispatcher.borrow_mut();
        Some(work(&mut ui, &mut dispatcher))
    }

    /// Runs `work` over the document, then the effects it left.
    fn act(&self, ring: bool, work: impl FnOnce(&mut BlitzUi<'_>, &mut Dispatcher<NodeId>)) {
        let effects = self.with_ui(|ui, dispatcher| {
            ui.ring = ring;
            work(ui, dispatcher);
            std::mem::take(&mut ui.effects)
        });
        self.apply(effects.unwrap_or_default());
    }

    fn handle(&self) -> Option<NodeHandle> {
        self.inner.root.borrow().clone()
    }

    /// Whether the document changed since it was last styled and laid out:
    /// boxes read now are the old ones.
    pub(crate) fn stale(&self) -> bool {
        self.read(|doc| {
            let root = doc.root_element();
            root.has_dirty_descendants() || root.has_damaged_descendants()
        })
        .unwrap_or(false)
    }

    /// Whether the document is laid out at the window's size: the root
    /// element is the window's CSS size (`html` is `height: 100%`). No
    /// document yet reads as laid out.
    fn laid_out_to_window(&self) -> bool {
        self.read(|doc| {
            let viewport = doc.viewport();
            let scale = viewport.scale_f64();
            let (width, height) = viewport.window_size;
            let size = doc.root_element().final_layout().size;
            (f64::from(size.width) - f64::from(width) / scale).abs() < 1.0
                && (f64::from(size.height) - f64::from(height) / scale).abs() < 1.0
        })
        .unwrap_or(true)
    }

    /// Runs `work` with the document borrowed.
    pub(crate) fn read<T>(&self, work: impl FnOnce(&BaseDocument) -> T) -> Option<T> {
        let root = self.handle()?;
        let doc = root.doc();
        Some(work(&doc))
    }

    /// Runs `work` with the document borrowed mutably.
    pub(super) fn write<T>(&self, work: impl FnOnce(&mut BaseDocument) -> T) -> Option<T> {
        let root = self.handle()?;
        let mut doc = root.doc_mut();
        Some(work(&mut doc))
    }

    /// Runs the listeners for `name` on `node` (and, if `bubbles`, its
    /// ancestors') with `data`, as the page's own event would.
    pub(super) fn fire(&self, node: NodeId, name: &str, data: Rc<dyn Any>, bubbles: bool) {
        let Some(id) = self.read(|doc| dioxus_id(doc, node)).flatten() else {
            return;
        };
        let Some(runtime) = Runtime::try_current() else {
            return;
        };
        let outer = self.inner.synthetic.replace(true);
        runtime.handle_event(name, Event::new(data, bubbles), id);
        self.inner.synthetic.set(outer);
    }

    /// The window as the e2e driver reads it.
    fn probe(&self) -> super::e2e::Probe {
        let read = self.with_ui(|ui, dispatcher| {
            use caskade_ui_input::FocusTree;
            let collapse = |text: String| text.split_whitespace().collect::<Vec<_>>().join(" ");
            let focused = caskade_ui_input::dispatch::Ui::focused(ui);
            let name = focused.map(|node| {
                let labelled = ui.attr(node, "aria-labelledby").map(|ids| {
                    ids.split_whitespace()
                        .filter_map(|id| {
                            caskade_ui_input::tree::descendants(ui, ui.root())
                                .into_iter()
                                .find(|&at| ui.attr(at, "id") == Some(id))
                        })
                        .map(|at| ui.text(at))
                        .collect::<Vec<_>>()
                        .join(" ")
                });
                let label = caskade_ui_input::tree::closest(ui, node, |at| ui.tag(at) == "label")
                    .map(|at| ui.text(at));
                collapse(
                    ui.attr(node, "aria-label")
                        .map(str::to_owned)
                        .or(labelled.filter(|text| !text.trim().is_empty()))
                        .or(label)
                        .unwrap_or_else(|| ui.text(node)),
                )
            });
            let mut sections = Vec::new();
            let mut at = focused.and_then(|node| spatial::section(ui, node));
            while let Some(section) = at {
                sections.extend(ui.attr(section, "data-focus-section").map(str::to_owned));
                at = ui
                    .parent(section)
                    .and_then(|parent| spatial::section(ui, parent));
            }
            let stale = {
                let root = ui.doc().root_element();
                root.has_dirty_descendants() || root.has_damaged_descendants()
            };
            let within = spatial::scope(ui);
            super::e2e::Probe {
                focused: name.unwrap_or_default(),
                sections,
                text: collapse(ui.text(ui.root())),
                keyboard: 0,
                pointer: 0,
                actions: 0,
                repeats: 0,
                handled: 0,
                grid: None,
                focus_debug: format!(
                    "pending={}, waiting={}, frames={}, stale={}, entered={:?}, default={:?}, scope={:?}, entry={:?}, focused_box={:?}, left={:?}",
                    dispatcher.pending(),
                    dispatcher.waiting_for_view(),
                    self.inner.frames.get(),
                    stale,
                    dispatcher.entered().map(|node| ui.text(node)),
                    dispatcher
                        .spatial
                        .view_default(ui)
                        .map(|node| ui.text(node)),
                    (ui.attr(within, "data-cs"), ui.attr(within, "aria-label"), ui.rect(within)),
                    dispatcher.spatial.entry(ui).map(|node| ui.text(node)),
                    focused.map(|node| ui.rect(node)),
                    focused.and_then(|node| dispatcher.spatial.next(ui, node, spatial::Direction::Left))
                        .map(|node| (ui.text(node), ui.rect(node))),
                ),
            }
        });
        let mut probe = read.unwrap_or_default();
        probe.grid = self
            .read(|doc| crate::library::probe_grid(doc, doc.root_element().id))
            .flatten();
        probe.keyboard = self.inner.keys.get();
        probe.pointer = self.inner.pointers.get();
        probe.actions = self.inner.pad_actions.get();
        probe.repeats = self.inner.repeats.get();
        probe.handled = self.inner.handled.get();
        probe
    }

    fn apply(&self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Focus { from, to } => {
                    if let Some(from) = from {
                        self.fire(from, "blur", no_data(), false);
                        self.fire(from, "focusout", no_data(), true);
                    }
                    self.fire(to, "focus", no_data(), false);
                    self.fire(to, "focusin", no_data(), true);
                }
                Effect::Blur(from) => {
                    self.fire(from, "blur", no_data(), false);
                    self.fire(from, "focusout", no_data(), true);
                }
                Effect::Click(node) => {
                    let data = self.read(|doc| {
                        doc.get_node(node).map(|node| {
                            dioxus_native::synthetic_click_event(node, Default::default())
                        })
                    });
                    if let Some(Some(data)) = data {
                        let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(data));
                        self.fire(node, "click", data, true);
                    }
                }
                Effect::Keyboard(field) => {
                    let mut keyboard = self.keyboard;
                    keyboard.set(Some(field));
                }
                Effect::Back => {
                    let back = self.inner.back.borrow().clone();
                    match back {
                        Some(back) => back(),
                        None => tracing::debug!("back: no previous view"),
                    }
                }
                Effect::Scrolled(_) => {
                    let mut scrolled = self.scrolled;
                    scrolled += 1;
                }
            }
        }
    }

    /// Handles one job.
    pub fn run(&self, job: Job) {
        match job {
            Job::Action(action, source, glyphs) => {
                let ring = source != InputSource::Touch;
                let routed = self.with_ui(|ui, dispatcher| {
                    ui.ring = ring;
                    dispatcher.route(ui, action, source, glyphs)
                });
                let Some(routed) = routed else {
                    return;
                };
                // The bindings run with nothing borrowed: they're app code.
                if !caskade_ui_input::bindings::run(&routed.handlers) {
                    self.act(ring, |ui, dispatcher| dispatcher.fallback(ui, &routed));
                }
                self.frames();
            }
            Job::Frame => self.act(true, |ui, dispatcher| dispatcher.frame(ui)),
            Job::Focus(node) => {
                self.act(true, |ui, dispatcher| dispatcher.focus(ui, node, false));
            }
            Job::Navigated(key) => {
                self.act(true, |ui, dispatcher| dispatcher.navigated(ui, &key));
                self.frames();
            }
            Job::Clicked { hit, touch } => {
                self.act(false, |ui, dispatcher| {
                    let target = hit.and_then(|hit| {
                        caskade_ui_input::tree::closest(ui, hit, |at| spatial::is_focusable(ui, at))
                    });
                    match target {
                        Some(target) => ui.take_focus(target),
                        None => {
                            if caskade_ui_input::dispatch::Ui::focused(ui).is_none() {
                                caskade_ui_input::dispatch::Ui::blur(ui);
                            }
                        }
                    }
                    if touch && let Some(hit) = hit {
                        dispatcher.tap(ui, hit);
                    }
                });
                self.frames();
            }
        }
        self.notice();
    }

    /// Tells the dispatcher where focus is now, keeps it on the root when
    /// nothing has it, and closes the on-screen keyboard once its field has
    /// lost focus.
    fn notice(&self) {
        let now = self.with_ui(|ui, dispatcher| {
            let focused = caskade_ui_input::dispatch::Ui::focused(ui);
            if focused != self.inner.seen.get() {
                self.inner.seen.set(focused);
                if let Some(node) = focused {
                    dispatcher.focused_in(ui, node);
                }
            }
            if let Some(root) = self.inner.root.borrow().as_ref()
                && super::doc::idle(ui.doc(), root.node_id())
                && ui.doc().get_focussed_node_id() != Some(root.node_id())
            {
                ui.doc().set_focus_to(root.node_id());
            }
            focused
        });
        let open = *self.keyboard.peek();
        if let (Some(field), Some(now)) = (open, now)
            && now != Some(field)
        {
            self.close_keyboard();
        }
        let mut changed = self.changed;
        changed += 1;
    }

    /// Checks frames for a while: a dialog an action opened has rendered
    /// after a frame or two, a view's data maybe later.
    fn frames(&self) {
        if self.inner.frames.replace(true) {
            return;
        }
        let input = self.clone();
        // Buttons, views and menus call settle too. Their scopes can go
        // away while these checks are pending; cancellation there would
        // leave `frames` set and prevent every later request from running.
        Runtime::current().spawn(self.inner.scope, async move {
            let mut frame = 0;
            loop {
                crate::data::sleep(FRAME).await;
                input.run_frame();
                frame += 1;
                let (pending, waiting) = {
                    let dispatcher = input.inner.dispatcher.borrow();
                    (dispatcher.pending(), dispatcher.waiting_for_view())
                };
                // A view's focus waits longer for the view's controls (its
                // own bound, `caskade_ui_input`'s VIEW_FRAMES).
                if (frame >= SETTLE_FRAMES && !pending) || (frame >= MAX_FRAMES && !waiting) {
                    break;
                }
            }
            input.inner.frames.set(false);
        });
    }

    fn run_frame(&self) {
        self.act(true, |ui, dispatcher| dispatcher.frame(ui));
        self.notice();
    }

    /// A key on the root: its action, while navigation is on. Whether the
    /// key was taken.
    fn key(&self, event: &KeyboardEvent) -> bool {
        // An IME's keys are the IME's (#84).
        if event.is_composing() {
            return false;
        }
        if !self.inner.dispatcher.borrow().active() {
            return false;
        }
        let key = event.key().to_string();
        let held = event.modifiers();
        let modifiers = KeyModifiers {
            ctrl: held.ctrl(),
            meta: held.meta(),
            alt: held.alt(),
            shift: held.shift(),
        };
        let action = self.with_ui(|ui, _| {
            let target = caskade_ui_input::dispatch::Ui::focused(ui);
            key_action(ui, target, &key, modifiers)
        });
        let Some(Some(action)) = action else {
            return false;
        };
        self.send(action, InputSource::Keyboard, None);
        true
    }
}

/// The Dioxus element id of `node` or its nearest ancestor that has one:
/// where its listeners are.
fn dioxus_id(doc: &BaseDocument, node: NodeId) -> Option<ElementId> {
    let mut at = Some(node);
    while let Some(here) = at {
        let node = doc.get_node(here)?;
        if let Some(id) = node
            .attr(blitz_dom::LocalName::from("data-dioxus-id"))
            .and_then(|id| id.parse::<usize>().ok())
        {
            return Some(ElementId::from_raw(id));
        }
        at = node.parent;
    }
    None
}

/// The input state, inside [`InputRoot`].
pub fn use_input() -> Input {
    use_context::<Input>()
}

/// How a binding applies.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bind {
    /// Shown in the hint bar; unlabelled bindings aren't hinted.
    pub label: Option<String>,
    /// The element it belongs to (a section, a field): it applies while
    /// focus is inside. `None` for the whole view.
    pub owner: Option<NodeId>,
    /// Also inside an open overlay, after the overlay's own.
    pub global: bool,
}

/// Handles `action` while the component is mounted: `run` returns false to
/// let the next binding (or the default) have it. The innermost binding
/// around focus wins; among equals, the newest.
pub fn use_input_action(action: Action, bind: Bind, run: impl FnMut() -> bool + 'static) {
    let input = use_input();
    let slot: Rc<RefCell<Box<dyn FnMut() -> bool>>> =
        use_hook(|| Rc::new(RefCell::new(Box::new(|| false) as Box<dyn FnMut() -> bool>)));
    // The newest handler, with what this render captured.
    match slot.try_borrow_mut() {
        Ok(mut current) => *current = Box::new(run),
        Err(_) => tracing::warn!(?action, "a binding rendered while it ran"),
    }
    let bindings = input.bindings();
    let id: BindingId = use_hook(|| {
        let slot = slot.clone();
        bindings.borrow_mut().add(Binding::new(action, move || {
            slot.try_borrow_mut().is_ok_and(|mut run| run())
        }))
    });
    {
        let mut bindings = bindings.borrow_mut();
        bindings.set_owner(id, bind.owner);
        bindings.set_label(id, bind.label);
        bindings.set_global(id, bind.global);
    }
    use_drop(move || bindings.borrow_mut().remove(id));
}

/// Puts focus back where it was when the view at `key` comes back, or on
/// the view's entry when it's new: a view calls this with its location.
pub fn use_focus_location(key: String) {
    let input = use_input();
    // Notify after the view has rendered. During render the previous
    // view can still be mounted, so its default could settle before the
    // new view is drawn on a slow renderer.
    use_effect(use_reactive!(|key| {
        input.navigated(key);
    }));
}

/// The element around the views that the input layer works through; pad
/// actions arrive from the app's events (`shell::events`). `console` turns
/// navigation on from the start, as console mode does.
#[component]
pub fn InputRoot(
    #[props(default)] console: bool,
    /// The guide menu's actions: the running app's by default.
    #[props(default = Guide(Rc::new(NativeGuide)))]
    guide: Guide,
    children: Element,
) -> Element {
    let input = use_hook(|| {
        let (jobs, mut queue) = mpsc::unbounded_channel();
        let input = Input::new(console, jobs);
        provide_context(input.clone());
        // One action at a time, after the event that sent it.
        let worker = input.clone();
        spawn(async move {
            while let Some(job) = queue.recv().await {
                // A move is worked out from the layout. Right after the view
                // changed, before the renderer laid it out again, that's the
                // old one (on a slow device the sidebar's Library was skipped
                // while its counts came in): a pad action waits for the next
                // layout, briefly.
                let action = matches!(job, Job::Action(..));
                if action || matches!(job, Job::Navigated(_)) {
                    let mut waited = Duration::ZERO;
                    while waited < STALE_WAIT && worker.stale() {
                        crate::data::sleep(FRAME).await;
                        waited += FRAME;
                    }
                }
                worker.run(job);
                if action {
                    let handled = &worker.inner.handled;
                    handled.set(handled.get() + 1);
                }
            }
        });
        // Pads, through the app's events.
        let pads = input.clone();
        let mut events = crate::shell::events().subscribe();
        spawn(async move {
            use caskade_app::events::Event as AppEvent;
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match events.recv().await {
                    Ok(AppEvent::InputActionEvent(event)) => {
                        let action = event.0;
                        let heard = &pads.inner;
                        heard.pad_actions.set(heard.pad_actions.get() + 1);
                        if action.repeat {
                            heard.repeats.set(heard.repeats.get() + 1);
                        }
                        pads.send(action.action, InputSource::Pad, Some(action.glyphs));
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
        // A scripted run's questions about the window.
        if let Some(mut asks) = super::e2e::take_asks() {
            let answers = input.clone();
            spawn(async move {
                while let Some(reply) = asks.recv().await {
                    let _ = reply.send(answers.probe());
                }
            });
        }
        input
    });
    use_resized(&input);
    if input.inner.dispatcher.borrow().console != console {
        input.set_console(console);
    }
    let on_key = input.clone();
    let on_mount = input.clone();
    let on_click = input.clone();
    let typing = input.keyboard();
    rsx! {
        div {
            "data-input-root": "true",
            tabindex: "-1",
            "data-input-active": if input.active() { "true" } else { "false" },
            // A box of its own: Blitz gives focus on a box-less element
            // (`display: contents`) to an ancestor instead.
            style: "display: block;",
            onmounted: move |event| {
                let Some(handle) = event.data().downcast::<NodeHandle>().cloned() else {
                    return;
                };
                // Keys go to the focused element: something must be, for
                // them to reach the keyboard map.
                {
                    let mut doc = handle.doc_mut();
                    if super::doc::idle(&doc, handle.node_id()) {
                        doc.set_focus_to(handle.node_id());
                    }
                }
                *on_mount.inner.root.borrow_mut() = Some(handle);
                let _ = on_mount.inner.jobs.send(Job::Frame);
            },
            onkeydown: move |event: KeyboardEvent| {
                on_key.inner.keys.set(on_key.inner.keys.get() + 1);
                if on_key.search_key(&event)
                    || on_key.paste_key(&event)
                    || on_key.key(&event)
                    || on_key.typing(&event)
                {
                    event.prevent_default();
                    event.stop_propagation();
                }
            },
            onclick: move |_: MouseEvent| {
                // A click the input layer sent (a pad's A) is no pointer's.
                if on_click.inner.synthetic.get() {
                    return;
                }
                on_click.inner.pointers.set(on_click.inner.pointers.get() + 1);
                let touch = false;
                let hit = on_click.read(|doc| doc.get_hover_node_id()).flatten();
                let _ = on_click.inner.jobs.send(Job::Clicked { hit, touch });
            },
            onpointerup: {
                let input = input.clone();
                move |event: PointerEvent| {
                    if event.pointer_type() == "touch" {
                        let hit = input.read(|doc| doc.get_hover_node_id()).flatten();
                        let _ = input.inner.jobs.send(Job::Clicked { hit, touch: true });
                    }
                }
            },
            {children}
            GuideMenu { guide }
            if let Some(field) = typing {
                OnScreenKeyboard { key: "{field:?}", field }
            }
        }
    }
}

/// Bumps [`Input::resized`] after the window resizes or changes scale, once
/// the new size is laid out: a frame or a few later, and at most once per
/// wait however many resize events a drag sends meanwhile. Nothing runs
/// between resizes.
fn use_resized(input: &Input) {
    let waiting = use_hook(|| Rc::new(Cell::new(false)));
    // Without a window (headless tests) there are no window events; that
    // stays so for the document's life, so the hook is always or never
    // called.
    if try_consume_context::<std::sync::Arc<dyn dioxus_native::winit::window::Window>>().is_none() {
        return;
    }
    let input = input.clone();
    dioxus_native::use_window_event(move |event, _| {
        use dioxus_native::winit::event::WindowEvent;
        if !matches!(
            event,
            WindowEvent::SurfaceResized(_) | WindowEvent::ScaleFactorChanged { .. }
        ) || waiting.replace(true)
        {
            return;
        }
        let input = input.clone();
        let waiting = waiting.clone();
        spawn(async move {
            for _ in 0..MAX_FRAMES {
                crate::data::sleep(FRAME).await;
                if input.laid_out_to_window() {
                    break;
                }
            }
            waiting.set(false);
            let mut resized = input.resized;
            resized += 1;
        });
    });
}

/// Text the user edits, which a paste belongs to: a field, a select, an
/// editable element, a text box or combo box (the webview's
/// `editableTarget`).
fn editable(ui: &BlitzUi<'_>, node: NodeId) -> bool {
    use caskade_ui_input::FocusTree;
    caskade_ui_input::keys::is_text_entry(ui, node)
        || caskade_ui_input::tree::closest(ui, node, |at| {
            ui.tag(at) == "select" || matches!(ui.attr(at, "role"), Some("textbox" | "combobox"))
        })
        .is_some()
}
