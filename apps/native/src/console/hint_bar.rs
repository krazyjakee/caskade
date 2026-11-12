//! The button-hint bar (#107; the webview's `HintBar.tsx` and `glyphs.ts`):
//! what each button does where focus is now, in the glyphs of the
//! controller in hand (keys when no pad is in use), and the clock.

use caskade_ui_i18n::intl::Clock;
use caskade_ui_i18n::{format, t, tp};
use caskade_ui_input::{Action, GlyphFamily, InputSource};
use dioxus_native::prelude::*;

/// A pad family, or the keyboard when no pad is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Pad(GlyphFamily),
    Keyboard,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Pad(GlyphFamily::Xbox) => "xbox",
            Self::Pad(GlyphFamily::Playstation) => "playstation",
            Self::Pad(GlyphFamily::Nintendo) => "nintendo",
            Self::Pad(GlyphFamily::Generic) => "generic",
        }
    }
}

/// The family last used: the pad's, or the keyboard.
pub fn use_family() -> Family {
    let input = crate::input::use_input();
    match input.source() {
        Some(InputSource::Pad) => Family::Pad(input.glyphs()),
        _ => Family::Keyboard,
    }
}

/// The label of `action`'s button, or `None` when the family has none (the
/// keyboard has no guide key).
pub fn glyph(action: Action, family: Family) -> Option<String> {
    let family = match family {
        Family::Pad(family) => family,
        Family::Keyboard => {
            return Some(match action {
                Action::Confirm => tp!("keyboard key", "Enter"),
                Action::Back => tp!("keyboard key", "Esc"),
                Action::Details => "I".into(),
                Action::Search => "/".into(),
                Action::TabPrev => "[".into(),
                Action::TabNext => "]".into(),
                Action::PagePrev => tp!("keyboard key", "PgUp"),
                Action::PageNext => tp!("keyboard key", "PgDn"),
                Action::Menu => "M".into(),
                Action::View => "V".into(),
                _ => return None,
            });
        }
    };
    let face = |xbox: &str, playstation: &str, nintendo: &str| {
        match family {
            GlyphFamily::Xbox | GlyphFamily::Generic => xbox,
            GlyphFamily::Playstation => playstation,
            GlyphFamily::Nintendo => nintendo,
        }
        .to_owned()
    };
    let [lb, rb, lt, rt, menu, view, guide] = match family {
        GlyphFamily::Xbox => ["LB", "RB", "LT", "RT", "Menu", "View", "Guide"],
        GlyphFamily::Playstation => ["L1", "R1", "L2", "R2", "Options", "Create", "PS"],
        GlyphFamily::Nintendo => ["L", "R", "ZL", "ZR", "+", "−", "Home"],
        GlyphFamily::Generic => ["LB", "RB", "LT", "RT", "Start", "Select", "Guide"],
    };
    Some(match action {
        Action::Confirm => face("A", "✕", "A"),
        Action::Back => face("B", "○", "B"),
        Action::Details => face("X", "□", "Y"),
        Action::Search => face("Y", "△", "X"),
        Action::TabPrev => lb.into(),
        Action::TabNext => rb.into(),
        Action::PagePrev => lt.into(),
        Action::PageNext => rt.into(),
        Action::Menu => menu.into(),
        Action::View => view.into(),
        Action::Guide => guide.into(),
        _ => return None,
    })
}

/// Hints read left to right in this order, most useful first: a bar too
/// narrow for them all shows what fits from the left.
const ORDER: [Action; 11] = [
    Action::Confirm,
    Action::Back,
    Action::Details,
    Action::Search,
    Action::Menu,
    Action::TabPrev,
    Action::TabNext,
    Action::PagePrev,
    Action::PageNext,
    Action::View,
    Action::Guide,
];

/// The webview's action names, for `data-action`.
fn action_name(action: Action) -> &'static str {
    match action {
        Action::Confirm => "confirm",
        Action::Back => "back",
        Action::Details => "details",
        Action::Search => "search",
        Action::Menu => "menu",
        Action::TabPrev => "tab.prev",
        Action::TabNext => "tab.next",
        Action::PagePrev => "page.prev",
        Action::PageNext => "page.next",
        Action::View => "view",
        Action::Guide => "guide",
        _ => "",
    }
}

/// Face buttons are told apart by colour on most pads.
fn tone(action: Action) -> Option<&'static str> {
    match action {
        Action::Confirm => Some("installed"),
        Action::Back => Some("danger"),
        Action::Details => Some("accent"),
        Action::Search => Some("warning"),
        _ => None,
    }
}

/// Settings → General → Time format: 12 or 24 hours, or the language's own.
fn use_clock_setting() -> Clock {
    let general = crate::settings::use_settings_section("core.general");
    match general
        .value()
        .and_then(|value| {
            value
                .get("timeFormat")
                .and_then(|f| f.as_str().map(str::to_owned))
        })
        .as_deref()
    {
        Some("12h") => Clock::H12,
        Some("24h") => Clock::H23,
        _ => Clock::System,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// The time, ticking on the minute.
#[component]
fn ClockText() -> Element {
    let clock = use_clock_setting();
    let mut now = use_signal(now_ms);
    use_future(move || async move {
        loop {
            let wait = 60_000 - (now_ms().rem_euclid(60_000)) as u64;
            let _ = caskade_app::runtime::spawn(tokio::time::sleep(
                std::time::Duration::from_millis(wait),
            ))
            .await;
            now.set(now_ms());
        }
    });
    let text = format::time(crate::library::local_date_time(now()), clock, false);
    rsx! {
        span { "data-part": "clock", class: "font-mono text-sm text-fg-muted tabular-nums", "{text}" }
    }
}

/// The bar at the bottom of console mode's shell.
#[component]
pub fn HintBar() -> Element {
    let input = crate::input::use_input();
    let family = use_family();
    let active = input.hints();
    let hints: Vec<(Action, String, String)> = ORDER
        .iter()
        .filter_map(|&action| {
            let binding = active.iter().find(|binding| binding.action == action)?;
            let label = glyph(action, family)?;
            Some((action, label, binding.label.clone()))
        })
        .collect();
    rsx! {
        footer {
            "data-cs": "hint-bar",
            "data-glyphs": family.name(),
            class: "relative z-10 flex h-(--cs-hintbar-height) shrink-0 items-center gap-6 border-t border-border bg-surface-raised/90 px-8",
            ul {
                "aria-label": t!(
                    note = "The console hint bar's list of buttons and what they do",
                    "Controls"
                ),
                class: "flex h-7 min-w-0 flex-1 flex-wrap content-start items-center gap-x-6 overflow-hidden",
                for (action, label, text) in hints {
                    li {
                        key: "{action_name(action)}",
                        "data-part": "hint",
                        "data-action": action_name(action),
                        class: "flex shrink-0 items-center gap-2 text-sm",
                        kbd {
                            "data-part": "glyph",
                            "data-tone": tone(action),
                            class: "grid h-7 min-w-7 place-items-center rounded-full border-2 border-border-strong px-1.5 font-mono text-xs font-semibold text-fg data-[tone=accent]:border-accent-alt data-[tone=accent]:text-accent-alt data-[tone=danger]:border-danger data-[tone=danger]:text-danger data-[tone=installed]:border-status-installed data-[tone=installed]:text-status-installed data-[tone=warning]:border-warning data-[tone=warning]:text-warning",
                            "{label}"
                        }
                        span { "data-part": "label", class: "text-fg-muted", "{text}" }
                    }
                }
            }
            ClockText {}
        }
    }
}
