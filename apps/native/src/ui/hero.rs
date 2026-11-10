//! The large featured-game banner, as `@caskade/ui`'s `Hero`.

use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{CHEVRON_DOWN, ELLIPSIS, PLAY, Svg};
use super::status_badge::{GameStatus, StatusBadge};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("art", "The game's artwork (a `game-art`)."),
        ("glow", "The coloured glow behind the content."),
        ("shade", "The gradient that keeps text readable."),
        ("fx", "The scanline and beam overlay."),
        ("content", "The text and actions."),
        ("eyebrow", "The small label above the title."),
        ("live-dot", "The pulsing dot in the eyebrow."),
        ("title", "The game's title."),
        ("description", "The game's description."),
        ("meta", "Genre, size and playtime."),
        ("actions", "The buttons."),
        ("primary-action", "The launch button group."),
        ("badge", "The status (a `status-badge`)."),
        ("tags", "The game's tags."),
        (
            "website",
            "A link to the game's own page on the web (a web game's).",
        ),
        (
            "played-on",
            "On Home, the other device the game was last played on (in the meta).",
        ),
    ],
    app_only: &["art", "tags", "website", "played-on"],
    contains: &["game-art", "status-badge"],
    slots: &["actions"],
    ..Entry::new(
        "hero",
        "Hero (apps/native/src/ui/hero.rs); the home and game pages' hero sections (app)",
        Source::Ui,
        "The large featured-game banner.",
    )
};

/// What the hero shows; the app's game types fit it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeroGame {
    pub title: String,
    pub genre: String,
    pub status: GameStatus,
    /// Hue pair for the background.
    pub art: (u16, u16),
    pub description: Option<String>,
    pub playtime_hours: Option<f64>,
    pub last_played: Option<String>,
    pub size_gb: Option<f64>,
}

#[component]
pub fn Hero(
    game: HeroGame,
    #[props(default)] eyebrow: Option<String>,
    /// Label of the launch button.
    #[props(default)]
    action: Option<String>,
    /// Replaces the launch button and its neighbours, e.g. with store links.
    #[props(default)]
    actions: Option<Element>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let (a, b) = game.art;
    // The genre, then the size (unless there's playtime), the playtime and
    // when it was last played, as the webview lists them.
    let mut meta = vec![game.genre.clone()];
    if let Some(gb) = game.size_gb
        && game.playtime_hours.is_none_or(|hours| hours == 0.0)
    {
        meta.push(t!(
            note = "A game's size on disk, in gigabytes",
            "{sizeGb} GB",
            sizeGb = gb
        ));
    }
    if let Some(hours) = game.playtime_hours {
        meta.push(t!(
            note = "Hours played",
            "{playtimeHours}h played",
            playtimeHours = hours
        ));
    }
    if let Some(last_played) = game.last_played.as_ref().filter(|text| !text.is_empty()) {
        meta.push(t!("Last played {lastPlayed}", lastPlayed = last_played));
    }
    meta.retain(|item| !item.is_empty());
    rsx! {
        section {
            "data-cs": "hero",
            class: "relative flex h-(--cs-hero-height) flex-col justify-center overflow-hidden rounded-panel border border-border px-10",
            style: "background: linear-gradient(135deg, hsl({a} 50% 20%), hsl({b} 55% 10%))",
            ..attributes,
            div {
                "data-part": "glow",
                "aria-hidden": "true",
                class: "absolute -inset-1/4",
                style: "background: radial-gradient(40% 50% at 70% 40%, hsl({b} 70% 50% / 0.55), transparent 70%)",
            }
            div {
                "data-part": "shade",
                "aria-hidden": "true",
                class: "absolute inset-0 bg-linear-to-r from-surface-base from-25% to-transparent to-70%",
            }
            div { "data-part": "fx", "aria-hidden": "true" }
            div { "data-part": "content", class: "relative",
                if let Some(eyebrow) = &eyebrow {
                    div {
                        "data-part": "eyebrow",
                        class: "mb-3 flex animate-enter items-center gap-2 font-mono text-xs tracking-[0.2em] text-accent-alt uppercase",
                        span {
                            "data-part": "live-dot",
                            class: "size-1.5 rounded-full bg-accent-alt shadow-[0_0_8px] shadow-accent-alt",
                        }
                        "{eyebrow}"
                    }
                }
                h1 {
                    "data-part": "title",
                    class: "animate-enter font-display text-5xl font-semibold tracking-[0.14em] uppercase [--i:1]",
                    "{game.title}"
                }
                if let Some(description) = &game.description {
                    p {
                        "data-part": "description",
                        class: "mt-4 max-w-md animate-enter text-sm leading-relaxed text-fg-muted [--i:2]",
                        "{description}"
                    }
                }
                div {
                    "data-part": "meta",
                    class: "mt-3 flex animate-enter gap-2 font-mono text-[11px] tracking-wide text-fg-muted uppercase [--i:3]",
                    for (i, item) in meta.into_iter().enumerate() {
                        span { key: "{item}", class: "flex gap-2",
                            if i > 0 {
                                span { class: "text-accent-alt/60", "//" }
                            }
                            "{item}"
                        }
                    }
                }
                div {
                    "data-part": "actions",
                    class: "mt-6 flex animate-enter items-center gap-3 [--i:4]",
                    if let Some(actions) = actions {
                        {actions}
                    } else {
                        div { "data-part": "primary-action",
                            div { class: "chamfer flex bg-linear-to-r from-accent to-accent-strong text-accent-fg",
                                button {
                                    r#type: "button",
                                    class: "flex items-center gap-2 px-6 py-2.5 font-display font-semibold tracking-wider uppercase hover:bg-white/10",
                                    Svg { icon: PLAY, class: "size-4 fill-current" }
                                    if let Some(action) = &action {
                                        "{action}"
                                    }
                                }
                                button {
                                    r#type: "button",
                                    "aria-label": t!("More launch options"),
                                    class: "border-l border-white/20 px-2.5 hover:bg-white/10",
                                    Svg { icon: CHEVRON_DOWN, class: "size-4" }
                                }
                            }
                        }
                        button {
                            r#type: "button",
                            class: "rounded-control border border-border-strong bg-surface-overlay/70 px-5 py-2.5 text-sm backdrop-blur hover:border-accent-alt/50 hover:bg-surface-hover",
                            {t!("View Details")}
                        }
                        button {
                            r#type: "button",
                            "aria-label": t!(note = "A button opening more actions", "More"),
                            class: "rounded-control border border-border-strong bg-surface-overlay/70 p-2.5 backdrop-blur hover:border-accent-alt/50 hover:bg-surface-hover",
                            Svg { icon: ELLIPSIS, class: "size-4" }
                        }
                    }
                }
                div { class: "mt-4 animate-enter [--i:5]",
                    StatusBadge { part: "badge", status: game.status }
                }
            }
        }
    }
}
