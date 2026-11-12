//! A game tile in a grid, as `@caskade/ui`'s `GameCard` and the library's
//! `LibraryCard`: the card is one focus stop (`role="link"`), opened by a
//! click, Enter or a pad's A. The grid around it (#94) gives it its place
//! (`data-index`, `data-focus-default` through `attributes`), its focus key
//! and what opening it does.

use caskade_core::model::GameSummary;
use caskade_ui_i18n::{t, tn};
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::game_art::GameArt;
use super::icons::{CHECK, CIRCLE_CHECK, CLOCK, HARD_DRIVE, HEART, STORE, Svg};
use super::interact::{Keys, flag, use_interaction};
use super::status_badge::{GameStatus, StatusBadge};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("art", "The cover (a `game-art`)."),
        ("badge", "The status (a `status-badge`)."),
        ("title", "The game's title."),
        (
            "subtitle",
            "The genre; on LibraryCard, the platform unless it is a PC one.",
        ),
        ("meta", "The row of facts under the title."),
        ("playtime", "Time played."),
        ("size", "Size on disk."),
        ("last-played", "When it was last played."),
        ("stores", "How many stores sell it."),
        ("owned", "Shown instead of stores when you own it."),
        ("favorite", "The favourite heart."),
        (
            "storage-availability",
            "Shown when the managed copy is unavailable.",
        ),
        ("plugins", "Badges and overlays contributed by plugins."),
        (
            "select",
            "In selection mode, the check that shows whether the game is chosen.",
        ),
        (
            "playing-elsewhere",
            "In console mode, the paired device the game is running on.",
        ),
        (
            "played-on",
            "On Home's Recently Played, the other device the game was last played on, in place of the subtitle.",
        ),
        (
            "starting",
            "Starting\u{2026}, over the art from a double click or Play until the game runs, the launch fails or a question stops it.",
        ),
    ],
    states: &[
        (
            "data-status",
            &["installed", "not-installed", "available", "emulated"],
        ),
        ("data-variant", &["library", "recent", "store", "web"]),
        ("data-storage-status", &["unavailable"]),
        ("data-tile", &["small", "large", "huge"]),
        ("data-selected", &["true"]),
    ],
    app_only: &[
        "favorite",
        "storage-availability",
        "plugins",
        "select",
        "playing-elsewhere",
        "played-on",
        "starting",
        "data-storage-status",
        "data-tile",
        "data-selected",
    ],
    contains: &["game-art", "status-badge"],
    slots: &["plugins"],
    ..Entry::new(
        "game-card",
        "GameCard (apps/native/src/ui/game_card.rs)",
        Source::Ui,
        "A game tile in a grid.",
    )
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CardVariant {
    /// The library's card: cover, title, platform.
    #[default]
    Library,
    /// Recently played: with time played and when.
    Recent,
    /// A store's: size and how many stores sell it.
    Store,
    /// A web game's tile.
    Web,
}

impl CardVariant {
    fn name(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Recent => "recent",
            Self::Store => "store",
            Self::Web => "web",
        }
    }
}

/// What a card shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardGame {
    pub id: String,
    pub title: String,
    /// The library's platform line, a store's genre…
    pub subtitle: Option<String>,
    pub status: GameStatus,
    /// The cover's URL (`caskade-asset://`).
    pub cover: Option<String>,
    pub favorite: bool,
    pub playtime_ms: u64,
    pub last_played_ms: Option<i64>,
    /// Store variant: its size.
    pub size_bytes: Option<u64>,
    /// Store variant: how many stores sell it; `None` when it's owned.
    pub stores: Option<usize>,
    /// The managed copy is unavailable (`data-storage-status`).
    pub storage_unavailable: bool,
    /// The other device it was last played on, in place of the subtitle
    /// (Home's Recently Played).
    pub played_on: Option<String>,
    /// The signed-in account's age limit holds it back (#106).
    pub admin_pin_needed: bool,
}

impl CardGame {
    /// A library game, with its cover's URL.
    pub fn from_summary(game: &GameSummary, cover: Option<String>) -> Self {
        Self {
            id: game.id.clone(),
            title: game.title.clone(),
            subtitle: platform_line(&game.platforms),
            status: if game.installed {
                GameStatus::Installed
            } else {
                GameStatus::NotInstalled
            },
            cover,
            favorite: game.favorite,
            playtime_ms: game.playtime_ms,
            last_played_ms: game.last_played_at_ms,
            admin_pin_needed: game.admin_pin_needed,
            ..Self::default()
        }
    }
}

/// A game's platform under its title, unless it's a PC game, as the
/// webview's `platformLine`.
pub fn platform_line(platforms: &[String]) -> Option<String> {
    match platforms {
        [] => None,
        [only] if only == "PC" || only == "Windows" => None,
        [first] => Some(first.clone()),
        [first, rest @ ..] => Some(format!("{first} +{}", rest.len())),
    }
}

#[component]
pub fn GameCard(
    game: CardGame,
    #[props(default)] variant: CardVariant,
    /// Its place in its grid: the enter animation's stagger.
    #[props(default)]
    index: usize,
    /// In selection mode, whether it's chosen.
    #[props(default)]
    selected: Option<bool>,
    /// "Starting…" over the art.
    #[props(default)]
    starting: bool,
    /// Where focus comes back to (`data-focus-key`).
    #[props(default)]
    focus_key: Option<String>,
    /// Opened: a click, Enter or a pad's A.
    #[props(default)]
    on_open: Option<EventHandler<()>>,
    /// Focus came in (by any means).
    #[props(default)]
    on_focus: Option<EventHandler<()>>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let interaction = use_interaction();
    let open = move || {
        if let Some(on_open) = on_open {
            on_open.call(());
        }
    };
    let hours = game.playtime_ms / 3_600_000;
    let playtime = if hours > 0 {
        t!(
            note = "Hours played, abbreviated",
            "{playtimeHours}h",
            playtimeHours = hours
        )
    } else {
        "\u{2014}".to_owned()
    };
    let size = game
        .size_bytes
        .map(|bytes| {
            let gb = ((bytes as f64) / 1e9 * 10.0).round() / 10.0;
            t!(
                note = "A game's size on disk, in gigabytes",
                "{sizeGb} GB",
                sizeGb = gb
            )
        })
        .unwrap_or_else(|| "\u{2014}".to_owned());
    let last_played = game
        .last_played_ms
        .map(crate::library::local_date_time)
        .map(caskade_ui_i18n::format::date_time)
        .unwrap_or_else(|| caskade_ui_i18n::format::last_played(None));
    rsx! {
        div {
            role: "link",
            tabindex: "0",
            "aria-label": "{game.title}",
            "aria-pressed": selected.map(|selected| if selected { "true" } else { "false" }),
            "data-cs": "game-card",
            "data-status": game.status.name(),
            "data-variant": variant.name(),
            "data-storage-status": game.storage_unavailable.then_some("unavailable"),
            "data-selected": selected.and_then(flag),
            "data-focus-key": focus_key,
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            style: "--i: {index};",
            class: "group relative flex h-full min-w-0 flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none transition-colors duration-(--cs-motion-duration-fast) data-hovered:bg-surface-hover data-focus-visible:focus-ring-over",
            onfocusin: move |_| {
                interaction.focus();
                if let Some(on_focus) = on_focus {
                    on_focus.call(());
                }
            },
            onfocusout: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| open(),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::Enter, open);
            },
            ..attributes,
            div { class: "relative min-h-0 flex-1",
                GameArt { part: "art", image: game.cover.clone(), class: "size-full" }
                if variant != CardVariant::Library {
                    div { class: "absolute top-2 left-2",
                        StatusBadge { part: "badge", status: game.status }
                    }
                } else if game.admin_pin_needed {
                    span { class: "absolute top-2 left-2 rounded-control bg-surface-base/90 px-2 py-1",
                        super::AgeLockLabel {}
                    }
                }
                if starting {
                    span {
                        "data-part": "starting",
                        class: "absolute inset-x-0 bottom-0 bg-surface-base/85 px-3 py-1.5 text-xs text-fg",
                        {t!("Starting…")}
                    }
                }
            }
            div { class: "flex min-w-0 shrink-0 flex-col gap-1 px-3 py-2.5",
                p { "data-part": "title", class: "truncate text-sm font-semibold", "{game.title}" }
                if let Some(played_on) = &game.played_on {
                    p { "data-part": "played-on", title: "{played_on}", class: "truncate text-xs text-fg-subtle", "{played_on}" }
                } else if variant != CardVariant::Recent && let Some(subtitle) = &game.subtitle {
                    p { "data-part": "subtitle", class: "truncate text-xs text-fg-subtle", "{subtitle}" }
                }
                if variant != CardVariant::Library && variant != CardVariant::Web {
                    div { "data-part": "meta", class: "flex items-center justify-between font-mono text-[11px] text-fg-muted",
                        if variant == CardVariant::Store {
                            span { "data-part": "size", class: "inline-flex items-center gap-1",
                                Svg { icon: HARD_DRIVE, class: "size-3.5" }
                                "{size}"
                            }
                            if let Some(stores) = game.stores {
                                span { "data-part": "stores", class: "inline-flex items-center gap-1",
                                    Svg { icon: STORE, class: "size-3.5" }
                                    {tn!(note = "How many stores sell a game", "# store", "# stores", stores)}
                                }
                            } else {
                                span { "data-part": "owned", class: "inline-flex items-center gap-1 text-status-installed",
                                    Svg { icon: CIRCLE_CHECK, class: "size-3.5" }
                                    {t!(note = "A store game the user already owns", "Owned")}
                                }
                            }
                        } else {
                            span { "data-part": "playtime", class: "inline-flex items-center gap-1",
                                Svg { icon: CLOCK, class: "size-3.5" }
                                "{playtime}"
                            }
                            span { "data-part": "last-played", "{last_played}" }
                        }
                    }
                }
            }
            if game.storage_unavailable {
                span {
                    "data-part": "storage-availability",
                    class: "absolute top-2 left-2 rounded-control border border-warning/40 bg-surface-base/90 px-2 py-1 text-[10px] text-warning",
                    {t!("Managed copy unavailable")}
                }
            }
            if let Some(selected) = selected {
                span {
                    "data-part": "select",
                    "aria-hidden": "true",
                    class: if selected { "absolute top-2 left-2 grid size-6 place-items-center rounded-full border-2 border-accent-alt bg-accent text-accent-fg" } else { "absolute top-2 left-2 grid size-6 place-items-center rounded-full border-2 border-fg-subtle bg-surface-base/80" },
                    if selected {
                        Svg { icon: CHECK, stroke_width: 3.0, class: "size-4" }
                    }
                }
            }
            if game.favorite {
                span {
                    "data-part": "favorite",
                    role: "img",
                    "aria-label": t!(note = "A heart on a favourite game's card", "Favorite"),
                    class: "absolute top-3 right-3 text-accent-alt",
                    Svg { icon: HEART, class: "size-3.5 fill-accent-alt" }
                }
            }
            div { "data-part": "plugins", class: "contents",
                // Plugins' badges and overlays (#109).
                crate::plugins::CardExtras { game_id: game.id.clone() }
            }
        }
    }
}
