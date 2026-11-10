//! Each section's panel. The schema-driven ones render from the core's
//! schemas now: Installer (`core.installer`) and General's game updates
//! (`core.updates`), as the webview's `InstallerSettings` and
//! `UpdateSettings`. The others come with #104 and #105.

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::schema::SchemaSettings;
use crate::ui::icons::{PACKAGE_OPEN, REFRESH_CW};

/// The panel of section `tab` (at sub-section `sub`).
pub fn panel(tab: &'static str, sub: Option<String>) -> Element {
    let _ = sub;
    match tab {
        "installer" => rsx! {
            div { class: "flex flex-col gap-6",
                SchemaSettings {
                    namespace: "core.installer",
                    title: t!("Installer"),
                    icon: Some(PACKAGE_OPEN),
                    description: t!("Preferences apply to the next install job. Jobs already running keep their current settings."),
                    reset_label: t!("Reset installer settings"),
                }
                Pending {}
            }
        },
        "general" => rsx! {
            div { class: "flex flex-col gap-6",
                Pending {}
                // Game updates (`core.updates`), beside Caskade's own (#54).
                SchemaSettings {
                    namespace: "core.updates",
                    title: t!("Game updates"),
                    icon: Some(REFRESH_CW),
                    description: t!("Games installed from GitHub are checked for new releases without an account. Updating keeps your saves and settings."),
                    reset_label: t!("Reset update settings"),
                }
            }
        },
        _ => rsx! {
            Pending {}
        },
    }
}

/// What isn't built yet in a panel (#104, #105).
#[component]
fn Pending() -> Element {
    rsx! {
        div {
            "data-pending": "true",
            class: "rounded-panel border border-dashed border-border p-5",
            p { class: "text-sm text-fg-muted", {t!("Not built yet.")} }
        }
    }
}
