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
use caskade_ui_input::keys::{Modifiers as KeyModifiers, key_action};
use caskade_ui_input::spatial;
use caskade_ui_input::{Action, GlyphFamily, InputSource};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::dioxus_core::{ElementId, Runtime};
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
    #[allow(dead_code, reason = "the views (#96 onwards) send it")]
    Navigated(String),
    /// A click landed on `hit`: focus goes to the control there (Blitz
    /// clears it on a click anywhere but a field), and in console mode a tap
    /// on a field opens the on-screen keyboard.
    Clicked {
        hit: Option<NodeId>,
        touch: bool,
    },
}

/// Frames are checked for at least this long after an action or a click:
/// a dialog it opens has rendered by then.
const SETTLE_FRAMES: u32 = 6;
/// And at most this long while the dispatcher still has focus to place.
const MAX_FRAMES: u32 = 30;
const FRAME: Duration = Duration::from_millis(16);

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
}

/// What opening a game's details does.
type OpenDetails = Rc<dyn Fn(String)>;

struct Inner {
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
    frames: Cell<bool>,
    back: RefCell<Option<Rc<dyn Fn()>>>,
    details: RefCell<Option<OpenDetails>>,
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
                dispatcher: RefCell::new(Dispatcher::new(console)),
                root: RefCell::new(None),
                jobs,
                seen: Cell::new(None),
                synthetic: Cell::new(false),
                keys: Cell::new(0),
                pointers: Cell::new(0),
                frames: Cell::new(false),
                back: RefCell::new(None),
                details: RefCell::new(None),
            }),
            changed: Signal::new(0),
            keyboard: Signal::new(None),
            scrolled: Signal::new(0),
        }
    }

    /// Sends `action` as `source` sent it.
    pub fn send(&self, action: Action, source: InputSource, glyphs: Option<GlyphFamily>) {
        let _ = self.inner.jobs.send(Job::Action(action, source, glyphs));
    }

    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
    /// The view changed to history entry `key`: focus goes back where it
    /// was there, or to the view's entry.
    pub fn navigated(&self, key: impl Into<String>) {
        let _ = self.inner.jobs.send(Job::Navigated(key.into()));
    }

    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
    /// What `back` does with no overlay and no binding: the previous view.
    pub fn on_back(&self, back: impl Fn() + 'static) {
        *self.inner.back.borrow_mut() = Some(Rc::new(back));
    }

    /// What opening a game's details does: the router's way there.
    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
    pub fn on_details(&self, details: impl Fn(String) + 'static) {
        *self.inner.details.borrow_mut() = Some(Rc::new(details));
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
        self.inner.dispatcher.borrow_mut().console = console;
    }

    /// Whether spatial navigation and the keyboard map are on.
    pub fn active(&self) -> bool {
        self.changed.read();
        self.inner.dispatcher.borrow().active()
    }

    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
    /// The labels of the pad used last, for hints.
    pub fn glyphs(&self) -> GlyphFamily {
        self.changed.read();
        self.inner.dispatcher.borrow().glyphs()
    }

    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
    /// What sent the last action.
    pub fn source(&self) -> Option<InputSource> {
        self.changed.read();
        self.inner.dispatcher.borrow().source()
    }

    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
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

    #[allow(dead_code, reason = "the views (#96 onwards) call it")]
    /// Changes each time the input layer scrolls a box.
    pub fn scrolled(&self) -> u64 {
        *self.scrolled.read()
    }

    pub fn bindings(&self) -> Rc<RefCell<Bindings<NodeId>>> {
        self.inner.dispatcher.borrow().bindings()
    }

    /// The field the on-screen keyboard is open on.
    pub fn keyboard(&self) -> Option<NodeId> {
        *self.keyboard.read()
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

    /// Runs `work` with the document borrowed.
    pub(super) fn read<T>(&self, work: impl FnOnce(&BaseDocument) -> T) -> Option<T> {
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
        let read = self.with_ui(|ui, _| {
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
            super::e2e::Probe {
                focused: name.unwrap_or_default(),
                sections,
                text: collapse(ui.text(ui.root())),
                keyboard: 0,
                pointer: 0,
            }
        });
        let mut probe = read.unwrap_or_default();
        probe.keyboard = self.inner.keys.get();
        probe.pointer = self.inner.pointers.get();
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
        spawn(async move {
            for frame in 0..MAX_FRAMES {
                // Made on the app's runtime: a timer needs its reactor.
                let _ = caskade_app::runtime::spawn(async {
                    tokio::time::sleep(FRAME).await;
                })
                .await;
                input.run_frame();
                let pending = input.inner.dispatcher.borrow().pending();
                if frame + 1 >= SETTLE_FRAMES && !pending {
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

#[allow(dead_code, reason = "the views (#96 onwards) call it")]
/// Puts focus back where it was when the view at `key` comes back, or on
/// the view's entry when it's new: a view calls this with its location.
pub fn use_focus_location(key: String) {
    let input = use_input();
    let last: Rc<RefCell<Option<String>>> = use_hook(|| Rc::new(RefCell::new(None)));
    let changed = last.borrow().as_deref() != Some(key.as_str());
    if changed {
        *last.borrow_mut() = Some(key.clone());
        input.navigated(key);
    }
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
                worker.run(job);
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
    if input.inner.dispatcher.borrow().console != console {
        input.set_console(console);
    }
    let on_key = input.clone();
    let on_mount = input.clone();
    let on_click = input.clone();
    let typing = input.keyboard();
    rsx! {
        div {
            "data-cs": "input-root",
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
                if on_key.key(&event) {
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
