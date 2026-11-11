//! The library view (#99, `views/library/LibraryView.tsx`): the toolbar
//! (sort, grid or list, the platform it's narrowed to, the search it shows),
//! the virtual grid or list of the route's games, the alphabet bar when it's
//! sorted by title, and what it says when there are none or they didn't
//! load. LB and RB step through the filters.

use super::collection_dialog::{CollectionDialog, smart_preset};
use super::platform_grid::{PlatformGrid, breakdown};
use super::selection::use_selection_provider;
use super::selection_bar::{AllIdsProp, SelectionBar};
use super::{Events, Layout, VirtualLibrary};
use crate::data::{Query, use_library, use_library_page, use_query};
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

/// Installs a file dropped on the library: asks for its install, which
/// opens the install confirmation, or says why it can't.
pub type DropFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;

/// What installs dropped files: a test's, else the running app's install
/// requests.
#[derive(Clone)]
pub struct DropInstall(pub std::rc::Rc<dyn Fn(String) -> DropFuture>);

impl DropInstall {
    /// The running app's; `None` where it can't install (no drop zone then,
    /// as the webview has it).
    fn of_app() -> Option<Self> {
        crate::shell::services()?.install_requests.as_ref()?;
        Some(Self(std::rc::Rc::new(|path: String| {
            Box::pin(async move {
                caskade_app::runtime::spawn(async move {
                    let requests = crate::shell::services()
                        .and_then(|services| services.install_requests.as_ref())
                        .ok_or_else(|| t!("Could not add the dropped file."))?;
                    caskade_app::commands::install_requests::request_dropped_install(
                        &crate::shell::sink(),
                        requests,
                        path,
                    )
                    .await
                    .map(drop)
                })
                .await
                .unwrap_or_else(|_| Err(t!("Could not add the dropped file.")))
            })
        })))
    }
}

/// Files dropped on the library install (`LibraryDrop.tsx`): a shortcut, a
/// game's program or folder, an installer or an archive. The core works out
/// what each installs and the install confirmation opens with it. Whether
/// files are over the window now, and why the last drop failed.
fn use_library_drop() -> (Signal<bool>, Signal<Option<String>>) {
    let mut over = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    use_hook(move || {
        let Some(install) = try_consume_context::<DropInstall>().or_else(DropInstall::of_app)
        else {
            return;
        };
        let mut drops = crate::drops::drops().subscribe();
        spawn(async move {
            loop {
                let event = match drops.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                match event {
                    crate::drops::DropEvent::Over => over.set(true),
                    crate::drops::DropEvent::Left => over.set(false),
                    crate::drops::DropEvent::Dropped(paths) => {
                        over.set(false);
                        error.set(None);
                        for path in paths {
                            if let Err(message) = (install.0)(path.display().to_string()).await {
                                error.set(Some(message));
                            }
                        }
                    }
                }
            }
        });
    });
    (over, error)
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
    // Emulated's games by platform, when they span several.
    let platforms = use_query(DataKey::Summary, move |data| {
        let query = counted();
        async move {
            let wanted = query.filter == GameFilter::Emulated
                && query.platform.is_none()
                && query.query.is_none();
            if wanted {
                data.platforms(GameFilter::Emulated).await
            } else {
                Ok(Vec::new())
            }
        }
    });
    let platforms = platforms
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .and_then(|list| breakdown(&library, list));
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
    let (drop_over, mut drop_error) = use_library_drop();
    // Save as collection: the view's filter, sort, search and platform as
    // a smart collection's rules.
    let mut saving = use_signal(|| false);
    // Choosing several games for one edit (the webview's #173). Changing
    // the filter, sort or search keeps the games chosen.
    let mut selection = use_selection_provider();
    let selectable = !web && total.is_some_and(|total| total > 0);
    let choosing = selection.active();
    use_input_action(
        Action::Back,
        Bind {
            label: choosing.then(|| t!("Done selecting")),
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
    let all_ids = {
        let data = use_library();
        let query = query.clone();
        let all: super::selection_bar::AllIds = std::rc::Rc::new(move || {
            let data = data.clone();
            let query = query.clone();
            Box::pin(async move {
                crate::data::change(async move { data.game_ids(&query).await }).await
            })
        });
        AllIdsProp(all)
    };
    let control = "flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none hover:text-fg data-focus-visible:focus-ring";
    rsx! {
        div { "data-view": "library", class: "relative flex flex-1 flex-col",
            if saving() {
                CollectionDialog {
                    preset: caskade_core::model::CollectionKind::Smart { query: smart_preset(&query) },
                    on_close: move |_| saving.set(false),
                    on_saved: move |id: String| router.navigate(&Route::Collection { id }.href()),
                }
            }
            h1 { class: "sr-only", "{heading}" }
            if drop_over() {
                div {
                    "data-part": "drop-hint",
                    class: "pointer-events-none fixed inset-x-0 bottom-10 z-40 flex justify-center",
                    p { class: "flex items-center gap-2 rounded-full border border-accent-strong bg-surface-overlay px-5 py-2 text-sm shadow-lg",
                        Svg { icon: icons::FILE_PLUS_CORNER, class: "size-4 text-accent-alt" }
                        {t!("Drop to install into your library")}
                    }
                }
            }
            if let Some(error) = drop_error() {
                p {
                    role: "alert",
                    "data-part": "drop-error",
                    class: "mt-4 flex items-start justify-between gap-3 rounded-control border border-danger p-3 text-sm text-danger",
                    "{error}"
                    crate::ui::Button {
                        "aria-label": t!("Dismiss"),
                        class: "shrink-0",
                        on_press: move |_| drop_error.set(None),
                        Svg { icon: icons::X, class: "size-4" }
                    }
                }
            }
            div { "data-part": "toolbar", class: "mt-4 mb-4 flex flex-wrap items-center gap-2",
                if selectable {
                    crate::ui::Button {
                        "data-part": "select",
                        "aria-pressed": if choosing { "true" } else { "false" },
                        class: "flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg aria-pressed:border-accent-alt aria-pressed:text-fg",
                        on_press: move |_| if selection.active() { selection.stop() } else { selection.start() },
                        Svg { icon: icons::SQUARE_CHECK_BIG, class: "size-4" }
                        {t!(note = "Starts choosing several games in the library", "Select")}
                    }
                }
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
                    crate::ui::Button {
                        "data-part": "save-collection",
                        class: "flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg",
                        on_press: move |_| saving.set(true),
                        Svg { icon: icons::LIST_PLUS, class: "size-4" }
                        {t!("Save as collection")}
                    }
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
            } else if let Some(platforms) = platforms {
                PlatformGrid { platforms, library: library.clone() }
            } else if let Some(total) = total.filter(|&total| total > 0) {
                VirtualLibrary {
                    query,
                    layout: library.layout,
                    letters,
                    sorting: library.clone(),
                    events: Events {
                        open: Some(EventHandler::new(move |id: String| router.open_game(&id))),
                    },
                }
                SelectionBar { selection, total: total as usize, all_ids }
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
