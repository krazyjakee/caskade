//! Progress, as react-aria's `ProgressBar`: a bar with its value
//! (`role="progressbar"`), or busy with no value yet (indeterminate). No
//! catalogued root: views place and name it (`data-part`).

use dioxus_native::prelude::*;

use super::ids::use_id;

#[component]
pub fn Progress(
    /// 0–1; `None` while it can't tell (indeterminate).
    value: Option<f64>,
    /// Its name, for screen readers and shown above the bar when given.
    #[props(default)]
    label: Option<String>,
    /// The readout, e.g. "1.2 GB of 4 GB".
    #[props(default)]
    value_text: Option<String>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let label_id = use_id("progress");
    let percent = value.map(|value| (value.clamp(0.0, 1.0) * 100.0).round());
    rsx! {
        div {
            role: "progressbar",
            "aria-labelledby": label.as_ref().map(|_| label_id.clone()),
            "aria-valuemin": "0",
            "aria-valuemax": "100",
            "aria-valuenow": percent.map(|percent| format!("{percent}")),
            "aria-valuetext": value_text.clone(),
            "aria-busy": value.is_none().then_some("true"),
            "data-indeterminate": value.is_none().then_some("true"),
            class: "flex flex-col gap-1 {class}",
            ..attributes,
            if label.is_some() || value_text.is_some() {
                div { class: "flex items-center justify-between gap-3 text-xs",
                    if let Some(label) = &label {
                        span { id: "{label_id}", "data-part": "label", class: "text-fg-muted", "{label}" }
                    }
                    if let Some(text) = &value_text {
                        span { "data-part": "value", class: "font-mono text-fg-subtle", "{text}" }
                    }
                }
            }
            div { "data-part": "track", class: "relative h-1.5 overflow-hidden rounded-full bg-surface-hover",
                match percent {
                    Some(percent) => rsx! {
                        div {
                            "data-part": "fill",
                            class: "h-full rounded-full bg-linear-to-r from-accent to-accent-alt transition-[width] duration-(--cs-motion-duration-base)",
                            style: "width: {percent}%;",
                        }
                    },
                    None => rsx! {
                        div { "data-part": "fill", class: "h-full w-1/3 rounded-full bg-accent-alt animate-progress" }
                    },
                }
            }
        }
    }
}
