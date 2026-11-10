//! A checkbox with its label, as `@caskade/ui`'s `Checkbox`:
//! `role="checkbox"`, ticked by a click, Space or a pad's A; `indeterminate`
//! shows a dash (`aria-checked="mixed"`).

use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{CHECK, MINUS, Svg};
use super::ids::use_id;
use super::interact::{Keys, flag, use_interaction};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("box", "The square."),
        ("mark", "The tick or dash inside the box."),
        ("label", "The label."),
    ],
    interaction: &[
        "data-hovered",
        "data-pressed",
        "data-focus-visible",
        "data-disabled",
        "data-selected",
        "data-indeterminate",
        "data-invalid",
        "data-readonly",
    ],
    ..Entry::new(
        "checkbox",
        "Checkbox (apps/native/src/ui/checkbox.rs)",
        Source::Ui,
        "A checkbox with its label.",
    )
};

#[component]
pub fn Checkbox(
    checked: bool,
    /// The value it should take: the other one (ticked, from indeterminate).
    #[props(default)]
    on_change: Option<EventHandler<bool>>,
    #[props(default)] indeterminate: bool,
    #[props(default)] disabled: bool,
    #[props(default)] readonly: bool,
    #[props(default)] invalid: bool,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    /// The label; without one, give it `aria-label`.
    #[props(default)]
    label: Option<String>,
) -> Element {
    let interaction = use_interaction();
    // The label names the control: AccessKit doesn't take a checkbox's name
    // from its content.
    let label_id = use_id("checkbox-label");
    let toggle = move || {
        if !disabled
            && !readonly
            && let Some(on_change) = on_change
        {
            on_change.call(indeterminate || !checked);
        }
    };
    let state = match (indeterminate, checked) {
        (true, _) => "mixed",
        (false, true) => "true",
        (false, false) => "false",
    };
    let labelledby = label.is_some().then(|| label_id.clone());
    rsx! {
        div {
            role: "checkbox",
            "aria-labelledby": labelledby,
            tabindex: if disabled { "-1" } else { "0" },
            "aria-checked": state,
            "aria-disabled": flag(disabled),
            "aria-readonly": flag(readonly),
            "aria-invalid": flag(invalid),
            "data-cs": "checkbox",
            "data-selected": flag(checked && !indeterminate),
            "data-indeterminate": flag(indeterminate),
            "data-disabled": flag(disabled),
            "data-readonly": flag(readonly),
            "data-invalid": flag(invalid),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "group flex items-center gap-3 text-sm data-disabled:opacity-40 {class}",
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
                "data-part": "box",
                class: "grid size-4.5 shrink-0 place-items-center rounded-[5px] border border-border-strong bg-surface-base/60 transition-[background-color,border-color] duration-(--cs-motion-duration-fast) group-data-hovered:border-accent-alt/50 group-data-focus-visible:focus-ring group-data-indeterminate:border-accent-alt/60 group-data-indeterminate:bg-accent group-data-selected:border-accent-alt/60 group-data-selected:bg-accent",
                if indeterminate {
                    Svg { icon: MINUS, part: "mark", stroke_width: 3.0, class: "size-3 text-accent-fg" }
                } else {
                    Svg { icon: CHECK, part: "mark", stroke_width: 3.0, class: "size-3 text-accent-fg opacity-0 transition-opacity duration-(--cs-motion-duration-fast) group-data-selected:opacity-100" }
                }
            }
            if let Some(label) = label {
                span { id: "{label_id}", "data-part": "label", "{label}" }
            }
        }
    }
}
