//! A menu of actions, as `@caskade/ui`'s `MenuPopover`/`Menu`/`MenuItem`:
//! a button's dropdown ([`MenuButton`]) or a menu opened at a point. Items
//! are `role="menuitem"`; arrows, Home and End move between them, typing
//! jumps to the item starting with what was typed, Enter, Space, a click or
//! a pad's A run one and close the menu; Escape, Back or a click outside
//! close it, and focus goes back to the trigger (the input layer's overlay
//! scope).

use std::time::{Duration, Instant};

use blitz_dom::NodeId;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::button::{Button, ButtonSize, ButtonVariant};
use super::icons::{Icon, Svg};
use super::ids::use_id;
use super::interact::{Keys, flag, use_interaction_where};
use super::popover::{Anchor, Popover, use_trigger};
use crate::input::use_input;

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("list", "The list of items."),
        (
            "item",
            "An action; carries `data-focused` and `data-disabled`, and `data-tone=\"danger\"` when it removes something.",
        ),
        ("icon", "An item's icon."),
        ("label", "An item's text."),
        ("description", "A second line under an item's text."),
        ("section", "A group of items."),
        ("section-title", "A group's heading."),
        ("separator", "A line between groups."),
    ],
    interaction: &["data-placement", "data-entering", "data-exiting"],
    ..Entry::new(
        "menu",
        "Menu, MenuButton (apps/native/src/ui/menu.rs)",
        Source::Ui,
        "A menu of actions, rendered in a portal: a button's dropdown, or a right click's menu at the pointer.",
    )
};

/// One action in a menu.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    /// What [`Menu`]'s `on_action` gets.
    pub id: String,
    pub label: String,
    pub icon: Option<Icon>,
    /// A second line under the label.
    pub description: Option<String>,
    /// It removes or destroys something.
    pub danger: bool,
    pub disabled: bool,
}

impl MenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            description: None,
            danger: false,
            disabled: false,
        }
    }
}

/// What a menu lists.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item(MenuItem),
    /// A group of items, with an optional heading.
    Section {
        title: Option<String>,
        items: Vec<MenuItem>,
    },
    Separator,
}

impl MenuEntry {
    fn items(&self) -> Vec<&MenuItem> {
        match self {
            Self::Item(item) => vec![item],
            Self::Section { items, .. } => items.iter().collect(),
            Self::Separator => Vec::new(),
        }
    }
}

/// How long typed letters add up for typeahead.
const TYPEAHEAD: Duration = Duration::from_millis(600);

/// Arrows, Home, End and typeahead over `items` (their nodes and labels,
/// enabled ones only), from the focused one; the node to focus.
pub(crate) fn roving(
    key: &Key,
    items: &[(NodeId, String)],
    focused: Option<NodeId>,
    typed: &mut (String, Instant),
) -> Option<NodeId> {
    if items.is_empty() {
        return None;
    }
    let at = focused.and_then(|node| items.iter().position(|(item, _)| *item == node));
    let last = items.len() - 1;
    let index = match key {
        Key::Named(NamedKey::ArrowDown) => at.map_or(0, |at| if at == last { 0 } else { at + 1 }),
        Key::Named(NamedKey::ArrowUp) => at.map_or(last, |at| if at == 0 { last } else { at - 1 }),
        Key::Named(NamedKey::Home) => 0,
        Key::Named(NamedKey::End) => last,
        Key::Character(text) if text.chars().all(|c| !c.is_whitespace()) => {
            if typed.1.elapsed() > TYPEAHEAD {
                typed.0.clear();
            }
            typed.0.push_str(&text.to_lowercase());
            typed.1 = Instant::now();
            let start = at.map_or(0, |at| {
                if typed.0.chars().count() == 1 {
                    at + 1
                } else {
                    at
                }
            });
            (0..items.len())
                .map(|offset| (start + offset) % items.len())
                .find(|&index| items[index].1.to_lowercase().starts_with(&typed.0))?
        }
        _ => return None,
    };
    Some(items[index].0)
}

/// A menu at `anchor`. `on_action` gets the chosen item's id; the menu then
/// closes (`on_close`).
#[component]
pub fn Menu(
    anchor: Anchor,
    items: Vec<MenuEntry>,
    on_action: EventHandler<String>,
    on_close: EventHandler<()>,
    /// The menu's name, for screen readers: the trigger's label.
    #[props(default)]
    label: Option<String>,
    /// What gets focus back when it closes: its trigger.
    #[props(default)]
    return_to: Option<NodeId>,
    /// Shown above the items, not one of them: who a menu is about.
    #[props(default)]
    header: Option<Element>,
) -> Element {
    let input = use_input();
    let mut list = use_signal(|| None::<NodeHandle>);
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    let mut typed = use_signal(|| (String::new(), Instant::now()));
    let enabled: Vec<(String, String)> = items
        .iter()
        .flat_map(MenuEntry::items)
        .filter(|item| !item.disabled)
        .map(|item| (item.id.clone(), item.label.clone()))
        .collect();
    let choose = move |id: String| {
        on_action.call(id);
        on_close.call(());
    };
    let first = enabled.first().map(|(id, _)| id.clone());
    let menu_id = use_id("menu");
    rsx! {
        Popover {
            anchor,
            on_close,
            return_to,
            data_cs: "menu",
            class: "z-40 min-w-56 max-w-80 rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 outline-none animate-enter",
            {header}
            div {
                "data-part": "list",
                role: "menu",
                "aria-label": label,
                class: "max-h-[70vh] overflow-y-auto outline-none",
                onmounted: move |event| list.set(event.data().downcast::<NodeHandle>().cloned()),
                onkeydown: move |event| {
                    let known = nodes.peek().clone();
                    let order: Vec<(NodeId, String)> = enabled
                        .iter()
                        .filter_map(|(id, label)| {
                            known.iter().find(|(known, _)| known == id).map(|(_, node)| (*node, label.clone()))
                        })
                        .collect();
                    let focused = list.peek().as_ref().and_then(|handle| handle.doc().get_focussed_node_id());
                    if let Some(next) = roving(&event.key(), &order, focused, &mut typed.write()) {
                        event.prevent_default();
                        event.stop_propagation();
                        input.focus(next);
                    }
                },
                for (index, entry) in items.iter().enumerate() {
                    match entry {
                        MenuEntry::Item(item) => rsx! {
                            Item { key: "{index}", item: item.clone(), first: first.as_ref() == Some(&item.id), nodes, on_choose: choose }
                        },
                        MenuEntry::Section { title, items } => rsx! {
                            div {
                                key: "{index}",
                                "data-part": "section",
                                role: "group",
                                "aria-labelledby": title.as_ref().map(|_| format!("{menu_id}-section-{index}")),
                                if let Some(title) = title {
                                    div {
                                        id: "{menu_id}-section-{index}",
                                        "data-part": "section-title",
                                        class: "px-2.5 pt-2 pb-1 font-mono text-[10px] tracking-wider text-fg-subtle uppercase",
                                        "{title}"
                                    }
                                }
                                for item in items {
                                    Item { key: "{item.id}", item: item.clone(), first: first.as_ref() == Some(&item.id), nodes, on_choose: choose }
                                }
                            }
                        },
                        MenuEntry::Separator => rsx! {
                            div {
                                key: "{index}",
                                "data-part": "separator",
                                role: "separator",
                                class: "mx-1 my-1 h-px bg-border",
                            }
                        },
                    }
                }
            }
        }
    }
}

#[component]
fn Item(
    item: MenuItem,
    first: bool,
    nodes: Signal<Vec<(String, NodeId)>>,
    on_choose: EventHandler<String>,
) -> Element {
    let interaction = use_interaction_where(first);
    let label_id = use_id("menu-item");
    let id = item.id.clone();
    let disabled = item.disabled;
    let choose = use_callback(move |()| {
        if !disabled {
            on_choose.call(id.clone());
        }
    });
    let mount_id = item.id.clone();
    rsx! {
        div {
            role: "menuitem",
            tabindex: "-1",
            "aria-labelledby": "{label_id}",
            "aria-disabled": flag(disabled),
            "data-part": "item",
            "data-tone": item.danger.then_some("danger"),
            "data-focused": interaction.focused(),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-disabled": flag(disabled),
            "data-focus-default": flag(first),
            class: "flex cursor-default items-start gap-2.5 rounded-[6px] px-2.5 py-1.5 text-sm text-fg-muted outline-none console:data-focus-visible:focus-ring data-focused:bg-surface-hover data-focused:text-fg data-disabled:opacity-40 data-[tone=danger]:text-danger",
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut nodes = nodes;
                    nodes.write().retain(|(known, _)| *known != mount_id);
                    nodes.write().push((mount_id.clone(), node));
                }
            },
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| choose(()),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::EnterAndSpace, || choose(()));
            },
            onkeyup: move |event| interaction.key_up(&event, || choose(())),
            if let Some(icon) = item.icon {
                Svg { icon, part: "icon", class: "mt-0.5 size-4 shrink-0" }
            }
            span { class: "flex min-w-0 flex-col",
                span { id: "{label_id}", "data-part": "label", "{item.label}" }
                if let Some(description) = &item.description {
                    span { "data-part": "description", class: "text-xs text-fg-subtle", "{description}" }
                }
            }
        }
    }
}

/// A button that opens a menu of `items` below it (`aria-haspopup`), as
/// the webview's `ActionMenu`: Enter, Space, ArrowDown, a click or a pad's A
/// open it.
#[component]
pub fn MenuButton(
    items: Vec<MenuEntry>,
    on_action: EventHandler<String>,
    /// The button's text; with `icon` only, its `aria-label`.
    label: String,
    #[props(default)] icon: Option<Icon>,
    #[props(default = ButtonVariant::Ghost)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default)] disabled: bool,
    #[props(default)] id: Option<String>,
) -> Element {
    let trigger = use_trigger();
    let mut open = use_signal(|| None::<Anchor>);
    let mut show = move || {
        if let Some(anchor) = trigger.anchor() {
            open.set(Some(anchor));
        }
    };
    let icon_only = icon.is_some() && label.is_empty();
    rsx! {
        Button {
            variant,
            size,
            icon,
            disabled,
            on_press: move |_| show(),
            "aria-haspopup": "menu",
            "aria-expanded": if open().is_some() { "true" } else { "false" },
            "aria-label": icon_only.then(|| label.clone()),
            on_mounted: move |event| trigger.mounted(event),
            on_key_down: move |event: KeyboardEvent| {
                if event.key() == Key::Named(NamedKey::ArrowDown) {
                    event.prevent_default();
                    event.stop_propagation();
                    show();
                }
            },
            id,
            if !icon_only {
                "{label}"
            }
        }
        if let Some(anchor) = open() {
            Menu {
                anchor,
                items: items.clone(),
                on_action,
                on_close: move |_| open.set(None),
                label: label.clone(),
                return_to: trigger.node(),
            }
        }
    }
}
