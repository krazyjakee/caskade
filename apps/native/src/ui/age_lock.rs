//! "Admin PIN needed" (#106, #209), as the webview's `AgeLockLabel`: beside
//! a game the signed-in account's age limit holds back, on library cards
//! and rows, in search and on the game page.

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::Svg;
use super::icons::LOCK;

#[component]
pub fn AgeLockLabel(
    /// Why, in full: "An admin PIN is needed to play Doom, rated PEGI 18".
    #[props(default)]
    title: Option<String>,
) -> Element {
    rsx! {
        span {
            "data-age-lock": "true",
            title,
            class: "inline-flex shrink-0 items-center gap-1 text-xs text-fg-muted",
            Svg { icon: LOCK, class: "size-3.5" }
            {t!("Admin PIN needed")}
        }
    }
}
