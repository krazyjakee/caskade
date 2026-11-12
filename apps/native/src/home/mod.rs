//! Home (#98, the webview's `views/home/HomeView.tsx`): the game played
//! last as a hero with Resume · View details, then Recently Played,
//! Recently Added and Recently Released as shelves of the library's cards,
//! and the plugins' sections.

mod anatomy;
mod shelf_row;
#[cfg(test)]
mod tests;

pub use anatomy::ANATOMY;
#[allow(
    unused_imports,
    reason = "the store and collections views (#102) use it"
)]
pub(crate) use shelf_row::ShelfRow;

use caskade_core::model::{GameFilter, GamePage, GameSort, GameSummary, ListGames, PlayedOn};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{format, t};
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use crate::input::{Bind, use_input_action};

use crate::data::{Query, use_library_stats, use_query};
use crate::game_menu::{Target, use_game_menus};
use crate::library::LibraryCard;
use crate::router::{Route, use_router};
use crate::ui::icons::{self, Icon};
use crate::ui::{GameArt, Link, Svg};

/// Games on a shelf.
const SHELF_SIZE: u32 = 12;

/// Lucide's `history`: Recently Played.
const HISTORY: Icon = icons::HISTORY;

/// Lucide's `package-plus`: Recently Added.
const PACKAGE_PLUS: Icon = Icon(&[
    "M16 16h6",
    "M19 13v6",
    "M21 10V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l2-1.14",
    "m7.5 4.27 9 5.15",
    "M3.29 7 12 12l8.71-5",
    "M12 22V12",
]);

/// Lucide's `calendar-days`: Recently Released.
pub(crate) const CALENDAR_DAYS: Icon = Icon(&[
    "M8 2v4",
    "M16 2v4",
    "M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
    "M3 10h18",
    "M8 14h.01",
    "M12 14h.01",
    "M16 14h.01",
    "M8 18h.01",
    "M12 18h.01",
    "M16 18h.01",
]);

/// Lucide's `monitor-smartphone`: played on another device.
const MONITOR_SMARTPHONE: Icon = Icon(&[
    "M18 8V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h8",
    "M10 19v-3.96 3.15",
    "M7 19h5",
    "M16 10h4a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-4a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z",
]);

/// Where a game was last played when it was another device (#74): the
/// name this device has for it, or "another device" when the two aren't
/// paired. A game last played here has no label.
pub fn played_on_label(played_on: Option<&PlayedOn>) -> Option<String> {
    let played_on = played_on?;
    Some(
        match played_on
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            Some(device) => t!(
                note = "Home: the paired device a game's latest session was played on",
                "Last played on {device}",
                device = device
            ),
            None => t!(
                note = "Home: a game's latest session was played on a device this one isn't paired with",
                "Last played on another device"
            ),
        },
    )
}

/// The local calendar date, `YYYY-MM-DD` like release dates.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Of `games`, those released by `today`: pre-orders and announced games
/// wait for their date, and undated ones stay off.
pub fn released(games: &[GameSummary], today: &str) -> Vec<GameSummary> {
    games
        .iter()
        .filter(|game| {
            game.release_date
                .as_deref()
                .is_some_and(|date| date <= today)
        })
        .cloned()
        .collect()
}

/// The library's first `limit` games under `filter` in `sort`.
fn use_games(filter: GameFilter, sort: GameSort, limit: u32) -> Query<GamePage> {
    use_query(DataKey::Games, move |library| {
        let query = ListGames {
            filter,
            sort,
            limit,
            ..ListGames::default()
        };
        async move { library.page(&query).await }
    })
}

/// How many games are running now.
fn use_running_count() -> Query<usize> {
    use_query(DataKey::Sessions(None), |library| async move {
        library.sessions(None, 200).await.map(|sessions| {
            sessions
                .iter()
                .filter(|session| session.ended_at_ms.is_none())
                .count()
        })
    })
}

/// The library opened on a filter and, given one, a sort; the rest as
/// the user left them (the webview's links).
fn library_href(filter: &str, sort: Option<&str>) -> String {
    match sort {
        Some(sort) => format!("/library?filter={filter}&sort={sort}"),
        None => format!("/library?filter={filter}"),
    }
}

fn sort_name(sort: GameSort) -> &'static str {
    match sort {
        GameSort::Added => "added",
        GameSort::Released => "released",
        GameSort::Recent => "recent",
        GameSort::Playtime => "playtime",
        GameSort::Popular => "popular",
        GameSort::Title => "title",
    }
}

#[component]
pub fn HomeView() -> Element {
    let router = use_router();
    let running = use_running_count();
    // The hero and the shelf's 12.
    let recent = use_games(GameFilter::Recent, GameSort::Recent, SHELF_SIZE + 1);
    let stats = use_library_stats();
    let (games, recent_failed, recent_loading) = match &*recent.read() {
        Some(Ok(page)) => (page.items.clone(), false, false),
        Some(Err(_)) => (Vec::new(), true, false),
        None => (Vec::new(), false, true),
    };
    let running_failed = matches!(&*running.read(), Some(Err(_)));
    let active = match &*running.read() {
        Some(Ok(count)) => *count,
        _ => 0,
    };
    let total = format::duration(match &*stats.read() {
        Some(Ok(stats)) => stats.playtime_ms / 1000,
        _ => 0,
    });
    // The backend projects recent games from the play sessions it keeps.
    let hero = games.first().cloned();
    // The shelf leaves out the hero's game.
    let shelf: Vec<GameSummary> = games
        .iter()
        .skip(1)
        .take(SHELF_SIZE as usize)
        .cloned()
        .collect();
    let shelf_shown = games.len() != 1 || recent_failed || running_failed;
    let recent_href = library_href("recent", Some("recent"));
    let all_href = library_href("all", None);
    rsx! {
        div { "data-view": "home", class: "mt-4",
            div { class: "mb-5 flex items-center justify-between gap-4",
                h1 { class: "font-display text-2xl font-semibold", {t!("Welcome back")} }
                p { class: "flex items-center gap-2 font-mono text-xs text-fg-muted",
                    Svg { icon: icons::CLOCK, class: "size-4" }
                    {t!(
                        note = "Home's header: the library's total playtime and the games running now",
                        "{total} total · {active} running",
                        total = total,
                        active = active
                    )}
                }
            }
            if let Some(game) = hero {
                // Keyed, so another game's hero is one of its own.
                for game in [game] {
                    ContinueHero { key: "{game.id}", game }
                }
            }
            if shelf_shown {
                ShelfRow {
                    id: "recent",
                    title: t!("Recently Played"),
                    icon: HISTORY,
                    view_all: rsx! {
                        Link {
                            href: recent_href.clone(),
                            "data-part": "view-all",
                            class: "text-sm text-accent-alt",
                            on_press: {
                                let href = recent_href.clone();
                                move |_| router.navigate(&href)
                            },
                            {t!("View all →")}
                        }
                    },
                    notice: rsx! {
                        if recent_loading {
                            p { role: "status", class: "text-fg-muted", {t!("Loading recent games…")} }
                        }
                        if recent_failed || running_failed {
                            p { role: "alert", class: "text-fg-muted", {t!("Could not load your recent activity.")} }
                        }
                        if !recent_loading && !recent_failed && games.is_empty() {
                            div { class: "rounded-panel border border-dashed border-border p-10 text-center text-fg-muted",
                                p { {t!("No sessions yet. Pick something from your library.")} }
                                Link {
                                    href: all_href.clone(),
                                    class: "mt-3 inline-block text-accent-alt",
                                    on_press: {
                                        let href = all_href.clone();
                                        move |_| router.navigate(&href)
                                    },
                                    {t!("Browse library →")}
                                }
                            }
                        }
                    },
                    for (index, game) in shelf.into_iter().enumerate() {
                        LibraryCard { key: "{game.id}", game, index, show_played_on: true, focus_first: false }
                    }
                }
            }
            NewGamesShelf { id: "added", title: t!("Recently Added"), icon: PACKAGE_PLUS, sort: GameSort::Added }
            NewGamesShelf {
                id: "released",
                title: t!("Recently Released"),
                icon: CALENDAR_DAYS,
                sort: GameSort::Released,
                released_only: true,
            }
            // Plugins' home.sections (#109).
            crate::plugins::HomeSections {}
        }
    }
}

/// The game played last: its art, Continue Playing, its play time, the
/// other device it was played on, and Resume · View details, which opens
/// its page (and goes back to Home from there).
#[component]
fn ContinueHero(game: GameSummary) -> Element {
    let router = use_router();
    let menus = use_game_menus();
    let mut me = use_signal(|| None::<NodeHandle>);
    let target = Target {
        game_id: game.id.clone(),
        title: game.title.clone(),
        entry_id: None,
    };
    let open_menu = {
        let target = target.clone();
        move |anchor: Option<crate::ui::Anchor>| {
            let Some(menus) = menus else { return };
            let anchor = anchor.or_else(|| me.peek().as_ref().and_then(crate::ui::Anchor::of));
            if let Some(anchor) = anchor {
                menus.open(target.clone(), anchor);
            }
        }
    };
    // Its menu: a pad's Start while focus is in it, the menu key or
    // Shift+F10 at it.
    {
        let open_menu = open_menu.clone();
        use_input_action(
            Action::Menu,
            Bind {
                owner: me.read().as_ref().map(NodeHandle::node_id),
                label: Some(t!(
                    note = "Button hint: a pad's Start opens the focused item's menu",
                    "Options"
                )),
                ..Bind::default()
            },
            move || {
                open_menu(None);
                true
            },
        );
    }
    let playtime = format::playtime(game.playtime_ms / 1000);
    let played_on = played_on_label(game.last_played_on.as_ref());
    let cover = crate::library::art_url_card(game.cover.as_ref());
    let href = Route::Game {
        id: game.id.clone(),
        from: None,
    }
    .href();
    rsx! {
        section {
            "data-cs": "hero",
            class: "relative flex min-h-64 overflow-hidden rounded-panel border border-border bg-surface-raised",
            onmounted: move |event| me.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: {
                let open_menu = open_menu.clone();
                move |event: MouseEvent| {
                    event.stop_propagation();
                    open_menu(Some(crate::frame::pointer_anchor(me.peek().as_ref(), &event)));
                }
            },
            onkeydown: {
                let open_menu = open_menu.clone();
                move |event: KeyboardEvent| {
                    let shift_f10 = event.key() == Key::Named(NamedKey::F10) && event.modifiers().shift();
                    if event.key() == Key::Named(NamedKey::ContextMenu) || shift_f10 {
                        event.prevent_default();
                        event.stop_propagation();
                        open_menu(None);
                    }
                }
            },
            GameArt { part: "art", image: cover, class: "w-1/3 shrink-0" }
            div { "data-part": "content", class: "flex flex-col justify-center p-8",
                p { "data-part": "eyebrow", class: "font-mono text-xs tracking-widest text-accent-alt uppercase",
                    {t!("Continue Playing")}
                }
                h2 { "data-part": "title", class: "mt-3 font-display text-3xl font-semibold", "{game.title}" }
                p { "data-part": "meta", class: "mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-fg-muted",
                    span { "{playtime}" }
                    if let Some(played_on) = played_on {
                        span { "data-part": "played-on", class: "flex items-center gap-1.5",
                            Svg { icon: MONITOR_SMARTPHONE, class: "size-4 shrink-0" }
                            "{played_on}"
                        }
                    }
                }
                Link {
                    href,
                    "data-part": "actions",
                    // With a pad, Home opens on the hero's action.
                    "data-focus-default": "true",
                    class: "mt-5 w-fit rounded-control bg-accent px-5 py-2 font-semibold text-accent-fg outline-none data-focus-visible:focus-ring",
                    on_press: {
                        let id = game.id.clone();
                        move |_| router.open_game(&id)
                    },
                    {t!("Resume · View details")}
                }
            }
        }
    }
}

/// A shelf of the whole library's first games in `sort`, left out until
/// it has some.
#[component]
fn NewGamesShelf(
    id: String,
    title: String,
    icon: Icon,
    sort: GameSort,
    /// Only games already released (Recently Released).
    #[props(default)]
    released_only: bool,
) -> Element {
    let router = use_router();
    // Room for the undated and the unreleased the shelf leaves out.
    let limit = if released_only {
        SHELF_SIZE * 2
    } else {
        SHELF_SIZE
    };
    let result = use_games(GameFilter::All, sort, limit);
    let (all, failed) = match &*result.read() {
        Some(Ok(page)) => (page.items.clone(), false),
        Some(Err(_)) => (Vec::new(), true),
        None => (Vec::new(), false),
    };
    let mut games = if released_only {
        released(&all, &today())
    } else {
        all
    };
    games.truncate(SHELF_SIZE as usize);
    if !failed && games.is_empty() {
        return rsx! {};
    }
    let href = library_href("all", Some(sort_name(sort)));
    rsx! {
        ShelfRow {
            id,
            title,
            icon,
            view_all: rsx! {
                Link {
                    href: href.clone(),
                    "data-part": "view-all",
                    class: "text-sm text-accent-alt",
                    on_press: move |_| router.navigate(&href),
                    {t!("View all →")}
                }
            },
            notice: rsx! {
                if failed {
                    p { role: "alert", class: "text-fg-muted", {t!("Could not load these games.")} }
                }
            },
            for (index, game) in games.into_iter().enumerate() {
                LibraryCard { key: "{game.id}", game, index, focus_first: false }
            }
        }
    }
}
