//! A settings section rendered from its JSON Schema (#59, #128; the
//! webview's `SchemaSettings.tsx`): controls, choices, labels and limits
//! come from the core, which validates every save and translates the
//! schema's titles and descriptions (#214).

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;
use serde_json::{Map, Value};

use super::drafts::{DraftField, DraftSlider};
use super::reset::{ResetDefaults, ResetField, ResetPart};
use super::state::{
    SettingsFeedback, SettingsLoading, use_settings_definition, use_settings_section,
};
use crate::ui::{Icon, Select, SelectOption, SettingRow, SettingsSection, Switch};

type Schema = Map<String, Value>;

fn object(value: &Value) -> Schema {
    value.as_object().cloned().unwrap_or_default()
}

/// `value` with its `$ref` (to `#/$defs/…`) followed; at most eight deep.
pub fn resolve(value: &Value, root: &Schema, depth: usize) -> Result<Schema, String> {
    let schema = object(value);
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return Ok(schema);
    };
    let Some(name) = reference.strip_prefix("#/$defs/").filter(|_| depth < 8) else {
        return Err(t!("Unsupported settings schema reference."));
    };
    let defs = root.get("$defs").map(object).unwrap_or_default();
    let mut resolved = resolve(defs.get(name).unwrap_or(&Value::Null), root, depth + 1)?;
    // The reference's own keywords (its title, say) win.
    for (key, value) in schema {
        resolved.insert(key, value);
    }
    Ok(resolved)
}

/// A key as a label, where the schema has no title: `max_jobs` → "Max jobs".
pub fn label(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A field's choices: its `enum`, or the constants of its `oneOf`/`anyOf`.
pub fn options(schema: &Schema, root: &Schema) -> Vec<SelectOption> {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .filter_map(Value::as_str)
            .map(|value| SelectOption::new(value, label(value)))
            .collect();
    }
    let variants = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    variants
        .iter()
        .filter_map(|variant| {
            let item = resolve(variant, root, 0).ok()?;
            let id = item.get("const").and_then(Value::as_str).or_else(|| {
                item.get("enum")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 1)
                    .and_then(|values| values[0].as_str())
            })?;
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .map_or_else(|| label(id), str::to_owned);
            Some(SelectOption::new(id, title))
        })
        .collect()
}

/// A field's title as its row shows it.
pub fn title(key: &str, schema: &Schema) -> String {
    schema
        .get("title")
        .and_then(Value::as_str)
        .map_or_else(|| label(key), str::to_owned)
}

/// The section's fields, `$ref`s followed, in the schema's order.
pub fn fields(schema: &Value) -> Result<Vec<(String, Schema)>, String> {
    let root = object(schema);
    let properties = root.get("properties").map(object).unwrap_or_default();
    properties
        .iter()
        .map(|(key, value)| Ok((key.clone(), resolve(value, &root, 0)?)))
        .collect()
}

/// A share from 0 to 1, as a percentage in the language in use.
fn percent(share: f64) -> String {
    caskade_ui_i18n::format::percent(Some(share * 100.0))
}

#[component]
pub fn SchemaSettings(
    namespace: String,
    title: String,
    #[props(default)] description: Option<String>,
    #[props(default)] icon: Option<Icon>,
    /// The Reset button's name; "Reset {section} settings" by default.
    #[props(default)]
    reset_label: Option<String>,
) -> Element {
    let definition = use_settings_definition(&namespace);
    let section = use_settings_section(&namespace);
    let (Some(settings), Some(Ok(definition))) = (section.value(), definition.read().clone())
    else {
        let error = section.load_error().or_else(|| match &*definition.read() {
            Some(Err(error)) => Some(error.clone()),
            _ => None,
        });
        return rsx! {
            SettingsLoading {
                error,
                on_reload: move |_| {
                    section.reload();
                    let mut definition = definition;
                    definition.restart();
                },
            }
        };
    };
    let root = object(&definition.schema);
    let fields = match fields(&definition.schema) {
        Ok(fields) => fields,
        Err(problem) => {
            return rsx! {
                p { role: "alert", "{problem}" }
            };
        }
    };
    let reset = ResetPart {
        namespace: namespace.clone(),
        whole: true,
        fields: fields
            .iter()
            .map(|(key, schema)| ResetField {
                key: key.clone(),
                label: self::title(key, schema),
            })
            .collect(),
    };
    let reset_label =
        reset_label.unwrap_or_else(|| t!("Reset {section} settings", section = title.clone()));
    rsx! {
        div { "data-settings": "{namespace}", "data-cs": "schema-settings", class: "flex flex-col gap-4",
            SettingsFeedback { error: section.error(), saving: section.saving() }
            SettingsSection {
                icon,
                title: title.clone(),
                description,
                // In the card it resets, so two on one page can't be confused (#54).
                aside: rsx! {
                    ResetDefaults {
                        group: title.clone(),
                        label: reset_label,
                        parts: vec![reset],
                        disabled: section.saving(),
                    }
                },
                for (key, schema) in fields {
                    Field { key: "{key}", name: key.clone(), schema, root: root.clone(), value: settings.get(&key).cloned().unwrap_or(Value::Null), section }
                }
            }
        }
    }
}

#[component]
fn Field(
    name: String,
    schema: Schema,
    root: Schema,
    value: Value,
    section: super::state::SectionState,
) -> Element {
    let row_title = title(&name, &schema);
    let description = schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let choices = options(&schema, &root);
    let kind = schema.get("type").and_then(Value::as_str).unwrap_or("");
    let numeric = matches!(kind, "number" | "integer");
    let minimum = schema.get("minimum").and_then(Value::as_f64);
    let maximum = schema.get("maximum").and_then(Value::as_f64);
    let key = name.clone();
    let set = move |next: Value| section.set_field(&key, next);
    let control = if !choices.is_empty()
        && let Some(current) = value.as_str()
    {
        let set = set.clone();
        rsx! {
            Select {
                options: choices,
                value: Some(current.to_owned()),
                on_change: move |next: String| set(Value::String(next)),
                class: "w-48",
            }
        }
    } else if kind == "boolean"
        && let Some(on) = value.as_bool()
    {
        let set = set.clone();
        rsx! {
            Switch { checked: on, on_change: move |next: bool| set(Value::Bool(next)) }
        }
    } else if numeric
        && let (Some(current), Some(minimum), Some(maximum)) = (value.as_f64(), minimum, maximum)
    {
        let integer = kind == "integer";
        let set = set.clone();
        // A share from 0 to 1 reads as a percentage (#53).
        let share = !integer && minimum == 0.0 && maximum == 1.0;
        rsx! {
            DraftSlider {
                value: current,
                min: minimum,
                max: maximum,
                step: if integer { 1.0 } else { 0.05 },
                format: share.then(|| Callback::new(percent)),
                commit: move |next: f64| set(number(next, integer)),
                class: "w-48",
            }
        }
    } else if numeric && let Some(current) = value.as_f64() {
        let integer = kind == "integer";
        let set = set.clone();
        rsx! {
            DraftField {
                value: shown(current, integer),
                // What isn't a number isn't saved, and says so.
                validate: Callback::new(|next: String| {
                    (next.trim().is_empty() || next.trim().parse::<f64>().is_err())
                        .then(|| t!("Enter a number."))
                }),
                commit: move |next: String| {
                    if let Ok(parsed) = next.trim().parse::<f64>()
                        && parsed.is_finite()
                    {
                        set(number(parsed, integer));
                    }
                },
                class: "w-48",
            }
        }
    } else if kind == "string"
        && let Some(current) = value.as_str()
    {
        let set = set.clone();
        rsx! {
            DraftField {
                value: current.to_owned(),
                commit: move |next: String| set(Value::String(next)),
                class: "w-48",
            }
        }
    } else {
        rsx! {
            span { role: "alert", class: "text-sm text-danger", {t!("Unsupported setting type.")} }
        }
    };
    rsx! {
        SettingRow { label: row_title, description, {control} }
    }
}

/// A number as the schema's type has it.
fn number(value: f64, integer: bool) -> Value {
    if integer {
        Value::from(value.round() as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// A number as its field shows it: no `.0` on a whole one.
fn shown(value: f64, integer: bool) -> String {
    if integer || value.fract() == 0.0 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value}")
    }
}
