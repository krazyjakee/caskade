//! A settings page's Reset to defaults (#44; the webview's
//! `ResetDefaults.tsx`): it names the group it resets, lists the settings
//! that would change (or says it can't tell, for a section the core has no
//! defaults for), and changes nothing until confirmed. Only the fields a
//! page lists go back to their defaults, so a page showing one setting of
//! another section resets that one and leaves the rest of the section
//! alone.

use caskade_ui_data::ErrorReport;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;
use serde_json::{Map, Value};

use super::state::use_settings_sections;
use crate::data::{change, use_settings};
use crate::ui::{Button, ButtonSize, ButtonVariant, Dialog, FailureNotice, NoticeVariant};

/// A setting a page shows, by key, as its row names it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetField {
    pub key: String,
    pub label: String,
}

/// What a page shows of one section.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetPart {
    pub namespace: String,
    /// The settings this page shows from the section; only these reset.
    pub fields: Vec<ResetField>,
    /// The page shows the whole section: the core resets all of it.
    pub whole: bool,
}

#[component]
pub fn ResetDefaults(
    /// The group's name, e.g. "General".
    group: String,
    /// The button's accessible name, e.g. "Reset general settings".
    label: String,
    parts: Vec<ResetPart>,
    /// What the reset leaves alone, where more than other pages' settings.
    #[props(default)]
    keeps: Option<String>,
    #[props(default)] disabled: bool,
) -> Element {
    let mut open = use_signal(|| false);
    rsx! {
        Button {
            variant: ButtonVariant::Ghost,
            size: ButtonSize::Sm,
            disabled,
            on_press: move |_| open.set(true),
            "aria-label": "{label}",
            {t!("Reset to defaults…")}
        }
        if open() {
            ResetDialog { group, parts, keeps, on_close: move |_| open.set(false) }
        }
    }
}

/// What resetting one part changes.
#[derive(Clone, PartialEq)]
struct Change {
    part: ResetPart,
    fields: Vec<ResetField>,
    defaults: Map<String, Value>,
}

/// Equal for a reset; a missing value is the default.
fn same(current: Option<&Value>, default: Option<&Value>) -> bool {
    current.is_none() || current == default
}

/// What each part would change, from the sections' defaults and values.
fn changes(
    parts: &[ResetPart],
    defaults: &dyn Fn(&str) -> Option<Map<String, Value>>,
    values: &[Option<Value>],
) -> Vec<Change> {
    parts
        .iter()
        .zip(values)
        .filter_map(|(part, value)| {
            let Some(defaults) = defaults(&part.namespace) else {
                // A whole section can still be reset by the core, though
                // nothing says what that changes; a field of another can't.
                return part.whole.then(|| Change {
                    part: part.clone(),
                    fields: Vec::new(),
                    defaults: Map::new(),
                });
            };
            let value = value.as_ref()?;
            let fields: Vec<ResetField> = part
                .fields
                .iter()
                .filter(|field| !same(value.get(&field.key), defaults.get(&field.key)))
                .cloned()
                .collect();
            (!fields.is_empty()).then(|| Change {
                part: part.clone(),
                fields,
                defaults,
            })
        })
        .collect()
}

#[component]
fn ResetDialog(
    group: String,
    parts: Vec<ResetPart>,
    keeps: Option<String>,
    on_close: EventHandler<()>,
) -> Element {
    let sections = use_settings_sections();
    let settings = use_settings();
    let namespaces: Vec<String> = parts.iter().map(|part| part.namespace.clone()).collect();
    let mut values = use_resource({
        let settings = settings.clone();
        move || {
            let settings = settings.clone();
            let namespaces = namespaces.clone();
            change(async move {
                let mut values = Vec::new();
                for namespace in namespaces {
                    values.push(Some(settings.get(&namespace).await?.value));
                }
                Ok::<_, ErrorReport>(values)
            })
        }
    });
    let mut resetting = use_signal(|| false);
    let mut failed = use_signal(|| None::<ErrorReport>);
    let defaults_list = match &*sections.read() {
        Some(Ok(list)) => Some(list.clone()),
        _ => None,
    };
    let failure = match (&*sections.read(), &*values.read()) {
        (Some(Err(error)), _) | (_, Some(Err(error))) => Some(error.clone()),
        _ => None,
    };
    let loading = failure.is_none() && (defaults_list.is_none() || values.read().is_none());
    let defaults_of = {
        let list = defaults_list.clone().unwrap_or_default();
        move |namespace: &str| {
            list.iter()
                .find(|section| section.namespace == namespace)
                .and_then(|section| section.defaults.as_object().cloned())
        }
    };
    let current: Vec<Option<Value>> = match &*values.read() {
        Some(Ok(values)) => values.clone(),
        _ => vec![None; parts.len()],
    };
    let found = if loading || failure.is_some() {
        Vec::new()
    } else {
        changes(&parts, &defaults_of, &current)
    };
    let labels: Vec<String> = found
        .iter()
        .flat_map(|change| change.fields.iter().map(|field| field.label.clone()))
        .collect();
    let unseen: Vec<&ResetPart> = parts
        .iter()
        .filter(|part| defaults_of(&part.namespace).is_none())
        .collect();
    let resets_unseen = !loading && unseen.iter().any(|part| part.whole);
    let leaves_unseen = !loading && unseen.iter().any(|part| !part.whole);
    let can_reset = !loading && failure.is_none() && !found.is_empty();
    let reset = {
        let found = found.clone();
        move |_| {
            let found = found.clone();
            let settings = settings.clone();
            resetting.set(true);
            failed.set(None);
            spawn(async move {
                let result = change(async move {
                    for Change {
                        part,
                        fields,
                        defaults,
                    } in found
                    {
                        if part.whole {
                            settings.reset(&part.namespace).await?;
                        } else {
                            settings
                                .update(&part.namespace, |mut value| {
                                    if let Some(object) = value.as_object_mut() {
                                        for field in &fields {
                                            if let Some(default) = defaults.get(&field.key) {
                                                object.insert(field.key.clone(), default.clone());
                                            }
                                        }
                                    }
                                    value
                                })
                                .await?;
                        }
                    }
                    Ok::<_, ErrorReport>(())
                })
                .await;
                resetting.set(false);
                match result {
                    Ok(()) => on_close.call(()),
                    Err(error) => failed.set(Some(error)),
                }
            });
        }
    };
    let pending = resetting();
    rsx! {
        Dialog {
            title: t!("Reset {group} settings?", group = group.clone()),
            alert: true,
            on_close: move |_| {
                if !*resetting.peek() {
                    on_close.call(());
                }
            },
            div { class: "flex flex-col gap-4",
                if loading {
                    p { role: "status", class: "text-sm text-fg-muted", {t!("Checking what would change…")} }
                } else if let Some(error) = failure {
                    FailureNotice {
                        error,
                        variant: NoticeVariant::Inline,
                        title: t!("Caskade couldn't check what would change"),
                        on_retry: move |_| {
                            let mut sections = sections;
                            sections.restart();
                            values.restart();
                        },
                    }
                } else {
                    if !labels.is_empty() {
                        div { class: "flex flex-col gap-2 text-sm",
                            p { class: "text-fg-muted", {t!("These settings go back to their defaults:")} }
                            ul { class: "list-disc pl-5",
                                for label in labels.iter() {
                                    li { key: "{label}", "{label}" }
                                }
                            }
                        }
                    }
                    if resets_unseen {
                        p { class: "text-sm text-fg-muted",
                            if labels.is_empty() {
                                {t!("Caskade can't tell what would change. Reset puts all of these settings back to their defaults.")}
                            } else {
                                {t!("Caskade can't tell what else would change. Reset also puts the rest of these settings back to their defaults.")}
                            }
                        }
                    }
                    if leaves_unseen {
                        p { class: "text-sm text-fg-muted",
                            if found.is_empty() {
                                {t!("Caskade can't tell what would change, so these settings can't be reset here.")}
                            } else {
                                {t!("Caskade can't tell what would change for some settings here, so those stay as they are.")}
                            }
                        }
                    }
                    if labels.is_empty() && unseen.is_empty() {
                        p { class: "text-sm text-fg-muted", {t!("Every setting here already uses its default.")} }
                    }
                }
                p { class: "text-sm text-fg-muted",
                    {keeps.clone().unwrap_or_else(|| t!("Settings on other pages stay as they are."))}
                }
                if let Some(error) = failed() {
                    FailureNotice { error, variant: NoticeVariant::Inline }
                }
                div { class: "flex justify-end gap-2",
                    Button {
                        disabled: pending,
                        on_press: move |_| on_close.call(()),
                        "data-focus-default": "true",
                        if can_reset || loading { {t!("Cancel")} } else { {t!("Close")} }
                    }
                    // Offered once it's known what it does: a greyed-out
                    // Reset gives no reason while the check runs.
                    if can_reset {
                        Button {
                            variant: ButtonVariant::Danger,
                            disabled: pending,
                            on_press: reset,
                            if pending { {t!("Resetting…")} } else { {t!("Reset")} }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn part(whole: bool) -> ResetPart {
        ResetPart {
            namespace: "core.x".into(),
            whole,
            fields: vec![
                ResetField {
                    key: "a".into(),
                    label: "A".into(),
                },
                ResetField {
                    key: "b".into(),
                    label: "B".into(),
                },
            ],
        }
    }

    #[test]
    fn a_reset_lists_only_what_differs_and_a_section_without_defaults_resets_whole() {
        let defaults = |_: &str| json!({"a": 1, "b": 2}).as_object().cloned();
        let found = changes(&[part(false)], &defaults, &[Some(json!({"a": 1, "b": 3}))]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].fields,
            vec![ResetField {
                key: "b".into(),
                label: "B".into()
            }]
        );
        // Nothing differs: nothing to reset.
        assert!(changes(&[part(false)], &defaults, &[Some(json!({"a": 1}))]).is_empty());
        // No defaults: a whole section still resets, a field of one can't.
        let none = |_: &str| None;
        assert_eq!(changes(&[part(true)], &none, &[None]).len(), 1);
        assert!(changes(&[part(false)], &none, &[None]).is_empty());
    }
}
