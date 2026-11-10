//! One choice from a few options side by side, as `@caskade/ui`'s
//! `SegmentedControl` (react-aria's single-select `ToggleButtonGroup`):
//! each segment is a toggle button (`aria-pressed`); a click, Enter, Space
//! or a pad's A picks one, and a pad moves between them spatially.

use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{Icon, Svg};
use super::interact::{Keys, flag, use_interaction_where};
use super::settings::use_row_label;

pub const ANATOMY: Entry = Entry {
    parts: &[
        (
            "segment",
            "An option; carries `data-selected`, `data-hovered` and `data-focus-visible`.",
        ),
        ("icon", "A segment's icon."),
    ],
    interaction: &["data-disabled", "data-orientation"],
    ..Entry::new(
        "segmented",
        "SegmentedControl (apps/native/src/ui/segmented.rs)",
        Source::Ui,
        "One choice from a few options shown side by side.",
    )
};

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub id: String,
    pub label: String,
    pub icon: Option<Icon>,
}

impl Segment {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
        }
    }
}

#[component]
pub fn SegmentedControl(
    options: Vec<Segment>,
    value: String,
    on_change: EventHandler<String>,
    #[props(default)] disabled: bool,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    // In a setting row, the row names and describes it.
    let row = use_row_label();
    rsx! {
        div {
            role: "group",
            "aria-labelledby": row.as_ref().map(|row| row.label_id.clone()),
            "aria-describedby": row.as_ref().and_then(|row| row.description_id.clone()),
            "data-cs": "segmented",
            "data-orientation": "horizontal",
            "data-disabled": flag(disabled),
            class: "inline-flex rounded-control border border-border-strong bg-surface-base/60 p-0.5 data-disabled:opacity-40 {class}",
            ..attributes,
            for option in options {
                SegmentButton {
                    key: "{option.id}",
                    selected: option.id == value,
                    disabled,
                    option: option.clone(),
                    on_change,
                }
            }
        }
    }
}

#[component]
fn SegmentButton(
    option: Segment,
    selected: bool,
    disabled: bool,
    on_change: EventHandler<String>,
) -> Element {
    let interaction = use_interaction_where(selected);
    let id = option.id.clone();
    let pick = use_callback(move |()| {
        if !disabled && !selected {
            on_change.call(id.clone());
        }
    });
    rsx! {
        button {
            r#type: "button",
            "data-part": "segment",
            "aria-pressed": if selected { "true" } else { "false" },
            "aria-disabled": flag(disabled),
            "data-selected": flag(selected),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "flex items-center gap-1.5 rounded-[6px] px-3 py-1.5 text-sm whitespace-nowrap text-fg-muted transition-[background-color,color,box-shadow] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:text-fg data-selected:bg-accent/25 data-selected:text-fg data-selected:shadow-[inset_0_0_0_1px] data-selected:shadow-accent-alt/50",
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| pick(()),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::EnterAndSpace, || pick(()));
            },
            onkeyup: move |event| interaction.key_up(&event, || pick(())),
            if let Some(icon) = option.icon {
                Svg { icon, part: "icon", class: "size-4" }
            }
            "{option.label}"
        }
    }
}
