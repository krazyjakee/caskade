//! Driving a headless window by pad or by keys alone (#153): a pad's
//! moves walked until they reach a control, Tab pressed until it does, and
//! the frame over a test's own library and services, so a view opens from
//! Home and Back has somewhere to go.
//!
//! A walk only ever moves focus with a pad's directions. To try another
//! direction from a control the pad already reached it puts focus back on
//! that control first, as the pad left it; nothing is reached that the pad
//! didn't reach itself.

use super::tests::Window;
use crate::data::{DataSource, use_data_source};
use crate::router::Route;
use blitz_dom::{LocalName, NodeId};
use caskade_ui_input::Action;
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// A headless window a pad drives.
pub(crate) trait Driven {
    fn document(&self) -> &DioxusDocument;
    /// A pad's action, as the app's events bring it, then settled.
    fn press(&mut self, action: Action);
    /// Puts focus back on `node`, settled.
    fn put_focus(&mut self, node: NodeId);
}

impl Driven for Window {
    fn document(&self) -> &DioxusDocument {
        &self.doc
    }

    fn press(&mut self, action: Action) {
        self.pad(action);
    }

    fn put_focus(&mut self, node: NodeId) {
        self.focus(node);
    }
}

pub(crate) fn focused_node<W: Driven>(window: &W) -> Option<NodeId> {
    window.document().inner.borrow().get_focussed_node_id()
}

pub(crate) fn attr_of<W: Driven>(window: &W, node: NodeId, name: &str) -> Option<String> {
    window
        .document()
        .inner
        .borrow()
        .get_node(node)?
        .attr(LocalName::from(name))
        .map(str::to_owned)
}

/// Whether `node` is `root` or inside it.
pub(crate) fn within<W: Driven>(window: &W, root: NodeId, node: NodeId) -> bool {
    let doc = window.document().inner.borrow();
    std::iter::successors(Some(node), |&id| doc.get_node(id).and_then(|n| n.parent))
        .any(|id| id == root)
}

/// What `node` is called, for a failure's message.
pub(crate) fn name_of<W: Driven>(window: &W, node: NodeId) -> String {
    let text = window
        .document()
        .inner
        .borrow()
        .get_node(node)
        .map(|node| node.text_content())
        .unwrap_or_default();
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let label = attr_of(window, node, "aria-label")
        .or_else(|| attr_of(window, node, "id"))
        .unwrap_or_default();
    format!("{label}|{}", text.chars().take(40).collect::<String>())
}

/// Moves a pad doesn't make from `node`: a slider takes left and right, a
/// combo box up and down.
fn kept<W: Driven>(window: &W, node: NodeId, action: Action) -> bool {
    let role = attr_of(window, node, "role");
    let horizontal = matches!(action, Action::NavLeft | Action::NavRight);
    let slider = role.as_deref() == Some("slider")
        || attr_of(window, node, "type").as_deref() == Some("range");
    (horizontal && slider) || (!horizontal && role.as_deref() == Some("combobox"))
}

/// The most controls a walk visits before it gives up.
const WALK: usize = 80;

/// Walks a pad's directions from where focus is in `root` (or, if it's
/// elsewhere, from where a pad enters) until focus is on a control
/// `target` accepts, and leaves it there.
pub(crate) fn reach<W: Driven>(
    window: &mut W,
    root: NodeId,
    what: &str,
    target: impl Fn(&W, NodeId) -> bool,
) -> NodeId {
    const DIRECTIONS: [Action; 4] = [
        Action::NavDown,
        Action::NavRight,
        Action::NavUp,
        Action::NavLeft,
    ];
    let inside = |window: &W| focused_node(window).filter(|&node| within(window, root, node));
    // Nothing in `root` has focus: a pad's first move enters where the view
    // starts (a Settings panel's tab, then right into the panel).
    for action in [Action::NavDown, Action::NavRight, Action::NavRight] {
        if inside(&*window).is_some() {
            break;
        }
        window.press(action);
    }
    let start = inside(&*window).unwrap_or_else(|| {
        panic!(
            "a pad's moves went to {:?}, not into the area holding {what}",
            focused_node(&*window).map(|node| name_of(&*window, node))
        )
    });
    if target(&*window, start) {
        return start;
    }
    let mut seen = vec![start];
    let mut stack = vec![(start, 0usize)];
    while let Some((node, direction)) = stack.pop() {
        let Some(&action) = DIRECTIONS.get(direction) else {
            continue;
        };
        stack.push((node, direction + 1));
        if kept(&*window, node, action) {
            continue;
        }
        if focused_node(&*window) != Some(node) {
            window.put_focus(node);
        }
        window.press(action);
        let Some(now) = inside(&*window) else {
            continue;
        };
        if seen.contains(&now) {
            continue;
        }
        if target(&*window, now) {
            return now;
        }
        seen.push(now);
        assert!(
            seen.len() <= WALK,
            "a pad walked {WALK} controls without reaching {what}"
        );
        stack.push((now, 0));
    }
    let names: Vec<String> = seen.iter().map(|&node| name_of(&*window, node)).collect();
    panic!("a pad never reached {what}; it reached {names:?}")
}

/// The most times [`tab_to`] presses Tab.
const TABS: usize = 120;

/// Presses Tab until focus is on a control `target` accepts.
pub(crate) fn tab_to(
    window: &mut Window,
    what: &str,
    target: impl Fn(&Window, NodeId) -> bool,
) -> NodeId {
    let mut passed = Vec::new();
    for _ in 0..TABS {
        window.key(Key::Named(NamedKey::Tab));
        if let Some(node) = window.focused() {
            if target(&*window, node) {
                return node;
            }
            passed.push(name_of(&*window, node));
        }
    }
    panic!("Tab never reached {what}; it passed {passed:?}")
}

// ── The frame over a test's own library ──

/// The library and the services (`use_context_provider` calls) the next
/// [`open_frame`] window has.
type Extra = (DataSource, Rc<dyn Fn()>);

thread_local! {
    static EXTRA: RefCell<Option<Extra>> = const { RefCell::new(None) };
}

fn app() -> Element {
    let (data, provide) = use_hook(|| {
        EXTRA
            .with(|extra| extra.borrow().clone())
            .expect("the test's library")
    });
    use_data_source(move || data);
    use_context_provider(crate::library::view_tests::fake_drop_install);
    use_context_provider(crate::game::launch_tests::fake_steps);
    use_context_provider(crate::game::page_tests::page_data_tests::fake_data);
    use_context_provider(crate::game::editor::tests::fake_edit_data);
    use_context_provider(crate::store::harness::fake_store_data);
    provide();
    rsx! {
        style { {include_str!("../../assets/app.css")} }
        crate::input::InputRoot {
            super::Frame { start: "/" }
        }
    }
}

/// The frame on Home over `data`, with what `provide` puts in context (a
/// panel's services), as the app has them.
pub(crate) fn open_frame(data: DataSource, provide: impl Fn() + 'static) -> Window {
    EXTRA.with(|extra| *extra.borrow_mut() = Some((data, Rc::new(provide))));
    let mut doc = DioxusDocument::new(
        VirtualDom::new(app),
        blitz_dom::DocumentConfig {
            viewport: Some(blitz_traits::shell::Viewport::new(
                1280,
                800,
                1.0,
                blitz_traits::shell::ColorScheme::Dark,
            )),
            ..Default::default()
        },
    );
    doc.initial_build();
    let mut window = Window { doc };
    window.settle_until("Home", |window| {
        window.find("data-cs", "sidebar").is_some() && window.route() == Route::Home
    });
    window
}

// ── Settings by pad ──

/// Moves a pad into the open Settings section's panel: from nowhere a pad
/// enters on the section's tab (as it does arriving from the sidebar), and
/// right goes into the panel.
pub(crate) fn enter_panel(window: &mut Window) -> NodeId {
    let panel = window.get("role", "tabpanel");
    for _ in 0..3 {
        if focused_node(&*window).is_some_and(|node| within(&*window, panel, node)) {
            return panel;
        }
        window.pad(Action::NavRight);
    }
    let at = focused_node(&*window).map(|node| name_of(&*window, node));
    assert!(
        focused_node(&*window).is_some_and(|node| within(&*window, panel, node)),
        "a pad didn't get into the panel; focus is on {at:?}"
    );
    panel
}

/// Settings → `tab` opened from Home over `data` with `provide`'s
/// services, once `ready`; a pad in its panel.
pub(crate) fn settings_by_pad(
    data: DataSource,
    provide: impl Fn() + 'static,
    tab: &str,
    what: &str,
    ready: impl Fn(&Window) -> bool,
) -> (Window, NodeId) {
    let mut window = open_frame(data, provide);
    window.go(&format!("/settings?tab={tab}"));
    window.settle_until(what, |window| {
        window.find("role", "tabpanel").is_some() && ready(window)
    });
    let panel = enter_panel(&mut window);
    (window, panel)
}

/// A pad's B leaves the page for Home, where it was opened from (a first
/// B only leaves a text field it's in).
pub(crate) fn back_home(window: &mut Window) {
    window.pad(Action::Back);
    if window.route() != Route::Home
        && focused_node(&*window).is_some_and(|node| typing(&*window, node))
    {
        window.pad(Action::Back);
    }
    window.settle_until("Home again after B", |window| window.route() == Route::Home);
}

/// Whether `node` is a field typed in.
fn typing(window: &Window, node: NodeId) -> bool {
    let doc = window.doc.inner.borrow();
    doc.get_node(node)
        .and_then(|node| node.data.downcast_element())
        .is_some_and(|element| matches!(element.name.local.as_ref(), "input" | "textarea"))
}
