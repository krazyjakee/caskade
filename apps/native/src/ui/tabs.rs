//! Tabs, as react-aria's `Tabs`: a tab list (`role="tablist"`) with one tab
//! stop, arrows (and Home, End) moving and selecting as the APG's automatic
//! activation has it, a pad's LB/RB switching tabs anywhere inside, and
//! the selected tab's panel (`role="tabpanel"`) below. No catalogued root:
//! views style it (`data-part`).

use blitz_dom::NodeId;
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::ids::use_id;
use super::interact::{flag, use_interaction_where};
use crate::input::{Bind, use_input, use_input_action};

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: String,
    pub label: String,
}

impl Tab {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Tabs and the selected one's panel (`children`).
#[component]
pub fn Tabs(
    tabs: Vec<Tab>,
    selected: String,
    on_change: EventHandler<String>,
    /// The tab list's name, for screen readers.
    label: String,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    let input = use_input();
    let base = use_id("tabs");
    let mut me = use_signal(|| None::<NodeId>);
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    let ids: Vec<String> = tabs.iter().map(|tab| tab.id.clone()).collect();
    let at = ids.iter().position(|id| *id == selected).unwrap_or(0);
    let go = use_callback(move |index: usize| {
        if let Some(id) = ids.get(index) {
            on_change.call(id.clone());
            if let Some((_, node)) = nodes.peek().iter().find(|(known, _)| known == id) {
                input.focus(*node);
            }
        }
    });
    let count = tabs.len();
    // A pad's LB/RB, anywhere inside the tabs.
    use_input_action(
        Action::TabPrev,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            go((at + count - 1) % count.max(1));
            true
        },
    );
    use_input_action(
        Action::TabNext,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            go((at + 1) % count.max(1));
            true
        },
    );
    rsx! {
        div {
            class: "flex flex-col gap-4 {class}",
            onmounted: move |event| me.set(event.data().downcast::<NodeHandle>().map(NodeHandle::node_id)),
            div {
                role: "tablist",
                "aria-label": "{label}",
                "aria-orientation": "horizontal",
                "data-part": "tab-list",
                class: "flex gap-1 border-b border-border",
                onkeydown: move |event| {
                    let next = match event.key() {
                        Key::Named(NamedKey::ArrowRight) => (at + 1) % count.max(1),
                        Key::Named(NamedKey::ArrowLeft) => (at + count - 1) % count.max(1),
                        Key::Named(NamedKey::Home) => 0,
                        Key::Named(NamedKey::End) => count.saturating_sub(1),
                        _ => return,
                    };
                    event.prevent_default();
                    event.stop_propagation();
                    go(next);
                },
                for (index, tab) in tabs.iter().enumerate() {
                    TabButton {
                        key: "{tab.id}",
                        tab: tab.clone(),
                        selected: index == at,
                        panel: format!("{base}-panel"),
                        id: format!("{base}-tab-{index}"),
                        nodes,
                        on_select: move |_| go(index),
                    }
                }
            }
            div {
                id: "{base}-panel",
                role: "tabpanel",
                "aria-labelledby": "{base}-tab-{at}",
                "data-part": "tab-panel",
                {children}
            }
        }
    }
}

#[component]
fn TabButton(
    tab: Tab,
    selected: bool,
    panel: String,
    id: String,
    nodes: Signal<Vec<(String, NodeId)>>,
    on_select: EventHandler<()>,
) -> Element {
    let interaction = use_interaction_where(selected);
    let mount_id = tab.id.clone();
    rsx! {
        button {
            r#type: "button",
            id: "{id}",
            role: "tab",
            tabindex: if selected { "0" } else { "-1" },
            "aria-selected": if selected { "true" } else { "false" },
            "aria-controls": "{panel}",
            "data-part": "tab",
            "data-selected": flag(selected),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            class: "-mb-px border-b-2 border-transparent px-3 py-2 text-sm text-fg-muted transition-[color,border-color] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:text-fg data-selected:border-accent-alt data-selected:text-fg",
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut nodes = nodes;
                    nodes.write().retain(|(known, _)| *known != mount_id);
                    nodes.write().push((mount_id.clone(), node));
                }
            },
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| on_select.call(()),
            "{tab.label}"
        }
    }
}
