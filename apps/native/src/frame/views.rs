//! What each page shows, by route.

use crate::router::{LibraryRoute, Route, use_router};
use caskade_core::model::GameFilter;
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

/// The page the router shows.
#[component]
pub fn Outlet() -> Element {
    let router = use_router();
    let route = router.route();
    match route {
        Route::Home => rsx! {
            crate::home::HomeView {}
        },
        Route::Library(library) => rsx! {
            Library { library }
        },
        // Keyed in a list, so another game's page is a page of its own
        // (Dioxus keys only list items).
        Route::Game { id, from } => rsx! {
            for (id, from) in [(id, from)] {
                crate::game::GameView { key: "{id}", id, from }
            }
        },
        Route::StoreGame { igdb_id, from } => rsx! {
            for (igdb_id, from) in [(igdb_id, from)] {
                crate::store::StoreGameView { key: "{igdb_id}", igdb_id, from }
            }
        },
        Route::Activity { .. } => rsx! {
            super::notifications::Activity { route }
        },
        Route::Store(store) => rsx! {
            crate::store::StoreView { store }
        },
        Route::Collections { filter } => rsx! {
            crate::collections::CollectionsView { filter }
        },
        Route::Collection { id } => rsx! {
            for id in [id] {
                crate::collections::CollectionView { key: "{id}", id }
            }
        },
        Route::Settings { tab, section } => rsx! {
            crate::settings::SettingsView { tab, section }
        },
        Route::Plugin { plugin, view } => rsx! {
            crate::plugins::PluginViewPage { plugin, view }
        },
        Route::NotFound { .. } => placeholder(t!("Not found")),
    }
}

/// A page's name over "Not built yet.": what a page not found shows.
pub fn placeholder(title: String) -> Element {
    rsx! {
        div { class: "grid h-full flex-1 place-items-center",
            div { class: "text-center",
                h1 { class: "text-2xl font-semibold", "{title}" }
                p { class: "mt-2 text-fg-subtle", {t!("Not built yet.")} }
            }
        }
    }
}

/// The library's games for its route: the full view (filters bar,
/// selection, sorting) is #99's.
#[component]
fn Library(library: LibraryRoute) -> Element {
    let router = use_router();
    let stats = crate::data::use_library_stats();
    // With nothing played, the library opens on every game; choosing
    // Recently Played by name keeps it (`router.tsx`).
    let nothing_recent = library.opening
        && library.filter == GameFilter::Recent
        && matches!(&*stats.read(), Some(Ok(stats)) if stats.recent == 0);
    use_effect(use_reactive!(|nothing_recent| {
        if nothing_recent {
            let mut route = router.peek();
            if let Route::Library(library) = &mut route {
                library.filter = GameFilter::All;
            }
            router.replace(&route.href());
        }
    }));
    let mut library = library;
    if nothing_recent {
        library.filter = GameFilter::All;
    }
    rsx! {
        crate::library::LibraryView { library }
    }
}
