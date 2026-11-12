//! The library's Web tab (#99, the webview's `views/library/web`): its games
//! as a mosaic of square tiles, every one at once, favourites and games
//! played lately or often large; Add web game in the toolbar; and the web
//! game dialogs a game's menu opens (Play offline…, Remove…).

use crate::data::{Query, change, use_query};
use crate::flows::storage::{InstallRootPicker, use_storage};
use crate::router::{Route, use_router};
use crate::ui::{Button, ButtonVariant, Checkbox, Dialog, GameArt, Svg, TextField, icons};
use caskade_core::error::ErrorReport;
use caskade_core::model::{GameSummary, ListGames};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;
use std::collections::HashMap;

/// The most web games the mosaic shows: the core's page limit.
const MOSAIC_LIMIT: u32 = 1000;

/// Add web game's page field placeholder: a URL's start, the same in every
/// language.
const URL_PLACEHOLDER: &str = "https://";

/// Played this recently, a game's tile is large.
const RECENT_DAYS: i64 = 7;

/// A tile's size: 1×1, 2×2 or 3×3 cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileSize {
    Small,
    Large,
    Huge,
}

impl TileSize {
    fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large => "large",
            Self::Huge => "huge",
        }
    }

    fn span(self) -> &'static str {
        match self {
            Self::Small => "",
            Self::Large => "col-span-2 row-span-2",
            Self::Huge => "col-span-3 row-span-3",
        }
    }
}

/// What a game's tile size goes by.
struct TileFacts<'a> {
    id: &'a str,
    favorite: bool,
    seconds: u64,
    last_played_at_ms: Option<i64>,
}

/// Each game's tile. A tile is large when the game is a favourite, was
/// played in the last week, or has play time in the top quarter of `games`;
/// the first large one is huge.
pub fn tile_sizes(games: &[GameSummary], now_ms: i64) -> HashMap<String, TileSize> {
    let facts: Vec<TileFacts> = games
        .iter()
        .map(|game| TileFacts {
            id: &game.id,
            favorite: game.favorite,
            seconds: game.playtime_ms / 1000,
            last_played_at_ms: game.last_played_at_ms,
        })
        .collect();
    sizes(&facts, now_ms)
}

fn sizes(games: &[TileFacts], now_ms: i64) -> HashMap<String, TileSize> {
    let mut played: Vec<u64> = games
        .iter()
        .map(|game| game.seconds)
        .filter(|&seconds| seconds > 0)
        .collect();
    played.sort_unstable_by(|a, b| b.cmp(a));
    let top = games.len().div_ceil(4);
    // The play time a game needs to be in the top quarter.
    let threshold = top
        .min(played.len())
        .checked_sub(1)
        .and_then(|at| played.get(at).copied())
        .unwrap_or(u64::MAX);
    let since = now_ms - RECENT_DAYS * 86_400_000;
    let mut sizes = HashMap::new();
    let mut huge = false;
    for game in games {
        let large = game.favorite
            || game.last_played_at_ms.is_some_and(|at| at >= since)
            || (game.seconds > 0 && game.seconds >= threshold);
        let size = if large && !huge {
            huge = true;
            TileSize::Huge
        } else if large {
            TileSize::Large
        } else {
            TileSize::Small
        };
        sizes.insert(game.id.to_owned(), size);
    }
    sizes
}

/// Every game on the Web tab, for the mosaic.
pub fn use_web_games(query: ReadSignal<ListGames>) -> Query<Vec<GameSummary>> {
    use_query(DataKey::Games, move |data| {
        let query = ListGames {
            offset: 0,
            limit: MOSAIC_LIMIT,
            ..query()
        };
        async move { data.page(&query).await.map(|page| page.items) }
    })
}

#[component]
pub fn WebMosaic(games: Vec<GameSummary>) -> Element {
    let now = chrono::Utc::now().timestamp_millis();
    let sizes = tile_sizes(&games, now);
    rsx! {
        ul {
            "aria-label": t!(note = "The Web tab's mosaic of games", "Web games"),
            "data-part": "mosaic",
            class: "grid grid-flow-dense auto-rows-(--tile) grid-cols-[repeat(auto-fill,var(--tile))] justify-center gap-3 pb-8 [--tile:calc(var(--cs-card-width)*0.6)]",
            for (index, game) in games.into_iter().enumerate() {
                WebTile {
                    key: "{game.id}",
                    size: sizes.get(&game.id).copied().unwrap_or(TileSize::Small),
                    game,
                    index,
                }
            }
        }
    }
}

#[component]
fn WebTile(game: GameSummary, size: TileSize, index: usize) -> Element {
    let menus = crate::game_menu::use_game_menus();
    let target = crate::game_menu::Target {
        game_id: game.id.clone(),
        title: game.title.clone(),
        entry_id: None,
    };
    let mut wrapper = use_signal(|| None::<dioxus_native::NodeHandle>);
    let starting = menus.is_some_and(|menus| menus.starting(&game.id));
    let cover = super::art_url_card(game.cover.as_ref());
    let open_menu = {
        let target = target.clone();
        use_callback(move |anchor: Option<crate::ui::Anchor>| {
            let Some(menus) = menus else { return };
            let anchor = anchor.or_else(|| wrapper.peek().as_ref().and_then(crate::ui::Anchor::of));
            if let Some(anchor) = anchor {
                menus.open(target.clone(), anchor);
            }
        })
    };
    rsx! {
        li {
            "data-focus-key": "game-card:{game.id}",
            "data-cs": "game-card",
            "data-variant": "web",
            "data-tile": size.name(),
            "data-status": if game.installed { "installed" } else { "not-installed" },
            style: "--i: {index.min(20)}",
            class: "relative min-w-0 animate-enter rounded-[calc(var(--cs-radius-card)*2)] {size.span()}",
            onmounted: move |event: Event<MountedData>| wrapper.set(event.data().downcast::<dioxus_native::NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                event.stop_propagation();
                open_menu.call(Some(crate::frame::pointer_anchor(wrapper.peek().as_ref(), &event)));
            },
            onkeydown: move |event: KeyboardEvent| {
                let shift_f10 = event.key() == Key::Named(NamedKey::F10) && event.modifiers().shift();
                if event.key() == Key::Named(NamedKey::ContextMenu) || shift_f10 {
                    event.prevent_default();
                    event.stop_propagation();
                    open_menu.call(None);
                }
            },
            ondoubleclick: {
                let target = target.clone();
                move |_| {
                    if let Some(menus) = menus {
                        menus.play(target.clone());
                    }
                }
            },
            button {
                r#type: "button",
                "aria-label": "{game.title}",
                "data-spatial": "focusable",
                class: "group relative block size-full overflow-hidden rounded-[calc(var(--cs-radius-card)*2)] border border-border bg-surface-raised outline-none data-focus-visible:focus-ring-over",
                onclick: {
                    let target = target.clone();
                    move |_| {
                        if let Some(menus) = menus {
                            menus.look(target.clone());
                        }
                    }
                },
                GameArt { part: "art", image: cover, class: "size-full" }
                if game.admin_pin_needed {
                    span { class: "absolute top-2 left-2 rounded-control bg-surface-base/90 px-2 py-1",
                        crate::ui::AgeLockLabel {}
                    }
                }
                p {
                    "data-part": "title",
                    class: "absolute inset-x-0 bottom-0 truncate bg-linear-to-t from-surface-base/90 to-transparent px-2.5 pt-6 pb-2 text-sm font-semibold opacity-0 transition-opacity duration-(--cs-motion-duration-fast) group-hover:opacity-100 group-data-focus-visible:opacity-100",
                    "{game.title}"
                }
                if starting {
                    span {
                        "data-part": "starting",
                        class: "absolute inset-x-0 bottom-0 bg-surface-base/85 px-3 py-1.5 text-xs text-fg",
                        {t!("Starting…")}
                    }
                }
                if game.favorite {
                    span {
                        "data-part": "favorite",
                        role: "img",
                        "aria-label": t!(note = "A heart on a favourite game's card", "Favorite"),
                        class: "absolute top-2.5 right-2.5",
                        Svg { icon: icons::HEART, class: "size-3.5 fill-accent-alt text-accent-alt" }
                    }
                }
            }
        }
    }
}

type Web = &'static caskade_core::web::WebGames;

fn web() -> Result<Web, ErrorReport> {
    crate::shell::services()
        .map(|services| &services.web)
        .ok_or_else(|| ErrorReport::from(t!("Web games aren't available here")))
}

/// Adds the web game at `url`: its id.
pub async fn add(url: String) -> Result<String, ErrorReport> {
    let web = web()?;
    change(async move { caskade_app::commands::web::add_web_game(web, &url).await }).await
}

/// Opens the guided capture that keeps `game_id` in root `root_id`.
pub async fn save_offline(game_id: String, root_id: String) -> Result<(), ErrorReport> {
    let web = crate::shell::services()
        .map(|services| &services.web)
        .ok_or_else(|| ErrorReport::from(t!("Offline web games aren't available here")))?;
    change(async move {
        caskade_app::commands::web::save_web_game_offline(web, &game_id, &root_id).await
    })
    .await
}

/// Removes web game `game_id` for good.
pub async fn remove(game_id: String) -> Result<(), ErrorReport> {
    let web = web()?;
    change(async move { caskade_app::commands::web::remove_web_game(web, &game_id).await }).await
}

/// Deletes `game_id`'s offline copy; it plays online again.
pub async fn remove_offline(game_id: String) -> Result<(), ErrorReport> {
    let web = web()?;
    change(async move { caskade_app::commands::web::remove_web_offline(web, &game_id).await }).await
}

/// The Web tab's Add web game: a page address, and the game is in the
/// library. With Play offline, Caskade then asks where to keep it and opens
/// the guided capture.
#[component]
pub fn AddWebGame() -> Element {
    let storage = use_storage();
    let mut open = use_signal(|| false);
    let mut url = use_signal(String::new);
    let mut offline = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // The game added, waiting for where to keep its offline copy.
    let mut placing = use_signal(|| None::<String>);
    let can_offline = storage.is_some();
    let mut submit = move || {
        let page = url.peek().trim().to_owned();
        if page.is_empty() || *pending.peek() {
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            match add(page).await {
                Ok(game_id) => {
                    open.set(false);
                    url.set(String::new());
                    if *offline.peek() && can_offline {
                        placing.set(Some(game_id));
                    }
                    offline.set(false);
                }
                Err(report) => error.set(Some(report.summary)),
            }
            pending.set(false);
        });
    };
    rsx! {
        div {
            Button {
                icon: Some(icons::PLUS),
                variant: ButtonVariant::Primary,
                "data-spatial": "focusable",
                "data-part": "add-web-game",
                on_press: move |_| {
                    error.set(None);
                    open.set(true);
                },
                {t!("Add web game")}
            }
            if let Some(error) = error().filter(|_| !open()) {
                p { role: "alert", class: "mt-2 max-w-sm text-sm text-danger", "{error}" }
            }
            if open() {
                Dialog {
                    title: t!("Add web game"),
                    "data-cs": "add-web-game",
                    class: "max-w-lg!",
                    on_close: move |_| {
                        if !pending() {
                            open.set(false);
                        }
                    },
                    div { class: "mt-4 flex flex-col gap-4",
                        TextField {
                            label: t!("Game page"),
                            "data-focus-default": "true",
                            kind: "url",
                            placeholder: URL_PLACEHOLDER.to_owned(),
                            description: t!("The address of the page the game plays on."),
                            value: url(),
                            disabled: pending(),
                            on_input: move |value: String| url.set(value),
                            on_submit: move |_| submit(),
                        }
                        if can_offline {
                            div { class: "flex flex-col gap-1",
                                Checkbox {
                                    checked: offline(),
                                    disabled: pending(),
                                    label: t!("Play offline"),
                                    on_change: move |on: bool| offline.set(on),
                                }
                                p { class: "pl-7.5 text-xs text-fg-muted",
                                    {t!("Caskade keeps a copy of the game. You'll play it for a minute so it can see what the game needs.")}
                                }
                            }
                        }
                        if let Some(error) = error() {
                            p { role: "alert", class: "text-sm text-danger", "{error}" }
                        }
                        div { class: "flex justify-end gap-2",
                            Button { disabled: pending(), on_press: move |_| open.set(false), {t!("Cancel")} }
                            Button {
                                variant: ButtonVariant::Primary,
                                "data-part": "add",
                                disabled: url().trim().is_empty() || pending(),
                                on_press: move |_| submit(),
                                if pending() { {t!("Adding…")} } else { {t!("Add")} }
                            }
                        }
                    }
                }
            }
            if let Some(game_id) = placing() {
                SaveOfflineDialog {
                    game_id,
                    title: String::new(),
                    on_close: move |_| placing.set(None),
                }
            }
        }
    }
}

/// A web game's Play offline…: where to keep it, then the guided capture
/// opens.
#[component]
pub fn SaveOfflineDialog(game_id: String, title: String, on_close: EventHandler<()>) -> Element {
    let Some(storage) = use_storage() else {
        return rsx! {};
    };
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    rsx! {
        Dialog {
            title: t!("Where to keep {title}", title = title),
            class: "max-w-xl!",
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            if let Some(error) = error() {
                p { role: "alert", class: "mb-3 text-sm text-danger", "{error}" }
            }
            InstallRootPicker {
                storage,
                heading: false,
                on_cancel: move |_| on_close.call(()),
                on_select: move |root_id: String| {
                    let game_id = game_id.clone();
                    pending.set(true);
                    spawn(async move {
                        match save_offline(game_id, root_id).await {
                            Ok(()) => on_close.call(()),
                            Err(report) => error.set(Some(report.summary)),
                        }
                        pending.set(false);
                    });
                },
            }
        }
    }
}

/// Asks before a web game's menu removes it: scans never bring a web game
/// back, so it goes for good, with its offline copy and saves. On its own
/// page, the page goes back.
#[component]
pub fn RemoveWebGameDialog(
    game_id: String,
    title: String,
    /// The game has other entries, which stay.
    keeps_game: bool,
    on_close: EventHandler<()>,
) -> Element {
    let router = use_router();
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let body = if keeps_game {
        t!(
            "Its web version is removed: the offline copy and the progress saved in the game window are deleted, with its play sessions. The game's other versions stay in your library."
        )
    } else {
        t!(
            "It leaves your library: the offline copy and the progress saved in the game window are deleted, with its play sessions. Add its page again to play it again."
        )
    };
    let confirm = move |_| {
        let game_id = game_id.clone();
        pending.set(true);
        spawn(async move {
            match remove(game_id.clone()).await {
                Ok(()) => {
                    on_close.call(());
                    if !keeps_game
                        && let Route::Game { id, .. } = router.peek()
                        && id == game_id
                    {
                        router.back();
                    }
                }
                Err(report) => error.set(Some(report.summary)),
            }
            pending.set(false);
        });
    };
    rsx! {
        Dialog {
            title: t!("Remove {title}?", title = title),
            alert: true,
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted", "{body}" }
            if let Some(error) = error() {
                p { role: "alert", class: "mt-3 text-sm text-danger", "{error}" }
            }
            div { class: "mt-5 flex justify-end gap-2",
                Button {
                    "data-spatial": "focusable",
                    "data-focus-default": "true",
                    disabled: pending(),
                    on_press: move |_| on_close.call(()),
                    {t!("Cancel")}
                }
                Button {
                    "data-spatial": "focusable",
                    variant: ButtonVariant::Danger,
                    disabled: pending(),
                    on_press: confirm,
                    if pending() { {t!("Removing…")} } else { {tp!("web game", "Remove")} }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, favorite: bool, seconds: u64, last: Option<i64>) -> TileFacts<'_> {
        TileFacts {
            id,
            favorite,
            seconds,
            last_played_at_ms: last,
        }
    }

    #[test]
    fn favourites_and_games_played_lately_or_often_get_large_tiles_the_first_huge() {
        let now = 100 * 86_400_000;
        let games = [
            game("plain", false, 0, None),
            game("fav", true, 0, None),
            game("lately", false, 10, Some(now - 86_400_000)),
            game("long-ago", false, 5, Some(now - 30 * 86_400_000)),
            game("most", false, 9_000, Some(now - 40 * 86_400_000)),
            game("never", false, 0, None),
            game("some", false, 1, None),
            game("tiny", false, 2, None),
        ];
        let sizes = sizes(&games, now);
        assert_eq!(sizes["plain"], TileSize::Small);
        assert_eq!(sizes["fav"], TileSize::Huge);
        assert_eq!(sizes["lately"], TileSize::Large);
        // Top quarter of 8 games: the 2 most played (9,000 s and 10 s).
        assert_eq!(sizes["most"], TileSize::Large);
        assert_eq!(sizes["long-ago"], TileSize::Small);
        assert_eq!(sizes["some"], TileSize::Small);
    }
}
