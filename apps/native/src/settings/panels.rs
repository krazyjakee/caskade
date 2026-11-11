//! Each section's panel: General and Appearance (#104, `general.rs`,
//! `appearance.rs`); Installer's schema
//! (`core.installer`), as the webview's `InstallerSettings`. The others
//! come with #104 and #105.

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::schema::SchemaSettings;
use crate::ui::icons::PACKAGE_OPEN;

/// The panel of section `tab` (at sub-section `sub`).
pub fn panel(tab: &'static str, sub: Option<String>) -> Element {
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
                super::torrent::TorrentLinksSettings {}
            }
        },
        "storage" => rsx! {
            super::storage::StorageSettings {}
        },
        "library-discovery" => rsx! {
            super::discovery::LibraryDiscoverySettings {}
        },
        "plugins" => rsx! {
            super::plugins::PluginsSettings {}
        },
        "stores" => rsx! {
            super::stores::StoreAccountsSettings {}
        },
        "launchers" => rsx! {
            super::launchers::LaunchersSettings {}
        },
        "appearance" => rsx! {
            super::appearance::AppearanceSettings {}
        },
        "general" => rsx! {
            super::general::GeneralSettings { sub }
        },
        "streaming" => rsx! {
            super::streaming::StreamingSettings {}
        },
        "scripts" => rsx! {
            super::scripts::ScriptsSettings {}
        },
        "about" => rsx! {
            super::about::AboutSettings { section: sub }
        },
        "statistics" => rsx! {
            super::statistics::StatisticsSettings {}
        },
        "devices" => rsx! {
            super::devices::DevicesSettings {}
        },
        "backup" => rsx! {
            super::backup::BackupSettings {}
        },
        "accounts" => rsx! {
            super::accounts::AccountsSettings {}
        },
        "profile" => rsx! {
            super::accounts::ProfileSettings {}
        },
        "emulators" => rsx! {
            super::emulation::EmulationSettings { section: sub }
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
