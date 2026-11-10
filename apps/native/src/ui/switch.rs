//! An on/off toggle, as `@caskade/ui`'s `Switch`: `role="switch"`, toggled
//! by a click, Space or a pad's A. The control is controlled: `checked`
//! comes in, `on_change` asks for the other value.

use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::ids::use_id;
use super::interact::{Keys, flag, use_interaction};
use super::settings::use_row_label;

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("track", "The pill."),
        ("thumb", "The knob."),
        ("label", "The label, when given."),
    ],
    states: &[("data-size", &["sm", "md"])],
    interaction: &[
        "data-hovered",
        "data-pressed",
        "data-focus-visible",
        "data-disabled",
        "data-selected",
        "data-readonly",
    ],
    ..Entry::new(
        "switch",
        "Switch (apps/native/src/ui/switch.rs)",
        Source::Ui,
        "An on/off toggle.",
    )
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchSize {
    Sm,
    #[default]
    Md,
}

#[component]
pub fn Switch(
    checked: bool,
    /// The value it should take: the other one.
    #[props(default)]
    on_change: Option<EventHandler<bool>>,
    #[props(default)] size: SwitchSize,
    #[props(default)] disabled: bool,
    /// Shown, but not changed by the user.
    #[props(default)]
    readonly: bool,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    /// The label; without one, give it `aria-label`.
    #[props(default)]
    label: Option<String>,
) -> Element {
    let interaction = use_interaction();
    // The label names the control: AccessKit doesn't take a switch's name
    // from its content.
    let label_id = use_id("switch-label");
    let toggle = move || {
        if !disabled
            && !readonly
            && let Some(on_change) = on_change
        {
            on_change.call(!checked);
        }
    };
    let (name, track, thumb) = match size {
        SwitchSize::Sm => (
            "sm",
            "h-5 w-9",
            "size-3.5 group-data-selected:translate-x-4",
        ),
        SwitchSize::Md => (
            "md",
            "h-6 w-11",
            "size-4.5 group-data-selected:translate-x-5",
        ),
    };
    // Without a label of its own, a setting row's names it.
    let row = use_row_label();
    let labelledby = match &row {
        Some(row) if label.is_none() => Some(row.label_id.clone()),
        _ => label.is_some().then(|| label_id.clone()),
    };
    let describedby = row
        .filter(|_| label.is_none())
        .and_then(|row| row.description_id);
    rsx! {
        div {
            role: "switch",
            "aria-labelledby": labelledby,
            "aria-describedby": describedby,
            tabindex: if disabled { "-1" } else { "0" },
            "aria-checked": if checked { "true" } else { "false" },
            "aria-disabled": flag(disabled),
            "aria-readonly": flag(readonly),
            "data-cs": "switch",
            "data-size": name,
            "data-selected": flag(checked),
            "data-disabled": flag(disabled),
            "data-readonly": flag(readonly),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "group inline-flex items-center gap-2.5 text-sm data-disabled:opacity-40 {class}",
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| toggle(),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::Space, || {});
            },
            onkeyup: move |event| interaction.key_up(&event, toggle),
            ..attributes,
            span {
                "data-part": "track",
                class: "flex shrink-0 items-center rounded-full border border-border-strong bg-surface-hover px-0.5 transition-[background-color,border-color,box-shadow] duration-(--cs-motion-duration-base) group-data-focus-visible:focus-ring group-data-selected:border-accent-alt/60 group-data-selected:bg-accent group-data-selected:shadow-[0_0_14px_-3px] group-data-selected:shadow-accent-alt {track}",
                span {
                    "data-part": "thumb",
                    class: "rounded-full bg-fg-muted shadow-sm transition-[translate,background-color] duration-(--cs-motion-duration-base) ease-(--cs-motion-ease-out) group-data-selected:bg-white {thumb}",
                }
            }
            if let Some(label) = label {
                span { id: "{label_id}", "data-part": "label", "{label}" }
            }
        }
    }
}
