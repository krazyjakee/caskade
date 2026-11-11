//! A settings section that syncs, kept on this device instead (#244):
//! `core.sync.sections`, as the webview's `SectionSync`. Shown in the
//! section itself once this device has paired devices; only an admin
//! changes it, as with what syncs.

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;
use serde_json::Value;

use super::state::{SettingsStatus, use_settings_section};
use crate::data::status::use_devices;
use crate::data::use_settings_query;
use crate::ui::{SettingRow, Switch};

/// The sync category a section belongs to.
fn category(namespace: &str) -> Option<&'static str> {
    match namespace {
        "core.appearance" => Some("appearance"),
        "core.accounts" => Some("accounts"),
        "core.romm" => Some("rommServers"),
        _ => None,
    }
}

/// `note` follows the description while the section syncs: what stays on
/// each device anyway.
#[component]
pub fn SectionSync(namespace: &'static str, #[props(default)] note: Option<String>) -> Element {
    let devices = use_devices();
    let paired = matches!(&*devices.read(), Some(Ok(Some(status))) if !status.devices.is_empty());
    // Without devices there's nothing to choose, and nothing else is asked
    // for.
    if !paired {
        return rsx! {};
    }
    rsx! {
        Choice { namespace, note }
    }
}

#[component]
fn Choice(namespace: &'static str, note: Option<String>) -> Element {
    let sync = use_settings_section("core.sync");
    let editable = use_settings_query(
        caskade_ui_data::Key::Settings(String::new()),
        |settings| async move { settings.editable().await },
    );
    // An admin, or a copy without accounts, may change it.
    let admin = matches!(&*editable.read(), Some(Ok(caskade_ui_data::Editable::All)));
    let what = t!("Same on your other devices");
    let Some(value) = sync.value() else {
        return rsx! {
            SettingsStatus { section: sync, what }
        };
    };
    let Some(category) = category(namespace) else {
        return rsx! {};
    };
    let category_on = value
        .pointer(&format!("/categories/{category}"))
        .and_then(Value::as_str)
        .unwrap_or("sync")
        == "sync";
    let kept = value
        .get("sections")
        .and_then(|sections| sections.get(namespace))
        .and_then(Value::as_str)
        == Some("thisDeviceOnly");
    let description = if !category_on {
        t!("Not synced: this kind of setting is This device only in Settings → Devices.")
    } else if kept {
        t!("Kept on this device. Changes here stay here, and other devices' don't arrive.")
    } else {
        [Some(t!("Changes here reach your other devices.")), note]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
    };
    rsx! {
        SettingRow { label: what.clone(), description,
            Switch {
                checked: category_on && !kept,
                disabled: !admin || !category_on || sync.saving(),
                on_change: move |on: bool| {
                    let mut sections = value
                        .get("sections")
                        .cloned()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| Value::Object(Default::default()));
                    sections[namespace] =
                        Value::String(if on { "sync" } else { "thisDeviceOnly" }.into());
                    sync.set_field("sections", sections);
                },
            }
        }
        SettingsStatus { section: sync, what }
    }
}
