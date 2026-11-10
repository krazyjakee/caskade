//! Settings layout and small labels, as `@caskade/ui`'s
//! `form/SettingsSection.tsx`: a titled group of settings
//! ([`SettingsSection`]), one setting with its controls ([`SettingRow`]),
//! a coloured dot with a label ([`StatusDot`]) and a bordered chip
//! ([`Pill`]). A row names and describes its controls, as react-aria's
//! contexts do in the webview: a switch, select, text field, slider or
//! segmented control in it without a label of its own takes the row's.

use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{Icon, Svg};
use super::ids::use_id;

pub const SECTION_ANATOMY: Entry = Entry {
    parts: &[
        ("header", "The heading row."),
        ("icon", "The heading icon."),
        ("title", "The heading."),
        ("description", "The text under the heading."),
        ("aside", "Badges or actions at the end of the heading row."),
    ],
    contains: &["setting-row"],
    slots: &["aside"],
    container: true,
    ..Entry::new(
        "settings-section",
        "SettingsSection (apps/native/src/ui/settings.rs)",
        Source::Ui,
        "A titled group of settings.",
    )
};

pub const ROW_ANATOMY: Entry = Entry {
    parts: &[
        ("icon", "The row icon."),
        ("label", "The setting's name."),
        ("description", "The text under the name."),
        ("controls", "The controls."),
    ],
    slots: &["controls"],
    ..Entry::new(
        "setting-row",
        "SettingRow (apps/native/src/ui/settings.rs)",
        Source::Ui,
        "One setting: its label on the left, its controls on the right.",
    )
};

pub const DOT_ANATOMY: Entry = Entry {
    parts: &[("dot", "The dot."), ("label", "The text.")],
    states: &[("data-tone", &["ok", "off", "warning", "danger"])],
    ..Entry::new(
        "status-dot",
        "StatusDot (apps/native/src/ui/settings.rs)",
        Source::Ui,
        "A small coloured dot and label, e.g. Detected / Not found.",
    )
};

pub const PILL_ANATOMY: Entry = Entry {
    parts: &[("icon", "The icon."), ("label", "The text.")],
    states: &[("data-tone", &["warning", "danger", "accent"])],
    ..Entry::new(
        "pill",
        "Pill (apps/native/src/ui/settings.rs)",
        Source::Ui,
        "A bordered chip with an icon, e.g. Off by default.",
    )
};

/// A titled group of settings: a heading (with an icon, a description and
/// badges or actions at its end) over its rows.
#[component]
pub fn SettingsSection(
    title: String,
    #[props(default)] icon: Option<Icon>,
    #[props(default)] description: Option<String>,
    /// Badges or actions at the end of the heading row.
    #[props(default)]
    aside: Option<Element>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let heading = use_id("settings-section");
    rsx! {
        section {
            "data-cs": "settings-section",
            "aria-labelledby": "{heading}",
            class: "rounded-panel border border-border bg-surface-raised p-5 {class}",
            ..attributes,
            header { "data-part": "header", class: "mb-2 flex items-start gap-3",
                if let Some(icon) = icon {
                    Svg { icon, part: "icon", class: "mt-0.5 size-5 shrink-0 text-accent-alt" }
                }
                div { class: "min-w-0 flex-1",
                    h2 {
                        id: "{heading}",
                        "data-part": "title",
                        class: "font-display text-lg font-semibold tracking-wide",
                        "{title}"
                    }
                    if let Some(description) = &description {
                        p { "data-part": "description", class: "text-sm text-fg-muted", "{description}" }
                    }
                }
                if let Some(aside) = aside {
                    div { "data-part": "aside", class: "flex shrink-0 items-center gap-3", {aside} }
                }
            }
            {children}
        }
    }
}

/// What a [`SettingRow`] names and describes its controls with.
#[derive(Clone, Debug, PartialEq)]
pub struct RowLabel {
    /// The setting's name, for a control that needs an `aria-label` (a
    /// slider's thumb).
    pub label: String,
    pub label_id: String,
    pub description_id: Option<String>,
}

/// The row a control is in, if it's in one.
pub fn use_row_label() -> Option<RowLabel> {
    use_hook(try_consume_context::<RowLabel>)
}

/// One setting: its name and description, then its controls. The text
/// grows to a fixed width and no further, so on a wide panel the controls
/// stay beside their name; where the row is too narrow, they wrap under it
/// (#41).
#[component]
pub fn SettingRow(
    label: String,
    #[props(default)] icon: Option<Icon>,
    #[props(default)] description: Option<String>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let label_id = use_id("setting-label");
    let description_id = use_id("setting-description");
    let described = description.is_some();
    use_context_provider({
        let label = label.clone();
        let label_id = label_id.clone();
        let description_id = description_id.clone();
        move || RowLabel {
            label,
            label_id,
            description_id: described.then_some(description_id),
        }
    });
    rsx! {
        div {
            "data-cs": "setting-row",
            class: "flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-border py-3 first-of-type:border-t-0",
            ..attributes,
            // The icon stays with the text when the controls wrap; the basis
            // is the text's minimum width beside them, in rem.
            div { class: "flex max-w-md min-w-0 flex-[1_1_16rem] items-center gap-3",
                if let Some(icon) = icon {
                    Svg { icon, part: "icon", class: "size-5 shrink-0 text-fg-muted" }
                }
                div { class: "min-w-0 flex-1 break-words",
                    div { id: "{label_id}", "data-part": "label", class: "text-sm font-medium", "{label}" }
                    if let Some(description) = &description {
                        div { id: "{description_id}", "data-part": "description", class: "text-xs text-fg-muted", "{description}" }
                    }
                }
            }
            div { "data-part": "controls", class: "flex max-w-full min-w-0 flex-wrap items-center gap-3",
                {children}
            }
        }
    }
}

/// A [`StatusDot`]'s colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DotTone {
    #[default]
    Ok,
    Off,
    Warning,
    Danger,
}

impl DotTone {
    fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Off => "off",
            Self::Warning => "warning",
            Self::Danger => "danger",
        }
    }

    fn classes(self) -> &'static str {
        match self {
            Self::Ok => "bg-status-installed shadow-status-installed",
            Self::Off => "bg-status-missing shadow-transparent",
            Self::Warning => "bg-warning shadow-warning",
            Self::Danger => "bg-danger shadow-danger",
        }
    }
}

/// A small coloured dot and its label, e.g. "Detected" / "Not found".
#[component]
pub fn StatusDot(
    tone: DotTone,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        span {
            "data-cs": "status-dot",
            "data-tone": tone.name(),
            class: "inline-flex items-center gap-1.5 text-xs whitespace-nowrap text-fg-muted",
            ..attributes,
            span { "data-part": "dot", class: "size-1.5 rounded-full shadow-[0_0_6px] {tone.classes()}" }
            span { "data-part": "label", {children} }
        }
    }
}

/// A [`Pill`]'s colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PillTone {
    #[default]
    Warning,
    Danger,
    Accent,
}

impl PillTone {
    fn name(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Accent => "accent",
        }
    }

    fn classes(self) -> &'static str {
        match self {
            Self::Warning => "border-warning/40 bg-warning/10 text-warning",
            Self::Danger => "border-danger/40 bg-danger/10 text-danger",
            Self::Accent => "border-accent-alt/40 bg-accent-alt/10 text-accent-alt",
        }
    }
}

/// A bordered chip with an icon, e.g. "Off by default" / "Caution".
#[component]
pub fn Pill(
    tone: PillTone,
    icon: Icon,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    rsx! {
        span {
            "data-cs": "pill",
            "data-tone": tone.name(),
            class: "inline-flex shrink-0 items-center gap-1.5 rounded-control border px-2.5 py-1 text-xs font-medium whitespace-nowrap {tone.classes()}",
            ..attributes,
            Svg { icon, part: "icon", class: "size-3.5" }
            span { "data-part": "label", {children} }
        }
    }
}
