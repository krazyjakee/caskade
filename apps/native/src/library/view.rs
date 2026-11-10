//! The library view (#99, `views/library/LibraryView.tsx`): the toolbar
//! (sort, grid or list, the platform it's narrowed to, the search it shows),
//! the virtual grid or list of the route's games, the alphabet bar when it's
//! sorted by title, and what it says when there are none or they didn't
//! load. LB and RB step through the filters.

use super::{Events, Layout, VirtualLibrary};
use crate::data::{Query, use_library_page, use_query};
use crate::input::{Bind, use_input_action};
use crate::router::{LIBRARY_FILTERS, LibraryRoute, Route, RouteLink, use_router};
use crate::ui::{ErrorNotice, Select, SelectOption, Svg, icons};
use caskade_core::model::TitleLetter;
use caskade_core::model::{GameFilter, GameSort, ListGames};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use dioxus_native::prelude::*;

/// The fewest games that get the alphabet bar.
const ALPHABET_BAR_MIN_GAMES: u64 = 50;

/// The sorts, in the menu's order, with their names.
fn sorts() -> [(GameSort, &'static str, String); 6] {
    [
        (
            GameSort::Recent,
            "recent",
            tp!("library sort", "Recently played"),
        ),
        (GameSort::Title, "title", tp!("library sort", "Title")),
        (GameSort::Popular, "popular", tp!("library sort", "Popular")),
        (
            GameSort::Playtime,
            "playtime",
            tp!("library sort", "Playtime"),
        ),
        (
            GameSort::Added,
            "added",
            tp!("library sort", "Recently added"),
        ),
        (
            GameSort::Released,
            "released",
            tp!("library sort", "Release date"),
        ),
    ]
}

/// The library's href with `change` made to `library`.
pub fn library_href(library: &LibraryRoute, change: impl FnOnce(&mut LibraryRoute)) -> String {
    let mut next = library.clone();
    next.opening = false;
    change(&mut next);
    Route::Library(next).href()
}

/// Where each title letter starts in the games `query` lists, for the
/// alphabet bar.
fn use_title_letters(query: ReadSignal<ListGames>) -> Query<Vec<TitleLetter>> {
    use_query(DataKey::Summary, move |library| {
        let query = query();
        async move { library.letters(&query).await }
    })
}

#[component]
pub fn LibraryView(library: LibraryRoute) -> Element {
    let router = use_router();
    let web = library.filter == GameFilter::Web;
    let query = ListGames {
        filter: library.filter,
        sort: library.sort,
        query: library.q.clone(),
        platform: library.platform.clone(),
        ..ListGames::default()
    };
    // How many games there are, and whether they load.
    let mut counted = use_signal(|| ListGames {
        limit: 1,
        ..query.clone()
    });
    if counted.peek().filter != query.filter
        || counted.peek().sort != query.sort
        || counted.peek().query != query.query
        || counted.peek().platform != query.platform
    {
        counted.set(ListGames {
            limit: 1,
            ..query.clone()
        });
    }
    let count = use_library_page(counted.into());
    let (total, error) = match &*count.read() {
        Some(Ok(page)) => (Some(page.total), None),
        Some(Err(error)) => (None, Some(error.clone())),
        None => (None, None),
    };
    let alphabet = !web
        && library.sort == GameSort::Title
        && total.is_some_and(|total| total >= ALPHABET_BAR_MIN_GAMES);
    let letters = use_title_letters(counted.into());
    let letters = if alphabet {
        letters
            .read()
            .as_ref()
            .and_then(|letters| letters.as_ref().ok().cloned())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // LB and RB step through the filters, in place.
    let step = move |by: isize| {
        let Route::Library(library) = router.peek() else {
            return false;
        };
        let at = LIBRARY_FILTERS
            .iter()
            .position(|&filter| filter == library.filter)
            .unwrap_or(0) as isize;
        let count = LIBRARY_FILTERS.len() as isize;
        let next = LIBRARY_FILTERS[((at + by + count) % count) as usize];
        router.replace(&library_href(&library, |library| library.filter = next));
        true
    };
    use_input_action(
        Action::TabPrev,
        Bind {
            label: Some(t!("Previous tab")),
            ..Bind::default()
        },
        move || step(-1),
    );
    use_input_action(
        Action::TabNext,
        Bind {
            label: Some(t!("Next tab")),
            ..Bind::default()
        },
        move || step(1),
    );

    let heading = match &library.q {
        Some(q) => t!("Results for “{query}”", query = q),
        None => t!("Library"),
    };
    let sort_name = sorts()
        .into_iter()
        .find(|(sort, ..)| *sort == library.sort)
        .map(|(_, name, _)| name.to_owned());
    let sort_options: Vec<SelectOption> = sorts()
        .into_iter()
        .map(|(_, name, label)| SelectOption::new(name, label))
        .collect();
    let on_sort = {
        let library = library.clone();
        move |name: String| {
            if let Some((sort, ..)) = sorts().into_iter().find(|(_, id, _)| *id == name) {
                router.navigate(&library_href(&library, |library| library.sort = sort));
            }
        }
    };
    let control = "flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none hover:text-fg data-focus-visible:focus-ring";
    rsx! {
        div { "data-view": "library", class: "relative flex flex-1 flex-col",
            h1 { class: "sr-only", "{heading}" }
            div { "data-part": "toolbar", class: "mt-4 mb-4 flex flex-wrap items-center gap-2",
                // Install from a file, a folder or a link, receive a share,
                // or add a game by hand (#108); a game added by hand opens.
                if !web {
                    crate::flows::add_game::AddGame { on_open_game: move |id: String| router.open_game(&id) }
                }
                if let Some(platform) = &library.platform {
                    div { class: "flex items-center gap-2",
                        RouteLink {
                            to: library_href(&library, |library| library.platform = None),
                            current: false,
                            "data-part": "platform-back",
                            class: control,
                            Svg { icon: icons::ARROW_LEFT, class: "size-4" }
                            {t!(note = "Goes back from one platform's games to the list of platforms", "All platforms")}
                        }
                        span {
                            "data-part": "platform",
                            title: "{platform}",
                            class: "flex items-center gap-1.5 px-1 text-sm font-semibold text-fg",
                            "{platform}"
                        }
                    }
                }
                if let Some(q) = &library.q {
                    RouteLink {
                        to: library_href(&library, |library| library.q = None),
                        current: false,
                        "data-part": "clear-search",
                        class: "flex items-center gap-1.5 rounded-full border border-accent-alt py-1 pr-2 pl-3 text-sm text-fg outline-none data-focus-visible:focus-ring",
                        {t!("Results for “{query}”", query = q)}
                        span { "aria-label": t!("Clear search"), role: "img",
                            Svg { icon: icons::X, class: "size-4" }
                        }
                    }
                }
                div { class: "ml-auto flex flex-wrap items-center gap-2",
                    Select {
                        "aria-label": t!("Sort games"),
                        "data-part": "sort",
                        options: sort_options,
                        value: sort_name,
                        on_change: on_sort,
                    }
                    if !web {
                        for (mode, name) in [(Layout::Grid, "grid"), (Layout::List, "list")] {
                            RouteLink {
                                key: "{name}",
                                to: library_href(&library, |library| library.layout = mode),
                                current: mode == library.layout,
                                "aria-label": if mode == Layout::Grid { t!("grid view") } else { t!("list view") },
                                "data-part": "layout",
                                "data-selected": (mode == library.layout).then_some("true"),
                                class: "rounded-control border border-border p-2 text-fg-muted data-selected:text-accent-alt data-focus-visible:focus-ring",
                                Svg { icon: if mode == Layout::Grid { icons::GRID_2X2 } else { icons::LIST }, class: "size-4" }
                            }
                        }
                    }
                }
            }
            // A failed count with nothing shown says so; a refetch that
            // fails keeps the games already there.
            if let Some(error) = error {
                ErrorNotice {
                    title: t!("Couldn't load your library"),
                    summary: error.summary.clone(),
                    actions: rsx! {
                        crate::ui::Button {
                            on_press: move |_| {
                                let mut count = count;
                                count.restart();
                            },
                            {t!("Try again")}
                        }
                    },
                }
            } else if total.is_some_and(|total| total > 0) {
                VirtualLibrary {
                    query,
                    layout: library.layout,
                    letters,
                    sorting: library.clone(),
                    events: Events {
                        open: Some(EventHandler::new(move |id: String| router.open_game(&id))),
                    },
                }
            } else if total == Some(0) {
                Empty { library: library.clone() }
            }
        }
    }
}

/// What the library says with no games to show.
#[component]
fn Empty(library: LibraryRoute) -> Element {
    let message = if library.q.is_some() {
        t!("No games match your search in this filter.")
    } else if library.filter == GameFilter::Recent {
        t!("You haven't played anything yet.")
    } else if library.filter == GameFilter::Web {
        t!("No web games yet. Add one with Add web game.")
    } else {
        t!("No games in this filter yet.")
    };
    let all = library_href(&library, |library| library.filter = GameFilter::All);
    rsx! {
        div {
            "data-part": "empty",
            class: "flex flex-col items-center gap-2 rounded-card border border-dashed border-border py-16 text-fg-muted",
            Svg { icon: icons::SEARCH_X, class: "size-8" }
            p { "{message}" }
            if library.filter != GameFilter::All {
                RouteLink { to: all, current: false, class: "text-sm text-accent-alt", {t!("Show all games")} }
            }
        }
    }
}
