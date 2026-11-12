//! One collection (the webview's `views/CollectionView.tsx`): its mosaic,
//! badge, name, description and numbers, a smart one's rules, Play Random,
//! Add Games (or Edit Rules) and its menu; then its games in the library's
//! virtual grid, which can choose several for one edit as the library
//! does.

use super::actions::{Open, use_collection_actions};
use super::card::{CollectionMosaic, KindBadge, collection_updated, cover_urls};
use super::rules::{describe_rules, sorted_by};
use super::{PlainButton, SHUFFLE};
use crate::data::{change, use_library, use_query};
use crate::game_menu::{Target, use_game_menus};
use crate::input::{Bind, use_input_action};
use crate::library::{
    AllIds, AllIdsProp, Events, Layout, SelectionBar, VirtualLibrary, use_selection_provider,
};
use crate::router::{CollectionFilter, Route, use_router};
use crate::ui::popover::use_trigger;
use crate::ui::{Anchor, Button, Link, Menu, Svg, icons};
use caskade_core::model::{CollectionKind, CollectionSummary, GameSummary, ListGames};
use caskade_ui_data::{ErrorReport, Key as DataKey, Library, PAGE_LIMIT};
use caskade_ui_i18n::{format, t, tn};
use caskade_ui_input::Action;
use dioxus_native::prelude::*;

/// Play Random looks at this many games at most, and picks an installed
/// one where it can.
const RANDOM_SAMPLE: usize = 1000;

#[component]
pub fn CollectionView(id: String) -> Element {
    // The card's summary (covers, playtime, how many games), from the
    // list the Collections view reads.
    let summary = use_query(DataKey::Collections, move |library| {
        let _ = super::revision();
        let id = id.clone();
        async move {
            library
                .collections()
                .await
                .map(|list| list.into_iter().find(|collection| collection.id == id))
        }
    });
    let state = summary.read().clone();
    match state {
        Some(Err(report)) => rsx! {
            div { role: "alert", class: "rounded-panel border border-border p-8",
                p { {t!("Could not load this collection.")} }
                p { class: "mt-2 text-sm text-fg-muted", "{report.summary}" }
            }
        },
        None => rsx! {
            p { role: "status", class: "text-fg-muted", {t!("Loading collection…")} }
        },
        Some(Ok(None)) => rsx! {
            div { "data-view": "collection", class: "rounded-panel border border-border p-8",
                p { {t!("This collection doesn't exist any more.")} }
                BackLink {}
            }
        },
        Some(Ok(Some(collection))) => rsx! {
            CollectionPage { collection }
        },
    }
}

/// Back to every collection.
#[component]
fn BackLink() -> Element {
    let router = use_router();
    let href = Route::Collections {
        filter: CollectionFilter::All,
    }
    .href();
    rsx! {
        Link {
            href: href.clone(),
            on_press: move |_| router.navigate(&href),
            class: "group mb-3 inline-flex items-center gap-1.5 font-mono text-xs tracking-wider text-fg-muted uppercase data-hovered:text-accent-alt",
            Svg { icon: icons::ARROW_LEFT, class: "size-4 transition-transform duration-(--cs-motion-duration-fast) group-data-hovered:-translate-x-1" }
            {t!("All Collections")}
        }
    }
}

#[component]
fn CollectionPage(collection: CollectionSummary) -> Element {
    let router = use_router();
    let library = use_library();
    let menus = use_game_menus();
    let actions = use_collection_actions();
    let trigger = use_trigger();
    let mut menu = use_signal(|| None::<Anchor>);
    let mut random_busy = use_signal(|| false);
    let mut random_error = use_signal(|| None::<String>);
    // How many games it holds now, and whether they load.
    let count = {
        let id = collection.id.clone();
        use_query(DataKey::Collections, move |library| {
            let _ = super::revision();
            let id = id.clone();
            async move {
                library
                    .collection_games(&id, 0, 1)
                    .await
                    .map(|page| page.total)
            }
        })
    };
    // Choosing several of its games for one edit (the webview's #173).
    let mut selection = use_selection_provider();
    use_input_action(
        Action::Back,
        Bind {
            label: selection.active().then(|| t!("Done selecting")),
            ..Bind::default()
        },
        move || {
            if !selection.active() {
                return false;
            }
            selection.stop();
            true
        },
    );

    let smart: Option<ListGames> = match &collection.kind {
        CollectionKind::Smart { query } => Some(query.clone()),
        CollectionKind::Manual => None,
    };
    let (total, games_error) = match &*count.read() {
        Some(Ok(total)) => (*total, None),
        Some(Err(report)) => (collection.games, Some(report.summary.clone())),
        None => (collection.games, None),
    };
    let counted = count.read().is_some();
    let playtime = collection.playtime_ms / 1000;
    let mut meta = vec![tn!("# game", "# games", total)];
    if playtime > 0 {
        meta.push(t!(
            note = "{played} is a duration: “12h 30m”",
            "{played} played",
            played = format::duration(playtime)
        ));
    }
    if let Some(query) = &smart {
        meta.push(sorted_by(query.sort));
    }
    meta.push(collection_updated(&collection));
    meta.retain(|item| !item.is_empty());
    let rules = smart.as_ref().map(describe_rules).unwrap_or_default();
    let failure = random_error().or_else(|| actions.error());
    let name = collection.name.clone();

    let play_random = {
        let library = library.clone();
        let id = collection.id.clone();
        move |_| {
            let Some(menus) = menus else { return };
            if *random_busy.peek() {
                return;
            }
            let library = library.clone();
            let id = id.clone();
            random_busy.set(true);
            random_error.set(None);
            spawn(async move {
                match change(pick_random(library, id)).await {
                    Ok(Some(game)) => menus.play(Target {
                        game_id: game.id,
                        title: game.title,
                        entry_id: None,
                    }),
                    Ok(None) => random_error.set(Some(t!("This collection has no games."))),
                    Err(report) => random_error.set(Some(report.summary)),
                }
                random_busy.set(false);
            });
        }
    };
    let all_ids = {
        let library = library.clone();
        let id = collection.id.clone();
        let smart = smart.clone();
        let all: AllIds = std::rc::Rc::new(move || {
            let library = library.clone();
            let id = id.clone();
            let smart = smart.clone();
            Box::pin(async move {
                change(async move {
                    match smart {
                        Some(query) => library.game_ids(&query).await,
                        None => library
                            .collection(&id)
                            .await
                            .map(|found| found.map(|found| found.game_ids).unwrap_or_default()),
                    }
                })
                .await
            })
        });
        AllIdsProp(all)
    };
    let on_deleted = EventHandler::new(move |()| {
        router.navigate(
            &Route::Collections {
                filter: CollectionFilter::All,
            }
            .href(),
        )
    });
    let choosing = selection.active();
    rsx! {
        div {
            "data-view": "collection",
            "data-kind": if smart.is_some() { "smart" } else { "manual" },
            class: "flex flex-1 flex-col",
            div {
                BackLink {}
            }
            section {
                "data-cs": "panel",
                class: "relative shrink-0 overflow-hidden rounded-panel border border-border bg-surface-raised p-8",
                CollectionMosaic {
                    covers: cover_urls(&collection),
                    class: "absolute inset-y-0 right-0 w-1/2 opacity-60 mask-l-from-30% mask-l-to-100%",
                }
                div { class: "relative max-w-xl",
                    div { class: "animate-enter",
                        KindBadge { smart: smart.is_some() }
                    }
                    h1 { class: "mt-3 animate-enter font-display text-4xl font-semibold tracking-wide [--i:1]",
                        "{collection.name}"
                    }
                    if !collection.description.is_empty() {
                        p { class: "mt-2 animate-enter text-fg-muted [--i:2]", "{collection.description}" }
                    }
                    div {
                        "data-part": "meta",
                        class: "mt-3 flex animate-enter flex-wrap gap-2 font-mono text-[11px] tracking-wide text-fg-muted uppercase [--i:3]",
                        for (i, item) in meta.into_iter().enumerate() {
                            span { key: "{item}", class: "flex gap-2",
                                if i > 0 {
                                    span { class: "text-accent-alt/60", "//" }
                                }
                                "{item}"
                            }
                        }
                    }
                    if smart.is_some() {
                        div { "data-part": "rules", class: "mt-5 animate-enter [--i:4]",
                            div { class: "mb-2 font-mono text-[11px] tracking-[0.2em] text-fg-subtle uppercase",
                                {t!("Matches all rules")}
                            }
                            div { class: "flex flex-wrap gap-2",
                                for rule in rules {
                                    span {
                                        key: "{rule.field}-{rule.op}-{rule.value}",
                                        "data-part": "rule",
                                        class: "inline-flex items-center gap-1.5 rounded-control border border-accent-alt/30 bg-surface-overlay/80 px-2.5 py-1 text-xs backdrop-blur",
                                        span { class: "text-fg-muted", "{rule.field}" }
                                        span { class: "font-mono text-accent-alt", "{rule.op}" }
                                        span { class: "font-medium", "{rule.value}" }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "mt-6 flex animate-enter items-center gap-3 [--i:5]",
                        if menus.is_some() {
                            div { "data-part": "primary-action",
                                PlainButton {
                                    "data-focus-default": "true",
                                    disabled: total == 0 || random_busy(),
                                    on_press: play_random,
                                    class: "chamfer relative flex items-center gap-2 bg-linear-to-r from-accent to-accent-strong px-6 py-2.5 font-display font-semibold tracking-wider text-accent-fg uppercase outline-none data-disabled:opacity-40 data-focus-visible:focus-ring-chamfer data-hovered:brightness-110",
                                    Svg { icon: SHUFFLE, class: "size-4" }
                                    {t!("Play Random")}
                                }
                            }
                        }
                        PlainButton {
                            on_press: {
                                let collection = collection.clone();
                                let open = if smart.is_some() { Open::Edit } else { Open::Add };
                                move |_| actions.open(open, collection.clone())
                            },
                            class: "flex items-center gap-2 rounded-control border border-border-strong bg-surface-overlay/70 px-5 py-2.5 text-sm outline-none backdrop-blur data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover",
                            if smart.is_some() {
                                Svg { icon: icons::PENCIL, class: "size-4" }
                                " "
                                {t!("Edit Rules")}
                            } else {
                                Svg { icon: icons::PLUS, class: "size-4" }
                                " "
                                {t!("Add Games")}
                            }
                        }
                        PlainButton {
                            "aria-label": t!(note = "The button that opens more actions", "More"),
                            "aria-haspopup": "menu",
                            "aria-expanded": if menu().is_some() { "true" } else { "false" },
                            on_mounted: move |event| trigger.mounted(event),
                            on_press: move |_| {
                                if let Some(anchor) = trigger.anchor() {
                                    menu.set(Some(anchor));
                                }
                            },
                            class: "rounded-control border border-border-strong bg-surface-overlay/70 p-2.5 outline-none backdrop-blur data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover",
                            Svg { icon: icons::ELLIPSIS, class: "size-4" }
                        }
                        if let Some(anchor) = menu() {
                            Menu {
                                anchor,
                                items: actions.items(&collection),
                                label: t!("{name} options", name = name.clone()),
                                return_to: trigger.node(),
                                on_action: {
                                    let collection = collection.clone();
                                    move |id: String| actions.run(&id, &collection)
                                },
                                on_close: move |_| menu.set(None),
                            }
                        }
                    }
                    if let Some(message) = failure {
                        p { role: "alert", class: "mt-3 text-sm text-danger", "{message}" }
                    }
                }
            }

            div { class: "mt-8 mb-3 flex items-center justify-between gap-3",
                h2 { class: "font-display text-lg font-semibold tracking-wide",
                    {t!(
                        note = "The heading over a collection's games, with how many",
                        "Games ({count})",
                        count = format::count(total as f64)
                    )}
                }
                if total > 0 {
                    Button {
                        "data-part": "select",
                        "aria-pressed": if choosing { "true" } else { "false" },
                        class: "flex items-center gap-1.5 rounded-control border border-border px-3 py-1.5 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg aria-pressed:border-accent-alt aria-pressed:text-fg",
                        on_press: move |_| if selection.active() { selection.stop() } else { selection.start() },
                        Svg { icon: icons::SQUARE_CHECK_BIG, class: "size-4" }
                        {t!(note = "Starts choosing several games in the library", "Select")}
                    }
                }
            }
            if let Some(message) = games_error {
                div { role: "alert", class: "rounded-panel border border-border p-8",
                    p { {t!("Could not load the games.")} }
                    p { class: "mt-2 text-sm text-fg-muted", "{message}" }
                }
            } else if total > 0 {
                VirtualLibrary {
                    query: ListGames::default(),
                    collection: collection.id.clone(),
                    // Its members changed here: no library event says so.
                    reload: super::revision(),
                    layout: Layout::Grid,
                    events: Events {
                        open: Some(EventHandler::new(move |id: String| router.open_game(&id))),
                    },
                }
                SelectionBar { selection, total: total as usize, all_ids }
            } else if counted {
                div {
                    "data-part": "empty",
                    class: "flex flex-col items-center gap-2 rounded-panel border border-dashed border-border-strong p-12 text-center text-fg-subtle",
                    Svg { icon: icons::SEARCH_X, class: "size-8" }
                    if smart.is_some() {
                        {t!("No games match these rules yet. They'll show up here as your library changes.")}
                    } else {
                        {t!("This collection is empty. Add games here, or from a game's menu.")}
                    }
                }
            }
            {actions.dialogs(Some(on_deleted))}
        }
    }
}

/// A random game from the collection, an installed one where it has any.
async fn pick_random(library: Library, id: String) -> Result<Option<GameSummary>, ErrorReport> {
    let total = library.collection_games(&id, 0, 1).await?.total as usize;
    if total == 0 {
        return Ok(None);
    }
    let mut at = random_below(total.saturating_sub(RANDOM_SAMPLE) + 1);
    let mut items: Vec<GameSummary> = Vec::new();
    // A page at a time: the library gives at most `PAGE_LIMIT`.
    while items.len() < RANDOM_SAMPLE && at < total {
        let page = library.collection_games(&id, at as u32, PAGE_LIMIT).await?;
        if page.items.is_empty() {
            break;
        }
        at += page.items.len();
        items.extend(page.items);
    }
    items.truncate(RANDOM_SAMPLE);
    let installed: Vec<GameSummary> = items
        .iter()
        .filter(|game| game.installed)
        .cloned()
        .collect();
    let mut pool = if installed.is_empty() {
        items
    } else {
        installed
    };
    if pool.is_empty() {
        return Ok(None);
    }
    let pick = random_below(pool.len());
    Ok(Some(pool.swap_remove(pick)))
}

/// A number below `n` (at least 1), not repeatable.
fn random_below(n: usize) -> usize {
    use std::hash::BuildHasher;
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or_default();
    let random = std::collections::hash_map::RandomState::new().hash_one(seed);
    (random % n.max(1) as u64) as usize
}
