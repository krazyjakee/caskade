//! A dropdown choice, as `@caskade/ui`'s `Select`: a trigger button showing
//! the chosen option, and its list (`select-popover`, `role="listbox"`) with
//! the chosen one focused first. Arrows, Home, End and typing move in the
//! list; Enter, Space, a click or a pad's A choose and close it.

use std::time::Instant;

use blitz_dom::NodeId;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::icons::{CHECK, CHEVRON_DOWN, Svg};
use super::ids::use_id;
use super::interact::{Keys, flag, use_interaction, use_interaction_where};
use super::menu::roving;
use super::popover::{Anchor, Popover, use_trigger};
use super::settings::use_row_label;
use crate::input::use_input;

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("label", "The visible label, when given."),
        (
            "trigger",
            "The button that opens the list; carries `data-hovered`, `data-pressed`, `data-focus-visible`.",
        ),
        ("value", "The selected option's text."),
        ("chevron", "The arrow on the trigger."),
    ],
    interaction: &[
        "data-open",
        "data-focused",
        "data-focus-visible",
        "data-disabled",
        "data-invalid",
    ],
    contains: &["select-popover"],
    ..Entry::new(
        "select",
        "Select (apps/native/src/ui/select.rs)",
        Source::Ui,
        "A dropdown choice; its list opens in a `select-popover`.",
    )
};

pub const POPOVER_ANATOMY: Entry = Entry {
    parts: &[
        (
            "option",
            "An option; carries `data-focused` and `data-selected`.",
        ),
        ("check", "The tick on the selected option."),
    ],
    interaction: &["data-placement", "data-entering", "data-exiting"],
    ..Entry::new(
        "select-popover",
        "Select (apps/native/src/ui/select.rs)",
        Source::Ui,
        "The open list of a `select`, rendered in a portal.",
    )
};

#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }
}

#[component]
pub fn Select(
    options: Vec<SelectOption>,
    /// The chosen option's value.
    value: Option<String>,
    on_change: EventHandler<String>,
    /// The visible label; without one, give it `aria-label`.
    #[props(default)]
    label: Option<String>,
    /// Shown while nothing is chosen.
    #[props(default)]
    placeholder: Option<String>,
    #[props(default)] disabled: bool,
    #[props(default)] invalid: bool,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let interaction = use_interaction();
    let trigger = use_trigger();
    let mut open = use_signal(|| None::<Anchor>);
    let label_id = use_id("select-label");
    let value_id = use_id("select-value");
    let chosen = options
        .iter()
        .find(|option| Some(&option.value) == value.as_ref())
        .map(|option| option.label.clone());
    let mut show = move || {
        if !disabled && let Some(anchor) = trigger.anchor() {
            open.set(Some(anchor));
        }
    };
    // Without a label of its own, a setting row's names it.
    let row = use_row_label();
    let labelledby = match (&label, &row) {
        (Some(_), _) => format!("{label_id} {value_id}"),
        (None, Some(row)) => format!("{} {value_id}", row.label_id),
        (None, None) => value_id.clone(),
    };
    let describedby = row
        .filter(|_| label.is_none())
        .and_then(|row| row.description_id);
    rsx! {
        div {
            "data-cs": "select",
            "data-open": flag(open().is_some()),
            "data-disabled": flag(disabled),
            "data-invalid": flag(invalid),
            "data-focused": interaction.focused(),
            class: "flex flex-col gap-1.5 text-sm data-disabled:opacity-40 {class}",
            if let Some(label) = &label {
                span { id: "{label_id}", "data-part": "label", class: "text-fg-muted", "{label}" }
            }
            button {
                r#type: "button",
                "data-part": "trigger",
                "aria-haspopup": "listbox",
                "aria-expanded": if open().is_some() { "true" } else { "false" },
                "aria-labelledby": "{labelledby}",
                "aria-describedby": describedby,
                "aria-disabled": flag(disabled),
                "aria-invalid": flag(invalid),
                "data-hovered": interaction.hovered(),
                "data-focus-visible": interaction.ring(),
                "data-pressed": interaction.pressed(),
                class: "flex h-9 items-center justify-between gap-2 rounded-control border border-border-strong bg-surface-base/60 px-3 text-left text-fg transition-[border-color] duration-(--cs-motion-duration-fast) data-hovered:border-accent-alt/50 data-focus-visible:focus-ring",
                onmounted: move |event| trigger.mounted(event),
                onfocus: move |_| interaction.focus(),
                onblur: move |_| interaction.blur(),
                onmouseenter: move |_| interaction.enter(),
                onmouseleave: move |_| interaction.leave(),
                onmousedown: move |_| interaction.down(),
                onmouseup: move |_| interaction.up(),
                onclick: move |_| show(),
                onkeydown: move |event| {
                    if matches!(event.key(), Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp)) {
                        event.prevent_default();
                        event.stop_propagation();
                        show();
                    } else {
                        interaction.key_down(&event, Keys::EnterAndSpace, show);
                    }
                },
                onkeyup: move |event| interaction.key_up(&event, show),
                ..attributes,
                span {
                    id: "{value_id}",
                    "data-part": "value",
                    class: if chosen.is_none() { "truncate text-fg-subtle" } else { "truncate" },
                    {chosen.clone().or(placeholder.clone()).unwrap_or_default()}
                }
                Svg { icon: CHEVRON_DOWN, part: "chevron", class: "size-4 shrink-0 text-fg-subtle" }
            }
            if let Some(anchor) = open() {
                List {
                    anchor,
                    options: options.clone(),
                    value: value.clone(),
                    label: label.clone().or(chosen.clone()),
                    on_change,
                    on_close: move |_| open.set(None),
                    return_to: trigger.node(),
                }
            }
        }
    }
}

#[component]
pub(crate) fn List(
    anchor: Anchor,
    options: Vec<SelectOption>,
    value: Option<String>,
    label: Option<String>,
    on_change: EventHandler<String>,
    on_close: EventHandler<()>,
    return_to: Option<NodeId>,
) -> Element {
    let input = use_input();
    let mut list = use_signal(|| None::<NodeHandle>);
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    let mut typed = use_signal(|| (String::new(), Instant::now()));
    let enabled: Vec<(String, String)> = options
        .iter()
        .filter(|option| !option.disabled)
        .map(|option| (option.value.clone(), option.label.clone()))
        .collect();
    // Focus starts on the chosen option, else the first.
    let start = value
        .clone()
        .filter(|value| enabled.iter().any(|(known, _)| known == value))
        .or_else(|| enabled.first().map(|(value, _)| value.clone()));
    let choose = move |chosen: String| {
        on_change.call(chosen);
        on_close.call(());
    };
    rsx! {
        Popover {
            anchor,
            on_close,
            data_cs: "select-popover",
            return_to,
            role: "listbox",
            match_width: true,
            "aria-label": label,
            class: "z-40 max-h-[60vh] overflow-y-auto rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 outline-none animate-enter",
            on_key_down: move |event: KeyboardEvent| {
                let known = nodes.peek().clone();
                let order: Vec<(NodeId, String)> = enabled
                    .iter()
                    .filter_map(|(value, label)| {
                        known.iter().find(|(known, _)| known == value).map(|(_, node)| (*node, label.clone()))
                    })
                    .collect();
                let focused = list.peek().as_ref().and_then(|handle| handle.doc().get_focussed_node_id());
                if let Some(next) = roving(&event.key(), &order, focused, &mut typed.write()) {
                    event.prevent_default();
                    event.stop_propagation();
                    input.focus(next);
                }
            },
            div {
                class: "contents",
                onmounted: move |event| list.set(event.data().downcast::<NodeHandle>().cloned()),
            }
            for option in options {
                OptionItem {
                    key: "{option.value}",
                    selected: Some(&option.value) == value.as_ref(),
                    start: Some(&option.value) == start.as_ref(),
                    option: option.clone(),
                    nodes,
                    on_choose: choose,
                }
            }
        }
    }
}

#[component]
fn OptionItem(
    option: SelectOption,
    selected: bool,
    start: bool,
    nodes: Signal<Vec<(String, NodeId)>>,
    on_choose: EventHandler<String>,
) -> Element {
    let interaction = use_interaction_where(start);
    let label_id = use_id("option");
    let value = option.value.clone();
    let disabled = option.disabled;
    let choose = use_callback(move |()| {
        if !disabled {
            on_choose.call(value.clone());
        }
    });
    let mount_value = option.value.clone();
    rsx! {
        div {
            role: "option",
            tabindex: "-1",
            "aria-labelledby": "{label_id}",
            "aria-selected": if selected { "true" } else { "false" },
            "aria-disabled": flag(disabled),
            "data-part": "option",
            "data-selected": flag(selected),
            "data-focused": interaction.focused(),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-disabled": flag(disabled),
            "data-focus-default": flag(start),
            class: "flex cursor-default items-center gap-2 rounded-[6px] px-2.5 py-1.5 text-sm text-fg-muted outline-none console:data-focus-visible:focus-ring data-focused:bg-surface-hover data-focused:text-fg data-selected:text-fg data-disabled:opacity-40",
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut nodes = nodes;
                    nodes.write().retain(|(known, _)| *known != mount_value);
                    nodes.write().push((mount_value.clone(), node));
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
            span { id: "{label_id}", class: "grow truncate", "{option.label}" }
            if selected {
                Svg { icon: CHECK, part: "check", class: "size-4 shrink-0 text-accent-alt" }
            }
        }
    }
}
