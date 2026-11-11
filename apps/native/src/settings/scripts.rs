//! Settings → Scripts (#180, #105), as the webview's `ScriptsSettings`:
//! the global lifecycle scripts, which run with every game's launches.
//! Admins only, as the core requires; the panel is `flows::scripts`'s,
//! which an emulator profile and the game editor (#100) share.

use caskade_ui_data::scripts::ScriptScope;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::flows::scripts::ScriptsPanel;
use crate::ui::SettingsSection;
use crate::ui::icons::SQUARE_TERMINAL;

#[component]
pub fn ScriptsSettings() -> Element {
    rsx! {
        div { "data-settings": "scripts", class: "flex flex-col gap-4",
            SettingsSection {
                icon: SQUARE_TERMINAL,
                title: t!("Global scripts"),
                description: t!("Commands that run around every game's launch. Emulator profiles and games can add their own; see the order at launch below."),
                ScriptsPanel { scope: ScriptScope::Global }
            }
        }
    }
}
