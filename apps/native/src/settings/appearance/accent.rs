//! Appearance → Accent colour: swatch grid, custom `#rrggbb` field, and
//! readable-accent contrast checks against the active theme's surfaces.

use caskade_ui_i18n::{t, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use crate::ui::{Button, ButtonSize, ButtonVariant, TextField};

/// Example in the empty accent field: hex colour syntax, not translated.
const ACCENT_EXAMPLE: &str = "#5b5ff7";

/// Accents a pad can choose (#61). The ids are the colours themselves, in
/// the setting's `#rrggbb` format.
fn swatches() -> [(&'static str, String); 8] {
    [
        ("#3b82f6", tp!("accent colour", "Blue")),
        ("#06b6d4", tp!("accent colour", "Cyan")),
        ("#22c55e", tp!("accent colour", "Green")),
        ("#f59e0b", tp!("accent colour", "Amber")),
        ("#f97316", tp!("accent colour", "Orange")),
        ("#ef4444", tp!("accent colour", "Red")),
        ("#ec4899", tp!("accent colour", "Pink")),
        ("#a855f7", tp!("accent colour", "Violet")),
    ]
}

/// What the accent picker says about a pick: a note, or why it was refused.
#[derive(Clone, PartialEq)]
struct Note {
    text: String,
    refused: bool,
}

/// The theme's surfaces as the window draws them, for the accent's
/// contrast check.
fn surfaces(handle: &NodeHandle) -> Vec<caskade_ui_theme::accent::Rgb> {
    let doc = handle.doc();
    let Some(root) = doc.try_root_element().map(|root| root.id) else {
        return Vec::new();
    };
    caskade_ui_theme::appearance::surfaces(
        doc.resolved_style_value(root, "--cs-color-surface-base")
            .trim(),
        doc.resolved_style_value(root, "--cs-color-surface-raised")
            .trim(),
    )
}

/// The theme's accent unless the user picked one; a pick that would be
/// hard to see on this theme's surfaces is adjusted (with a note) or
/// refused. Swatches for a pad and a finger; a `#rrggbb` field for any
/// other colour (Blitz has no colour dialog, and a field works with the
/// on-screen keyboard too).
#[component]
pub(super) fn AccentPicker(
    value: Option<String>,
    on_change: EventHandler<Option<String>>,
) -> Element {
    let mut note = use_signal(|| None::<Note>);
    let mut node = use_signal(|| None::<NodeHandle>);
    let derive = move |colour: &str| {
        let surfaces = node.peek().as_ref().map(surfaces).unwrap_or_default();
        caskade_ui_theme::accent::derive(colour, &surfaces)
    };
    let mut pick = move |colour: String| match derive(&colour) {
        Err(message) => note.set(Some(Note {
            text: message,
            refused: true,
        })),
        Ok(accent) => {
            note.set(accent.message.clone().map(|text| Note {
                text,
                refused: false,
            }));
            on_change.call(Some(accent.accent.clone()));
        }
    };
    // A swatch adjusted for this theme is saved as its adjusted colour.
    let chosen = value.as_ref().and_then(|value| {
        swatches()
            .into_iter()
            .find(|(id, _)| derive(id).is_ok_and(|accent| &accent.accent == value))
            .map(|(id, _)| id)
    });
    // What's typed in the colour field, until Enter.
    let mut typed = use_signal(|| None::<String>);
    let shown = typed().unwrap_or_else(|| value.clone().unwrap_or_default());
    rsx! {
        div {
            "data-part": "accent",
            class: "flex flex-col items-end gap-1.5",
            onmounted: move |event| node.set(event.data().downcast::<NodeHandle>().cloned()),
            div {
                role: "radiogroup",
                "aria-label": t!("Accent colour"),
                "data-part": "accent-swatches",
                class: "flex flex-wrap justify-end gap-1.5",
                for (id, label) in swatches() {
                    Swatch {
                        key: "{id}",
                        colour: id,
                        label,
                        selected: chosen == Some(id),
                        on_pick: move |colour: String| pick(colour),
                    }
                }
            }
            div { class: "flex items-center gap-2",
                div { class: "w-32",
                    TextField {
                        value: shown,
                        mono: true,
                        placeholder: ACCENT_EXAMPLE.to_owned(),
                        "aria-label": t!("Custom accent colour"),
                        on_input: move |next: String| typed.set(Some(next)),
                        on_submit: move |next: String| {
                            typed.set(None);
                            let next = next.trim().to_owned();
                            if !next.is_empty() {
                                pick(next);
                            }
                        },
                    }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    disabled: value.is_none(),
                    on_press: move |_| {
                        note.set(None);
                        on_change.call(None);
                    },
                    {t!("Theme accent")}
                }
            }
            if let Some(note) = note() {
                p {
                    role: if note.refused { "alert" } else { "status" },
                    class: if note.refused { "max-w-64 text-right text-xs text-danger" } else { "max-w-64 text-right text-xs text-fg-muted" },
                    "{note.text}"
                }
            }
        }
    }
}

/// One accent swatch: a radio in its colour, pressed by click, Enter,
/// Space or a pad's A.
#[component]
fn Swatch(
    colour: &'static str,
    label: String,
    selected: bool,
    on_pick: EventHandler<String>,
) -> Element {
    let interaction = crate::ui::interact::use_interaction();
    let press = move || on_pick.call(colour.to_owned());
    rsx! {
        button {
            r#type: "button",
            role: "radio",
            "data-part": "accent-swatch",
            "aria-label": "{label}",
            "aria-checked": if selected { "true" } else { "false" },
            "data-selected": crate::ui::interact::flag(selected),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            style: "background-color: {colour}",
            class: "size-8 rounded-control border border-border-strong outline-offset-2 data-focus-visible:focus-ring data-hovered:brightness-115 data-selected:outline-2 data-selected:outline-fg",
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| press(),
            onkeydown: move |event| {
                interaction.key_down(&event, crate::ui::interact::Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
        }
    }
}
