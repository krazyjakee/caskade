//! A game's install status, as `@caskade/ui`'s `StatusBadge`.

use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{CIRCLE_CHECK, CIRCLE_SLASH, GAMEPAD, SHOPPING_CART, Svg};

pub const ANATOMY: Entry = Entry {
    parts: &[("icon", "The status icon."), ("label", "The status text.")],
    states: &[(
        "data-status",
        &["installed", "not-installed", "available", "emulated"],
    )],
    ..Entry::new(
        "status-badge",
        "StatusBadge (apps/native/src/ui/status_badge.rs)",
        Source::Ui,
        "A game's install status.",
    )
};

/// A game's install state, as `data-status` on cards and badges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameStatus {
    Installed,
    #[default]
    NotInstalled,
    Available,
    Emulated,
}

impl GameStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::NotInstalled => "not-installed",
            Self::Available => "available",
            Self::Emulated => "emulated",
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Installed => t!("Installed"),
            Self::NotInstalled => t!("Not Installed"),
            Self::Available => t!(
                note = "A game that can be bought in a store",
                "Available to Buy"
            ),
            Self::Emulated => t!(note = "A game played through an emulator", "Emulator"),
        }
    }
}

#[component]
pub fn StatusBadge(
    status: GameStatus,
    /// The `data-part` it plays in a parent component.
    #[props(default)]
    part: Option<&'static str>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let (icon, class) = match status {
        GameStatus::Installed => (CIRCLE_CHECK, "bg-status-installed/85"),
        GameStatus::NotInstalled => (CIRCLE_SLASH, "bg-status-missing/85"),
        GameStatus::Available => (SHOPPING_CART, "bg-status-available/90"),
        GameStatus::Emulated => (GAMEPAD, "bg-status-emulated/90"),
    };
    rsx! {
        span {
            "data-cs": "status-badge",
            "data-part": part,
            "data-status": status.name(),
            class: "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium text-white backdrop-blur {class}",
            ..attributes,
            Svg { icon, part: "icon", class: "size-3" }
            span { "data-part": "label", {status.label()} }
        }
    }
}
