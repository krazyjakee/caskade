//! A failure, shown the same way everywhere, as `@caskade/ui`'s
//! `ErrorNotice`: what failed and why, what to do, and the technical
//! details behind a toggle (`aria-expanded`). It's an alert, so screen
//! readers say it as it appears.

use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{CIRCLE_ALERT, CIRCLE_X, Svg};
use super::ids::use_id;
use super::interact::{Keys, use_interaction};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("icon", "The alert icon."),
        ("title", "What couldn't be done, when given."),
        ("summary", "What failed and, when known, why."),
        ("hints", "What the user can do, one item each."),
        (
            "actions",
            "Recovery controls (e.g. Try again) and the details toggle.",
        ),
        (
            "details-toggle",
            "Show details / Hide details; carries `aria-expanded`.",
        ),
        (
            "details",
            "The redacted technical details and the log reference.",
        ),
        ("reference", "The reference to find the failure in the log."),
    ],
    states: &[
        ("data-variant", &["block", "inline"]),
        ("data-tone", &["danger", "warning"]),
        ("data-expanded", &["true", "false"]),
    ],
    contains: &["button"],
    slots: &["actions"],
    ..Entry::new(
        "error-notice",
        "ErrorNotice (apps/native/src/ui/error_notice.rs)",
        Source::Ui,
        "A failure, shown the same way everywhere: what failed and why, what to do, and the technical details behind a toggle.",
    )
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoticeVariant {
    /// Fills a view or panel.
    #[default]
    Block,
    /// Sits under a control.
    Inline,
}

#[component]
pub fn ErrorNotice(
    /// What failed and, when known, why: the error's summary.
    summary: String,
    /// What couldn't be done, e.g. "Couldn't load your library".
    #[props(default)]
    title: Option<String>,
    /// What the user can do, one sentence each.
    #[props(default)]
    hints: Vec<String>,
    /// Technical details, redacted; behind Show details.
    #[props(default)]
    detail: Option<String>,
    /// The reference logged with the details.
    #[props(default)]
    reference: Option<String>,
    #[props(default)] variant: NoticeVariant,
    /// Caskade is still recovering on its own.
    #[props(default)]
    warning: bool,
    #[props(default)] expanded: bool,
    #[props(default)] class: String,
    /// Recovery controls, e.g. Try again.
    #[props(default)]
    actions: Option<Element>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let mut open = use_signal(|| expanded);
    let interaction = use_interaction();
    let details_id = use_id("error-details");
    let has_details = detail.is_some() || reference.is_some();
    let mut toggle = move || {
        let now = !*open.peek();
        open.set(now);
    };
    let (variant_name, frame) = match variant {
        NoticeVariant::Block => (
            "block",
            "rounded-panel border border-border bg-surface-raised/60 p-5",
        ),
        NoticeVariant::Inline => ("inline", ""),
    };
    let tone = if warning { "warning" } else { "danger" };
    let has_actions = actions.is_some();
    rsx! {
        div {
            role: "alert",
            "data-cs": "error-notice",
            "data-variant": variant_name,
            "data-tone": tone,
            "data-expanded": has_details.then(|| if open() { "true" } else { "false" }),
            class: "flex gap-3 text-sm {frame} {class}",
            ..attributes,
            Svg {
                icon: if warning { CIRCLE_ALERT } else { CIRCLE_X },
                part: "icon",
                class: if warning { "mt-0.5 size-4 shrink-0 text-warning" } else { "mt-0.5 size-4 shrink-0 text-danger" },
            }
            div { class: "min-w-0 flex-1",
                if let Some(title) = &title {
                    p { "data-part": "title", class: "font-medium text-fg", "{title}" }
                }
                p {
                    "data-part": "summary",
                    class: match (title.is_some(), warning) {
                        (true, _) => "mt-1 break-words text-fg-muted",
                        (false, true) => "break-words text-warning",
                        (false, false) => "break-words text-danger",
                    },
                    "{summary}"
                }
                if !hints.is_empty() {
                    ul { "data-part": "hints", class: "mt-2 list-disc space-y-0.5 pl-5 text-fg-muted",
                        for hint in hints.iter() {
                            li { key: "{hint}", "{hint}" }
                        }
                    }
                }
                if has_actions || has_details {
                    div { "data-part": "actions", class: "mt-3 flex flex-wrap items-center gap-2",
                        if let Some(actions) = actions {
                            {actions}
                        }
                        if has_details {
                            button {
                                r#type: "button",
                                "data-part": "details-toggle",
                                "aria-expanded": if open() { "true" } else { "false" },
                                "aria-controls": "{details_id}",
                                "data-hovered": interaction.hovered(),
                                class: "rounded-control px-2 py-1 text-xs text-fg-muted underline-offset-2 data-focus-visible:focus-ring data-hovered:text-fg data-hovered:underline",
                                onmouseenter: move |_| interaction.enter(),
                                onmouseleave: move |_| interaction.leave(),
                                onclick: move |_| toggle(),
                                onkeydown: move |event| {
                                    interaction.key_down(&event, Keys::EnterAndSpace, toggle);
                                },
                                onkeyup: move |event| interaction.key_up(&event, toggle),
                                if open() {
                                    {t!(note = "Hides an error's technical details", "Hide details")}
                                } else {
                                    {t!(note = "Shows an error's technical details", "Show details")}
                                }
                            }
                        }
                    }
                }
                if has_details && open() {
                    div {
                        id: "{details_id}",
                        "data-part": "details",
                        class: "mt-2 rounded-control border border-border bg-surface-base/60 p-3 font-mono text-xs text-fg-muted",
                        if let Some(detail) = &detail {
                            p { class: "break-words whitespace-pre-wrap select-text", "{detail}" }
                        }
                        if let Some(reference) = &reference {
                            p {
                                "data-part": "reference",
                                class: if detail.is_some() { "mt-2 text-fg-subtle" } else { "text-fg-subtle" },
                                {t!(note = "The id an error's details are logged under", "Reference: {reference}", reference = reference)}
                            }
                        }
                    }
                }
            }
        }
    }
}
