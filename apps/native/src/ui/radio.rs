//! A radio group, as react-aria's `RadioGroup`: one tab stop (the checked
//! radio, else the first); arrows move and check the next radio, as the
//! APG has it; a click, Space or a pad's A checks one, and a pad moves
//! between them spatially. No catalogued root: the view around it styles
//! it (`data-part`).

use blitz_dom::NodeId;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::ids::use_id;
use super::interact::{Keys, flag, use_interaction};
use crate::input::use_input;

#[derive(Clone, Debug, PartialEq)]
pub struct RadioOption {
    pub value: String,
    pub label: String,
    /// A second line under the label.
    pub description: Option<String>,
    pub disabled: bool,
}

impl RadioOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
    }
}

#[component]
pub fn RadioGroup(
    options: Vec<RadioOption>,
    value: Option<String>,
    on_change: EventHandler<String>,
    /// The group's visible label; without one, give it `aria-label`.
    #[props(default)]
    label: Option<String>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let label_id = use_id("radio-group");
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    let enabled: Vec<String> = options
        .iter()
        .filter(|option| !option.disabled)
        .map(|option| option.value.clone())
        .collect();
    let stop = value
        .clone()
        .filter(|value| enabled.contains(value))
        .or_else(|| enabled.first().cloned());
    // Each radio's neighbours among the enabled ones: where arrows go.
    let neighbour = move |from: &str, forward: bool| -> Option<String> {
        let at = enabled.iter().position(|value| value == from)?;
        let next = if forward {
            (at + 1) % enabled.len()
        } else {
            (at + enabled.len() - 1) % enabled.len()
        };
        Some(enabled[next].clone())
    };
    let arrows: Vec<(Option<String>, Option<String>)> = options
        .iter()
        .map(|option| {
            (
                neighbour(&option.value, false),
                neighbour(&option.value, true),
            )
        })
        .collect();
    rsx! {
        div {
            role: "radiogroup",
            "aria-labelledby": label.as_ref().map(|_| label_id.clone()),
            class: "flex flex-col gap-2 {class}",
            ..attributes,
            if let Some(label) = &label {
                span { id: "{label_id}", "data-part": "label", class: "text-sm font-medium", "{label}" }
            }
            for (option, (previous, next)) in options.clone().into_iter().zip(arrows) {
                Radio {
                    key: "{option.value}",
                    checked: Some(&option.value) == value.as_ref(),
                    stop: Some(&option.value) == stop.as_ref(),
                    nodes,
                    on_check: on_change,
                    previous,
                    next,
                    option,
                }
            }
        }
    }
}

#[component]
fn Radio(
    option: RadioOption,
    checked: bool,
    stop: bool,
    nodes: Signal<Vec<(String, NodeId)>>,
    on_check: EventHandler<String>,
    /// Where the arrows go: the enabled radios before and after.
    previous: Option<String>,
    next: Option<String>,
) -> Element {
    let interaction = use_interaction();
    let input = use_input();
    let arrow = use_callback(move |forward: bool| {
        let target = if forward {
            next.clone()
        } else {
            previous.clone()
        };
        if let Some(target) = target {
            on_check.call(target.clone());
            if let Some((_, node)) = nodes.peek().iter().find(|(known, _)| *known == target) {
                input.focus(*node);
            }
        }
    });
    let label_id = use_id("radio");
    let value = option.value.clone();
    let disabled = option.disabled;
    let check = use_callback(move |()| {
        if !disabled && !checked {
            on_check.call(value.clone());
        }
    });
    let mount_value = option.value.clone();
    rsx! {
        div {
            role: "radio",
            tabindex: if stop { "0" } else { "-1" },
            "aria-checked": if checked { "true" } else { "false" },
            "aria-disabled": flag(disabled),
            "aria-labelledby": "{label_id}",
            "data-part": "radio",
            "data-selected": flag(checked),
            "data-disabled": flag(disabled),
            "data-hovered": interaction.hovered(),
            "data-pressed": interaction.pressed(),
            class: "group flex items-start gap-3 text-sm data-disabled:opacity-40",
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut nodes = nodes;
                    nodes.write().retain(|(known, _)| *known != mount_value);
                    nodes.write().push((mount_value.clone(), node));
                }
            },
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| check(()),
            onkeydown: move |event| {
                let forward = match event.key() {
                    Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => Some(true),
                    Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => Some(false),
                    _ => None,
                };
                if let Some(forward) = forward {
                    event.prevent_default();
                    event.stop_propagation();
                    arrow(forward);
                } else {
                    interaction.key_down(&event, Keys::Space, || {});
                }
            },
            onkeyup: move |event| interaction.key_up(&event, || check(())),
            span {
                "data-part": "circle",
                class: "mt-0.5 grid size-4.5 shrink-0 place-items-center rounded-full border border-border-strong bg-surface-base/60 transition-[border-color] duration-(--cs-motion-duration-fast) group-data-hovered:border-accent-alt/50 group-data-focus-visible:focus-ring group-data-selected:border-accent-alt",
                span { class: "size-2 rounded-full bg-accent-alt opacity-0 group-data-selected:opacity-100" }
            }
            span { class: "flex flex-col",
                span { id: "{label_id}", "data-part": "label", "{option.label}" }
                if let Some(description) = &option.description {
                    span { "data-part": "description", class: "text-xs text-fg-subtle", "{description}" }
                }
            }
        }
    }
}
