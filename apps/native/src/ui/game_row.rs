//! A game in the library's list layout, as the webview's `LibraryRow`:
//! icon, title, status, when it was last played, time played and size. Like
//! the card, the row is one focus stop opened by a click, Enter or a pad's
//! A; the list around it (#94) places it.

use caskade_ui_i18n::t;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::game_card::CardGame;
use super::icons::{CHECK, HEART, Svg};
use super::interact::{Keys, flag, use_interaction};
use super::status_badge::StatusBadge;

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("icon", "The game's icon, or its initial on generated art."),
        (
            "title",
            "The game's title; links to the game and covers the row.",
        ),
        ("favorite", "The favourite heart."),
        (
            "storage-availability",
            "Shown when the managed copy is unavailable.",
        ),
        ("plugins", "Badges contributed by plugins."),
        ("badge", "The status (a `status-badge`)."),
        ("last-played", "When it was last played."),
        ("playtime", "Time played."),
        ("size", "Size on disk."),
        ("actions", "The actions menu button."),
        (
            "select",
            "In selection mode, the check that shows whether the game is chosen.",
        ),
        (
            "starting",
            "Starting…, after the title from a double click or Play until the game runs, the launch fails or a question stops it.",
        ),
    ],
    states: &[
        ("data-status", &["installed", "not-installed", "emulated"]),
        ("data-storage-status", &["unavailable"]),
        ("data-selected", &["true"]),
    ],
    contains: &["status-badge"],
    slots: &["plugins"],
    ..Entry::new(
        "game-row",
        "GameRow (apps/native/src/ui/game_row.rs)",
        Source::App,
        "A game in the library's compact table (the list layout).",
    )
};

#[component]
pub fn GameRow(
    game: CardGame,
    /// The game's icon's URL, when it has one.
    #[props(default)]
    icon: Option<String>,
    #[props(default)] selected: Option<bool>,
    #[props(default)] starting: bool,
    #[props(default)] focus_key: Option<String>,
    #[props(default)] on_open: Option<EventHandler<()>>,
    #[props(default)] on_focus: Option<EventHandler<()>>,
    /// The actions menu button, at the end of the row.
    #[props(default)]
    actions: Option<Element>,
    /// A table's `grid-template-columns` (the webview's list:
    /// `minmax(0,1fr) 7.5rem 7.5rem 5rem 5.5rem 2rem`): the row lays out
    /// as cells in them (title, status, last played, playtime, size,
    /// actions), lining up with a header that uses the same. Without
    /// them, a flex row.
    #[props(default)]
    columns: Option<String>,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let interaction = use_interaction();
    let table = columns.is_some();
    let layout = match &columns {
        Some(columns) => format!("grid-template-columns: {columns};"),
        None => String::new(),
    };
    // Fixed widths in a flex row; a table's columns size its cells.
    let width = |flex: &'static str| if table { "" } else { flex };
    let (last_played_width, playtime_width, size_width) =
        (width("w-40"), width("w-24"), width("w-20"));
    let open = move || {
        if let Some(on_open) = on_open {
            on_open.call(());
        }
    };
    let last_played = game
        .last_played_ms
        .map(crate::library::local_date_time)
        .map(caskade_ui_i18n::format::date_time)
        .unwrap_or_else(|| caskade_ui_i18n::format::last_played(None));
    let playtime = caskade_ui_i18n::format::playtime(game.playtime_ms / 1000);
    let size = caskade_ui_i18n::format::size(game.size_bytes);
    let initial = game
        .title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    rsx! {
        div {
            role: "link",
            tabindex: "0",
            "aria-label": "{game.title}",
            "aria-pressed": selected.map(|selected| if selected { "true" } else { "false" }),
            "data-cs": "game-row",
            "data-status": game.status.name(),
            "data-storage-status": game.storage_unavailable.then_some("unavailable"),
            "data-selected": selected.and_then(flag),
            "data-focus-key": focus_key,
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            class: if table { "relative grid h-full items-center gap-3 border-b border-border px-2 outline-none data-hovered:bg-surface-hover data-focus-visible:focus-ring-over" } else { "relative flex h-full items-center gap-4 border-b border-border px-3 outline-none data-hovered:bg-surface-hover data-focus-visible:focus-ring-over" },
            style: "{layout}",
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
            // The title's cell.
            div { class: if table { "flex min-w-0 items-center gap-3" } else { "flex min-w-0 flex-1 items-center gap-4" },
            if let Some(selected) = selected {
                span {
                    "data-part": "select",
                    "aria-hidden": "true",
                    class: if selected { "grid size-5 shrink-0 place-items-center rounded-full bg-accent text-accent-fg" } else { "size-5 shrink-0 rounded-full border-2 border-fg-subtle" },
                    if selected {
                        Svg { icon: CHECK, stroke_width: 3.0, class: "size-3.5" }
                    }
                }
            }
            div { "data-part": "icon", "aria-hidden": "true", class: "grid size-7 shrink-0 place-items-center overflow-hidden rounded bg-surface-hover text-xs text-fg-muted",
                if let Some(icon) = icon {
                    img { src: "{icon}", alt: "", class: "size-full object-cover" }
                } else {
                    "{initial}"
                }
            }
            span { "data-part": "title", class: "min-w-0 grow truncate text-sm", "{game.title}" }
            if starting {
                span { "data-part": "starting", class: "shrink-0 text-xs text-fg-muted", {t!("Starting…")} }
            }
            if game.favorite {
                span {
                    "data-part": "favorite",
                    role: "img",
                    "aria-label": t!(note = "A heart on a favourite game's card", "Favorite"),
                    class: "shrink-0 text-accent-alt",
                    Svg { icon: HEART, class: "size-3.5 fill-accent-alt" }
                }
            }
            if game.storage_unavailable {
                span { "data-part": "storage-availability", class: "shrink-0 text-[10px] text-warning", {t!("Managed copy unavailable")} }
            }
            div { "data-part": "plugins", class: "contents" }
            }
            div { class: "flex min-w-0 shrink-0",
                StatusBadge { part: "badge", status: game.status }
            }
            span { "data-part": "last-played", class: "{last_played_width} shrink-0 truncate text-xs text-fg-muted", "{last_played}" }
            span { "data-part": "playtime", class: "{playtime_width} shrink-0 text-right text-xs text-fg-muted", "{playtime}" }
            span { "data-part": "size", class: "{size_width} shrink-0 text-right text-xs text-fg-muted", "{size}" }
            if actions.is_some() || table {
                span { "data-part": "actions", class: "flex shrink-0 justify-end", {actions} }
            }
        }
    }
}
