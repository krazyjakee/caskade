//! What every popup is built on (#93): a fixed box placed beside its
//! trigger (below it, or above when below doesn't fit), with
//! `data-placement` (which also makes it an overlay to the input layer:
//! focus goes in when it opens and comes back to the trigger when it
//! closes, as react-aria's focus scope does). It closes on Escape, a pad's
//! Back and a click outside it.

use std::cell::Cell;
use std::rc::Rc;

use blitz_dom::NodeId;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use crate::input::{Bind, use_input, use_input_action};
use caskade_ui_input::Action;

/// Where a popup sits, from its trigger's box in window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The window's height, to flip above when below doesn't fit.
    pub viewport_height: f64,
}

impl Anchor {
    /// The trigger's box, read from the document (in an event handler,
    /// where the document isn't being changed).
    pub fn of(handle: &NodeHandle) -> Option<Self> {
        let doc = handle.doc();
        let rect = doc.get_client_bounding_rect(handle.node_id())?;
        Some(Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            viewport_height: f64::from(doc.viewport().window_size.1) / doc.viewport().scale_f64(),
        })
    }
}

/// The trigger of a popup: its element, for its box and to give focus
/// back to.
#[derive(Clone, Copy, PartialEq)]
pub struct Trigger {
    handle: Signal<Option<NodeHandle>>,
}

pub fn use_trigger() -> Trigger {
    Trigger {
        handle: use_signal(|| None),
    }
}

impl Trigger {
    /// For the trigger's `onmounted`.
    pub fn mounted(mut self, event: Event<MountedData>) {
        self.handle
            .set(event.data().downcast::<NodeHandle>().cloned());
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self.handle.peek().as_ref().and_then(Anchor::of)
    }

    pub fn node(&self) -> Option<NodeId> {
        self.handle.peek().as_ref().map(NodeHandle::node_id)
    }
}

/// How far below (or above) its trigger a popup sits, in pixels.
pub const OFFSET: f64 = 6.0;
/// How much room below the trigger a popup wants before it flips above.
const ROOM: f64 = 240.0;

/// A popup's place: `top`/`left` from the anchor, and which side it's on.
pub fn place(anchor: Anchor, match_width: bool) -> (String, &'static str) {
    let below = anchor.y + anchor.height + OFFSET;
    let width = if match_width {
        format!(" min-width: {}px;", anchor.width)
    } else {
        String::new()
    };
    if below + ROOM > anchor.viewport_height && anchor.y > anchor.viewport_height / 2.0 {
        let bottom = anchor.viewport_height - anchor.y + OFFSET;
        (
            format!(
                "position: fixed; left: {}px; bottom: {bottom}px;{width}",
                anchor.x
            ),
            "top",
        )
    } else {
        (
            format!(
                "position: fixed; left: {}px; top: {below}px;{width}",
                anchor.x
            ),
            "bottom",
        )
    }
}

/// Focus for an overlay: the input layer moves it in over the next frames
/// (overlays a component opens itself, on a key, aren't its own actions),
/// and when the overlay goes, focus goes back to what had it when it
/// opened. Blitz leaves focus on the input root when the focused element
/// goes, which the input layer can't tell from a choice, so the overlay
/// gives it back itself. Call [`OverlayFocus::mounted`] from the overlay's
/// `onmounted`.
#[derive(Clone)]
pub struct OverlayFocus(Rc<Cell<Option<NodeId>>>);

impl OverlayFocus {
    /// What gets focus back when the overlay goes: `node`, else what had
    /// it when the overlay mounted.
    pub fn return_to(&self, node: Option<NodeId>) {
        if node.is_some() {
            self.0.set(node);
        }
    }

    pub fn mounted(&self, event: &Event<MountedData>) {
        if self.0.get().is_none()
            && let Some(handle) = event.data().downcast::<NodeHandle>()
        {
            self.0.set(handle.doc().get_focussed_node_id());
        }
    }
}

/// The node to focus first in the subtree at `root`: one marked
/// `data-focus-default`, else the first enabled control.
pub fn start_in(doc: &blitz_dom::BaseDocument, root: NodeId) -> Option<NodeId> {
    let attr = |node: &blitz_dom::Node, name: &str| {
        node.attr(blitz_dom::LocalName::from(name))
            .map(str::to_owned)
    };
    let mut order = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        if id != root {
            order.push(id);
        }
        stack.extend(node.children.iter().rev().copied());
    }
    let enabled = |node: &blitz_dom::Node| {
        attr(node, "aria-disabled").as_deref() != Some("true") && attr(node, "disabled").is_none()
    };
    order
        .iter()
        .copied()
        .find(|&id| {
            doc.get_node(id)
                .is_some_and(|node| attr(node, "data-focus-default").is_some() && enabled(node))
        })
        .or_else(|| {
            order.into_iter().find(|&id| {
                doc.get_node(id).is_some_and(|node| {
                    let tag = node
                        .element_data()
                        .map(|element| element.name.local.to_string());
                    let control = matches!(
                        tag.as_deref(),
                        Some("button" | "input" | "select" | "textarea")
                    ) || attr(node, "tabindex").is_some_and(|index| index != "-1")
                        || matches!(
                            attr(node, "role").as_deref(),
                            Some("menuitem" | "option" | "tab" | "radio")
                        );
                    control && enabled(node)
                })
            })
        })
}

pub fn use_overlay_focus() -> OverlayFocus {
    let input = use_input();
    let back = use_hook(|| OverlayFocus(Rc::new(Cell::new(None))));
    use_hook({
        let input = input.clone();
        move || input.settle()
    });
    let gone = back.clone();
    use_drop(move || {
        if let Some(node) = gone.0.get() {
            input.focus(node);
        }
        input.settle();
    });
    back
}

/// A popup: an invisible layer under it closes it on a click outside, and
/// Escape or a pad's Back close it.
#[component]
pub fn Popover(
    anchor: Anchor,
    on_close: EventHandler<()>,
    /// The popup's own `data-cs`.
    data_cs: &'static str,
    #[props(default)] role: Option<&'static str>,
    /// As wide as the trigger at least (a select's list).
    #[props(default)]
    match_width: bool,
    /// Keys inside it, before its own (Escape); stopping the event keeps
    /// it from the popup.
    #[props(default)]
    on_key_down: Option<EventHandler<KeyboardEvent>>,
    /// What gets focus back when it closes: its trigger.
    #[props(default)]
    return_to: Option<NodeId>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let mut me = use_signal(|| None::<NodeId>);
    let focus = use_overlay_focus();
    focus.return_to(return_to);
    let input = use_input();
    use_input_action(
        Action::Back,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            on_close.call(());
            true
        },
    );
    let (style, placement) = place(anchor, match_width);
    rsx! {
        div {
            "aria-hidden": "true",
            style: "position: fixed; inset: 0;",
            onclick: move |_| on_close.call(()),
        }
        div {
            "data-cs": data_cs,
            "data-placement": placement,
            role,
            style: "{style}",
            class: "{class}",
            onmounted: move |event| {
                focus.mounted(&event);
                if let Some(handle) = event.data().downcast::<NodeHandle>() {
                    me.set(Some(handle.node_id()));
                    // Focus goes in at once, whatever opened it.
                    let start = start_in(&handle.doc(), handle.node_id());
                    if let Some(start) = start {
                        input.focus(start);
                    }
                }
            },
            onkeydown: move |event: KeyboardEvent| {
                if let Some(on_key_down) = on_key_down {
                    on_key_down.call(event.clone());
                    if !event.propagates() {
                        return;
                    }
                }
                if event.key() == Key::Named(NamedKey::Escape) {
                    event.prevent_default();
                    event.stop_propagation();
                    on_close.call(());
                }
            },
            ..attributes,
            {children}
        }
    }
}
