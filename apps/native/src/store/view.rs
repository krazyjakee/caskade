//! The Store's page (the webview's `StoreView.tsx`): Browse, Recently
//! released and Upcoming as tabs, every view's state in the route, so Back
//! from a game's page comes back to it.

use std::time::Duration;

use caskade_core::igdb::{
    CatalogReleases, CatalogSearch, CatalogSort, IgdbCatalogFilters, IgdbCatalogPage,
};
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use dioxus_native::prelude::*;

use super::card::{CatalogCard, owned_copy};
use super::data::{Shelf, StoreData, use_store_data};
use super::releases::{Listed, listed_games};
use crate::home::ShelfRow;
use crate::input::{Bind, use_input_action};
use crate::router::{Route, StoreRoute, use_router};
use crate::ui::icons::{CALENDAR_CLOCK, SEARCH, SEARCH_X};
use crate::ui::{
    Button, ButtonSize, Link, Segment, SegmentedControl, Select, SelectOption, Svg, TextField,
};

const TABS: [&str; 3] = ["browse", "recent", "upcoming"];

/// Games per catalogue page.
pub const CATALOG_PAGE: u32 = 24;
/// Games on a shelf.
const SHELF_SIZE: u32 = 12;
/// Search waits this long after the last keystroke.
const DEBOUNCE: Duration = Duration::from_millis(350);
/// The filters' "any" choice.
const ANY: &str = "any";

fn tab_label(tab: &str) -> String {
    match tab {
        "recent" => t!(note = "A Store tab and shelf", "Recently released"),
        "upcoming" => t!(note = "A Store tab and shelf: releases to come", "Upcoming"),
        _ => t!(
            note = "A Store tab: search and filter the whole catalog",
            "Browse"
        ),
    }
}

fn sort_of(name: Option<&str>) -> CatalogSort {
    match name {
        Some("mostRated") => CatalogSort::MostRated,
        Some("popular") => CatalogSort::Popular,
        Some("newest") => CatalogSort::Newest,
        _ => CatalogSort::Relevance,
    }
}

fn sort_name(value: &str) -> Option<&'static str> {
    ["relevance", "mostRated", "popular", "newest"]
        .into_iter()
        .find(|name| *name == value)
}

/// The UTC day a year before today, `YYYY-MM-DD`.
fn year_ago() -> String {
    let today = chrono::Utc::now().date_naive();
    let day = today
        .checked_sub_months(chrono::Months::new(12))
        .unwrap_or(today);
    day.format("%Y-%m-%d").to_string()
}

fn ids(id: Option<&str>) -> Vec<u64> {
    id.and_then(|id| id.parse().ok()).into_iter().collect()
}

/// Browse's search for `store`, as the webview's `useCatalogSearch` asks.
pub fn search_for(store: &StoreRoute) -> CatalogSearch {
    let narrowed = store.q.is_some()
        || store.platform.is_some()
        || store.genre.is_some()
        || store.year.is_some();
    let past_year = !narrowed && matches!(store.sort, None | Some("relevance") | Some("mostRated"));
    CatalogSearch {
        query: store.q.clone(),
        sort: sort_of(store.sort),
        offset: 0,
        limit: CATALOG_PAGE,
        platforms: ids(store.platform.as_deref()),
        genres: ids(store.genre.as_deref()),
        released_from: match store.year {
            Some(year) => Some(format!("{year}-01-01")),
            None => past_year.then(year_ago),
        },
        released_to: store.year.map(|year| format!("{year}-12-31")),
    }
}

/// What a list of catalogue games asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Search(CatalogSearch),
    Releases(Shelf, CatalogReleases),
}

impl Request {
    fn at(&self, offset: u32) -> Self {
        match self {
            Self::Search(search) => Self::Search(CatalogSearch {
                offset,
                ..search.clone()
            }),
            Self::Releases(shelf, releases) => Self::Releases(
                *shelf,
                CatalogReleases {
                    offset,
                    ..releases.clone()
                },
            ),
        }
    }

    fn ask(&self, data: &StoreData) -> crate::game::launch::Answer<IgdbCatalogPage> {
        match self {
            Self::Search(search) => (data.search)(search.clone()),
            Self::Releases(shelf, releases) => data.releases(*shelf, releases.clone()),
        }
    }
}

#[component]
pub fn StoreView(store: StoreRoute) -> Element {
    let router = use_router();
    let go = move |next: StoreRoute| router.replace(&Route::Store(next).href());
    let at = TABS.iter().position(|tab| *tab == store.tab).unwrap_or(0);
    // LB/RB switch tabs.
    for (action, by) in [(Action::TabPrev, TABS.len() - 1), (Action::TabNext, 1)] {
        let store = store.clone();
        use_input_action(
            action,
            Bind {
                label: Some(if by == 1 {
                    t!("Next tab")
                } else {
                    t!("Previous tab")
                }),
                ..Bind::default()
            },
            move || {
                let tab = TABS[(at + by) % TABS.len()];
                go(StoreRoute {
                    tab,
                    ..store.clone()
                });
                true
            },
        );
    }
    rsx! {
        div { "data-view": "store", class: "mt-4",
            h1 { class: "font-display text-2xl font-semibold", {t!("Store")} }
            p { class: "mt-1 text-sm text-fg-muted",
                {t!("Every game IGDB knows, with where it is sold. Prices and availability are the stores' own.")}
            }
            div { role: "tablist", class: "mt-5 mb-4 flex flex-wrap gap-2",
                for tab in TABS {
                    {
                        let selected = tab == store.tab;
                        let next = StoreRoute { tab, ..store.clone() };
                        let href = Route::Store(next.clone()).href();
                        rsx! {
                            Link {
                                key: "{tab}",
                                href,
                                role: "tab",
                                "aria-selected": if selected { "true" } else { "false" },
                                "data-selected": selected.then_some("true"),
                                "data-focus-default": selected.then_some("true"),
                                class: "rounded-full border border-border bg-surface-raised px-4 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) data-hovered:text-fg data-focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg",
                                on_press: move |_| go(next.clone()),
                                {tab_label(tab)}
                            }
                        }
                    }
                }
            }
            if store.tab == "browse" {
                BrowseTab { store: store.clone() }
            } else {
                ReleasesTab { store: store.clone() }
            }
        }
    }
}

#[component]
fn BrowseTab(store: StoreRoute) -> Element {
    let router = use_router();
    let mut text = use_signal(|| store.q.clone().unwrap_or_default());
    // The query this field last put in the route: a different one came
    // from Back or a link, and the field follows it.
    let mut pushed = use_signal(|| store.q.clone());
    if *pushed.peek() != store.q {
        pushed.set(store.q.clone());
        text.set(store.q.clone().unwrap_or_default());
    }
    let mut typed = use_signal(|| 0u64);
    let submit = use_callback(move |value: String| {
        let q = Some(value.trim().to_owned()).filter(|q| !q.is_empty());
        let Route::Store(now) = router.peek() else {
            return;
        };
        if q == now.q {
            return;
        }
        pushed.set(q.clone());
        router.replace(&Route::Store(StoreRoute { q, ..now }).href());
    });
    let go = move |next: StoreRoute| router.replace(&Route::Store(next).href());
    let narrowed = store.q.is_some()
        || store.platform.is_some()
        || store.genre.is_some()
        || store.year.is_some();
    let query = store.q.clone();
    let heading = match (&query, store.sort) {
        (Some(query), _) => t!("Results for “{query}”", query = query.clone()),
        (None, Some("popular")) => t!("Popular games"),
        (None, Some("newest")) => t!("Newest games"),
        _ if narrowed => t!("Matching games"),
        _ => t!(
            note = "The Store's games released in the last 12 months with the most ratings",
            "Most rated this past year"
        ),
    };
    let empty = match &query {
        Some(query) => t!("No games match “{query}”.", query = query.clone()),
        None => t!("No games match these filters."),
    };
    let sort_value = match (store.sort, &store.q) {
        (Some("mostRated"), None) | (None, _) => "relevance",
        (Some(sort), _) => sort,
    };
    let mut sorts = vec![SelectOption::new(
        "relevance",
        if store.q.is_some() {
            t!("Relevance")
        } else {
            t!("Most rated")
        },
    )];
    if store.q.is_some() {
        sorts.push(SelectOption::new("mostRated", t!("Most rated")));
    }
    sorts.push(SelectOption::new("popular", tp!("catalog sort", "Popular")));
    sorts.push(SelectOption::new("newest", t!("Newest")));
    rsx! {
        div { class: "flex flex-wrap items-end gap-3",
            TextField {
                "aria-label": t!("Search the catalog"),
                placeholder: t!("Search every game…"),
                icon: SEARCH,
                kind: "search",
                value: text(),
                class: "min-w-64 flex-[1_1_20rem]",
                on_input: move |value: String| {
                    text.set(value.clone());
                    // Only the last keystroke's search goes to the server.
                    typed += 1;
                    let mine = *typed.peek();
                    spawn(async move {
                        let _ = caskade_app::runtime::spawn(tokio::time::sleep(DEBOUNCE)).await;
                        if *typed.peek() == mine {
                            submit.call(value);
                        }
                    });
                },
                on_submit: move |value: String| {
                    typed += 1;
                    submit.call(value);
                },
            }
            CatalogFilters { store: store.clone(), genre: true, year: true }
            Select {
                "aria-label": t!("Sort catalog games"),
                options: sorts,
                value: Some(sort_value.to_owned()),
                on_change: {
                    let store = store.clone();
                    move |sort: String| go(StoreRoute { sort: sort_name(&sort), ..store.clone() })
                },
            }
        }
        if !narrowed {
            ReleaseShelf { shelf: Shelf::Recent, store: store.clone() }
            ReleaseShelf { shelf: Shelf::Upcoming, store: store.clone() }
        }
        h2 { class: "mt-8 mb-4 font-display text-xl font-semibold", "{heading}" }
        CatalogGrid { request: Request::Search(search_for(&store)), empty }
    }
}

#[component]
fn ReleasesTab(store: StoreRoute) -> Element {
    let router = use_router();
    let go = move |next: StoreRoute| router.replace(&Route::Store(next).href());
    let shelf = if store.tab == "recent" {
        Shelf::Recent
    } else if store.tbd {
        Shelf::Tbd
    } else {
        Shelf::Upcoming
    };
    let request = Request::Releases(
        shelf,
        CatalogReleases {
            offset: 0,
            limit: CATALOG_PAGE,
            platforms: ids(store.platform.as_deref()),
            genres: ids(store.genre.as_deref()),
            tbd: shelf == Shelf::Tbd,
        },
    );
    let note = match shelf {
        Shelf::Recent => t!(
            "Releases dated to the day in the last 90 days (UTC), newest first, on each platform."
        ),
        Shelf::Tbd => t!("Announced releases without a date yet."),
        Shelf::Upcoming => t!(
            "Releases to come, soonest first. Month, quarter and year dates are as precise as IGDB knows them; cancelled ones are marked."
        ),
    };
    let empty = if shelf == Shelf::Recent {
        t!("No recent releases here.")
    } else {
        t!("No upcoming releases here.")
    };
    rsx! {
        div { class: "flex flex-wrap items-end gap-3",
            CatalogFilters { store: store.clone(), genre: true, year: false }
            if store.tab == "upcoming" {
                SegmentedControl {
                    "aria-label": t!("Upcoming releases"),
                    value: if store.tbd { "tbd" } else { "dated" },
                    options: vec![
                        Segment::new("dated", t!(note = "Upcoming releases with a release date", "Dated")),
                        Segment::new("tbd", t!(note = "A release date that isn't known yet (to be determined)", "TBD")),
                    ],
                    on_change: {
                        let store = store.clone();
                        move |value: String| go(StoreRoute { tbd: value == "tbd", ..store.clone() })
                    },
                }
            }
        }
        p { class: "mt-3 text-xs text-fg-subtle", "{note}" }
        div { class: "mt-4",
            CatalogGrid { request, empty }
        }
    }
}

/// The platform, genre and release year to narrow the catalogue by.
#[component]
fn CatalogFilters(store: StoreRoute, genre: bool, year: bool) -> Element {
    let router = use_router();
    let data = use_store_data();
    let filters = use_resource(move || (data.filters)());
    let loaded: Option<IgdbCatalogFilters> = filters.read().clone().and_then(Result::ok);
    let go = move |next: StoreRoute| router.replace(&Route::Store(next).href());
    let pick = |value: String| (value != ANY).then_some(value);
    let options = |list: &[caskade_core::igdb::IgdbFilterOption], any: String| {
        let mut options = vec![SelectOption::new(ANY, any)];
        options.extend(
            list.iter()
                .map(|option| SelectOption::new(option.id.to_string(), option.name.clone())),
        );
        options
    };
    let platforms = loaded
        .as_ref()
        .map(|loaded| loaded.platforms.clone())
        .unwrap_or_default();
    let genres = loaded
        .as_ref()
        .map(|loaded| loaded.genres.clone())
        .unwrap_or_default();
    let this_year = chrono::Datelike::year(&chrono::Utc::now().date_naive()) as u16;
    let mut years = vec![SelectOption::new(ANY, t!("Any year"))];
    years.extend(
        (1970..=this_year + 2)
            .rev()
            .map(|year| SelectOption::new(year.to_string(), year.to_string())),
    );
    rsx! {
        Select {
            "aria-label": t!(note = "The Store's platform filter", "Platform"),
            disabled: loaded.is_none(),
            class: "w-52",
            options: options(&platforms, t!("All platforms")),
            value: Some(store.platform.clone().unwrap_or_else(|| ANY.to_owned())),
            on_change: {
                let store = store.clone();
                move |value: String| go(StoreRoute { platform: pick(value), ..store.clone() })
            },
        }
        if genre {
            Select {
                "aria-label": t!(note = "The Store's genre filter", "Genre"),
                disabled: loaded.is_none(),
                class: "w-48",
                options: options(&genres, t!("All genres")),
                value: Some(store.genre.clone().unwrap_or_else(|| ANY.to_owned())),
                on_change: {
                    let store = store.clone();
                    move |value: String| go(StoreRoute { genre: pick(value), ..store.clone() })
                },
            }
        }
        if year {
            Select {
                "aria-label": t!("Release year"),
                class: "w-36",
                options: years,
                value: Some(store.year.map(|year| year.to_string()).unwrap_or_else(|| ANY.to_owned())),
                on_change: {
                    let store = store.clone();
                    move |value: String| go(StoreRoute { year: value.parse().ok(), ..store.clone() })
                },
            }
        }
    }
}

/// The library's copies of the games listed, by IGDB id.
fn use_owned(listed: &[Listed]) -> Vec<caskade_core::db::IgdbOwned> {
    let data = use_store_data();
    let ids: Vec<String> = listed.iter().map(|each| each.game.id.to_string()).collect();
    let owned = use_resource(use_reactive!(|ids| {
        let data = data.clone();
        async move {
            if ids.is_empty() {
                return Vec::new();
            }
            (data.owned)(ids).await.unwrap_or_default()
        }
    }));
    owned.read().clone().unwrap_or_default()
}

/// A list's pages, loaded one after another.
#[derive(Clone, PartialEq, Default)]
struct Pages {
    /// What they answer.
    request: Option<Request>,
    pages: Vec<IgdbCatalogPage>,
    /// The last load failed: the first page, or the next.
    failed: Option<String>,
    loading: bool,
}

/// A request's pages, the first loaded at once and more on asking; the last
/// list stays shown (busy) while a new request's first page loads.
fn use_pages(request: Request) -> (Signal<Pages>, Callback<()>, Callback<()>) {
    let data = use_store_data();
    let mut state = use_signal(Pages::default);
    let load = {
        let data = data.clone();
        use_callback(move |(request, offset): (Request, u32)| {
            let data = data.clone();
            state.write().loading = true;
            spawn(async move {
                let answer = request.at(offset).ask(&data).await;
                let mut state = state.write();
                // An answer for a request since replaced is dropped.
                if offset > 0 && state.request.as_ref() != Some(&request) {
                    return;
                }
                state.loading = false;
                match answer {
                    Ok(page) => {
                        if offset == 0 {
                            state.pages = vec![page];
                        } else {
                            state.pages.push(page);
                        }
                        state.request = Some(request);
                        state.failed = None;
                    }
                    Err(report) => {
                        if offset == 0 {
                            state.pages.clear();
                            state.request = Some(request);
                        }
                        state.failed = Some(report.summary);
                    }
                }
            });
        })
    };
    let mut asked = use_signal(|| None::<Request>);
    use_effect(use_reactive!(|request| {
        asked.set(Some(request.clone()));
        load.call((request, 0));
    }));
    let more = use_callback(move |()| {
        let state = state.peek().clone();
        let (Some(request), Some(offset)) = (
            state.request.clone(),
            state.pages.last().and_then(|page| page.next_offset),
        ) else {
            return;
        };
        load.call((request, offset));
    });
    let retry = use_callback(move |()| {
        if let Some(request) = asked.peek().clone() {
            load.call((request, 0));
        }
    });
    (state, more, retry)
}

/// Loading, failure and stale states shared by every catalogue list.
#[component]
fn Notices(state: Pages, current: bool, on_retry: EventHandler<()>) -> Element {
    let first_failed = current && state.pages.is_empty() && state.failed.is_some();
    rsx! {
        if state.request.is_none() || (!current && state.pages.is_empty()) {
            p { role: "status", class: "text-sm text-fg-muted", {t!("Loading games…")} }
        }
        if first_failed {
            div { role: "alert", class: "flex flex-wrap items-center gap-3 text-sm text-fg-muted",
                span {
                    match &state.failed {
                        Some(error) if !error.is_empty() => t!("Could not reach the game catalog: {error}", error = error.clone()),
                        _ => t!("Could not reach the game catalog."),
                    }
                }
                Button { size: ButtonSize::Sm, on_press: move |_| on_retry.call(()), {t!("Try again")} }
            }
        }
        if state.pages.iter().any(|page| page.stale) {
            p { role: "status", class: "mb-3 text-xs text-warning",
                {t!("Showing saved results: the metadata server can't be reached right now.")}
            }
        }
        if state.pages.iter().any(|page| page.sort_unavailable) {
            p { "data-part": "sort-unavailable", role: "status", class: "mb-3 text-xs text-fg-muted",
                {t!("The game catalog can't sort this way yet, so these are in its usual order.")}
            }
        }
    }
}

#[component]
fn CatalogGrid(request: Request, empty: String) -> Element {
    let (state, more, retry) = use_pages(request.clone());
    let pages = state();
    // The last list stays while the new one loads.
    let current = pages.request.as_ref() == Some(&request);
    let listed = listed_games(&pages.pages);
    let owned = use_owned(&listed);
    let has_next = current
        && pages
            .pages
            .last()
            .is_some_and(|page| page.next_offset.is_some());
    let more_failed = current && !pages.pages.is_empty() && pages.failed.is_some();
    rsx! {
        Notices { state: pages.clone(), current, on_retry: move |_| retry.call(()) }
        if current && pages.failed.is_none() && listed.is_empty() && pages.request.is_some() {
            div {
                "data-part": "empty",
                class: "flex flex-col items-center gap-2 rounded-card border border-dashed border-border py-16 text-center text-fg-muted",
                Svg { icon: SEARCH_X, class: "size-8 text-fg-subtle" }
                p { class: "text-sm", "{empty}" }
            }
        }
        if !listed.is_empty() {
            div {
                "data-catalog": "results",
                "aria-busy": (!current).then_some("true"),
                class: "grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 aria-busy:opacity-60",
                for (index, each) in listed.into_iter().enumerate() {
                    CatalogCard {
                        key: "{each.game.id}",
                        owned: owned_copy(&owned, each.game.id),
                        game: each.game,
                        release: each.release,
                        index: index % 24,
                    }
                }
            }
        }
        if more_failed {
            p { role: "alert", class: "mt-4 text-sm text-fg-muted", {t!("Could not load more games.")} }
        }
        if has_next {
            div { class: "mt-6 flex justify-center",
                Button {
                    disabled: pages.loading,
                    on_press: move |_| more.call(()),
                    if pages.loading { {t!("Loading…")} } else { {t!("Load more")} }
                }
            }
        }
    }
}

/// Browse's shelves of recent and upcoming releases, with a link to each
/// list.
#[component]
fn ReleaseShelf(shelf: Shelf, store: StoreRoute) -> Element {
    let router = use_router();
    let request = Request::Releases(
        shelf,
        CatalogReleases {
            offset: 0,
            limit: SHELF_SIZE,
            ..CatalogReleases::default()
        },
    );
    let (state, _, retry) = use_pages(request.clone());
    let pages = state();
    let current = pages.request.as_ref() == Some(&request);
    let mut listed = listed_games(&pages.pages);
    listed.truncate(SHELF_SIZE as usize);
    let owned = use_owned(&listed);
    if current && pages.failed.is_none() && listed.is_empty() {
        return rsx! {};
    }
    let (tab, id, icon) = match shelf {
        Shelf::Recent => ("recent", "store-recent", crate::home::CALENDAR_DAYS),
        _ => ("upcoming", "store-upcoming", CALENDAR_CLOCK),
    };
    let target = StoreRoute {
        tab,
        platform: store.platform.clone(),
        ..StoreRoute::default()
    };
    let href = Route::Store(target).href();
    rsx! {
        ShelfRow {
            id,
            title: tab_label(tab),
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
                Notices { state: pages.clone(), current, on_retry: move |_| retry.call(()) }
            },
            for each in listed {
                CatalogCard {
                    key: "{each.game.id}",
                    owned: owned_copy(&owned, each.game.id),
                    game: each.game,
                    release: each.release,
                    shelf: true,
                }
            }
        }
    }
}
