//! Fields several flows share, as the webview's `gameActions/ActionForm`:
//! a path typed or picked ([`PathField`]), and a list of values each in a
//! field of its own ([`ListField`]). Lifecycle scripts (#105) and the game
//! editor (#100) use them.

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::paths::{PathRequest, use_path_picker};
use crate::ui::icons::{FOLDER_OPEN, PLUS, X};
use crate::ui::{Button, TextField};

/// A path, typed or chosen with Browse… (the OS's dialog, or console
/// mode's browser), which starts where the path is.
#[component]
pub fn PathField(
    label: String,
    #[props(default)] description: Option<String>,
    value: String,
    on_change: EventHandler<String>,
    request: PathRequest,
    #[props(default)] disabled: bool,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let picker = use_path_picker();
    let browse = {
        let value = value.clone();
        move |_| {
            let mut request = request.clone();
            if !value.is_empty() {
                request.start_at = Some(value.clone());
            }
            spawn(async move {
                if let Some(path) = picker.pick(request).await {
                    on_change.call(path);
                }
            });
        }
    };
    rsx! {
        div { "data-part": "path",
            TextField {
                label,
                description,
                value,
                on_input: move |text| on_change.call(text),
                disabled,
                mono: true,
                attributes,
                action: rsx! {
                    Button {
                        "data-spatial": "focusable",
                        icon: FOLDER_OPEN,
                        disabled,
                        on_press: browse,
                        {t!("Browse…")}
                    }
                },
            }
        }
    }
}

/// Values in fields of their own, each removable, and one more added.
#[component]
pub fn ListField(
    label: String,
    values: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    add_label: String,
    #[props(default)] disabled: bool,
) -> Element {
    rsx! {
        fieldset { "data-part": "list", class: "flex flex-col gap-2",
            legend { class: "mb-1.5 text-sm font-medium", "{label}" }
            // Positions are the identity: the list has no other.
            for (index, value) in values.iter().cloned().enumerate() {
                div { key: "{index}", class: "flex items-end gap-2",
                    TextField {
                        "aria-label": t!("{label} {0}", label = label.clone(), 0 = index + 1),
                        value,
                        on_input: {
                            let values = values.clone();
                            move |next: String| {
                                let mut values = values.clone();
                                values[index] = next;
                                on_change.call(values);
                            }
                        },
                        disabled,
                        mono: true,
                        class: "flex-1",
                    }
                    Button {
                        "data-spatial": "focusable",
                        "data-part": "remove",
                        "aria-label": t!("Remove {label} {0}", label = label.clone(), 0 = index + 1),
                        icon: X,
                        disabled,
                        on_press: {
                            let values = values.clone();
                            move |_| {
                                let mut values = values.clone();
                                values.remove(index);
                                on_change.call(values);
                            }
                        },
                    }
                }
            }
            Button {
                "data-spatial": "focusable",
                icon: PLUS,
                disabled,
                class: "self-start",
                on_press: {
                    let values = values.clone();
                    move |_| {
                        let mut values = values.clone();
                        values.push(String::new());
                        on_change.call(values);
                    }
                },
                "{add_label}"
            }
        }
    }
}
