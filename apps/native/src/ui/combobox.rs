//! A combobox, as react-aria's `ComboBox`: a text field that filters a list
//! of options as you type (`role="combobox"`, `aria-autocomplete="list"`).
//! Focus stays in the field; ArrowDown/ArrowUp (or a pad's down and up)
//! move the active option (`aria-activedescendant`), Enter or A chooses it,
//! Escape or Back closes the list. No catalogued root (the webview's search
//! box, `search`, is the app shell's, #96).

use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::icons::{CHECK, Svg};
use super::ids::use_id;
use super::popover::{Anchor, Popover};
use super::select::SelectOption;
use super::text_field::TextField;
use crate::input::{Bind, use_input_action};

#[component]
pub fn Combobox(
    options: Vec<SelectOption>,
    /// The chosen option's value.
    value: Option<String>,
    on_change: EventHandler<String>,
    label: String,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] class: String,
) -> Element {
    let list_id = use_id("combobox-list");
    let chosen_label = options
        .iter()
        .find(|option| Some(&option.value) == value.as_ref())
        .map(|option| option.label.clone())
        .unwrap_or_default();
    let mut text = use_signal(|| chosen_label.clone());
    let mut open = use_signal(|| None::<Anchor>);
    let mut active = use_signal(|| 0usize);
    let mut field = use_signal(|| None::<NodeHandle>);
    let query = text().to_lowercase();
    let matches: Vec<SelectOption> = options
        .iter()
        .filter(|option| !option.disabled && option.label.to_lowercase().contains(&query))
        .cloned()
        .collect();
    let count = matches.len();
    let mut show = move || {
        if open.peek().is_none()
            && let Some(anchor) = field.peek().as_ref().and_then(Anchor::of)
        {
            open.set(Some(anchor));
        }
    };
    let chosen: Vec<(String, String)> = matches
        .iter()
        .map(|option| (option.value.clone(), option.label.clone()))
        .collect();
    let choose = use_callback(move |index: usize| {
        if let Some((value, label)) = chosen.get(index) {
            text.set(label.clone());
            on_change.call(value.clone());
        }
        open.set(None);
    });
    let mut step = move |forward: bool| {
        show();
        if count > 0 {
            let at = *active.peek();
            active.set(if forward {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            });
        }
    };
    let node = field.peek().as_ref().map(NodeHandle::node_id);
    for (action, forward) in [(Action::NavDown, true), (Action::NavUp, false)] {
        use_input_action(
            action,
            Bind {
                owner: node,
                ..Bind::default()
            },
            move || {
                step(forward);
                true
            },
        );
    }
    use_input_action(
        Action::Confirm,
        Bind {
            owner: node,
            ..Bind::default()
        },
        move || {
            if open.peek().is_some() {
                choose(*active.peek());
                true
            } else {
                false
            }
        },
    );
    let active_id = (open().is_some() && count > 0).then(|| format!("{list_id}-{}", active()));
    rsx! {
        div { class: "relative {class}",
            TextField {
                value: text(),
                label: label.clone(),
                placeholder,
                role: "combobox",
                "aria-autocomplete": "list",
                "aria-expanded": if open().is_some() { "true" } else { "false" },
                "aria-controls": "{list_id}",
                "aria-activedescendant": active_id,
                on_mounted: move |event: Event<MountedData>| field.set(event.data().downcast::<NodeHandle>().cloned()),
                on_input: move |typed: String| {
                    text.set(typed);
                    active.set(0);
                    show();
                },
                on_key_down: move |event: KeyboardEvent| {
                    let handled = match event.key() {
                        Key::Named(NamedKey::ArrowDown) => { step(true); true }
                        Key::Named(NamedKey::ArrowUp) => { step(false); true }
                        Key::Named(NamedKey::Enter) if open.peek().is_some() && !event.is_composing() => {
                            choose(*active.peek());
                            true
                        }
                        Key::Named(NamedKey::Escape) if open.peek().is_some() => {
                            open.set(None);
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        event.prevent_default();
                        event.stop_propagation();
                    }
                },
            }
            if let Some(anchor) = open() {
                Popover {
                    anchor,
                    on_close: move |_| open.set(None),
                    data_cs: "select-popover",
                    role: "listbox",
                    keep_focus: true,
                    match_width: true,
                    id: "{list_id}",
                    "aria-label": "{label}",
                    class: "z-40 max-h-[60vh] overflow-y-auto rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 outline-none",
                    for (index, option) in matches.iter().enumerate() {
                        div {
                            key: "{option.value}",
                            id: "{list_id}-{index}",
                            role: "option",
                            "aria-selected": if Some(&option.value) == value.as_ref() { "true" } else { "false" },
                            "data-part": "option",
                            "data-focused": (index == active()).then_some("true"),
                            "data-selected": (Some(&option.value) == value.as_ref()).then_some("true"),
                            class: "flex cursor-default items-center gap-2 rounded-[6px] px-2.5 py-1.5 text-sm text-fg-muted data-focused:bg-surface-hover data-focused:text-fg data-selected:text-fg",
                            onclick: move |_| choose(index),
                            span { class: "grow truncate", "{option.label}" }
                            if Some(&option.value) == value.as_ref() {
                                Svg { icon: CHECK, part: "check", class: "size-4 shrink-0 text-accent-alt" }
                            }
                        }
                    }
                }
            }
        }
    }
}
