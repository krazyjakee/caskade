//! A game page's way back (#195, the webview's `views/game/BackLink.tsx`
//! and `origin.ts`'s `returnPlace`): to the page the game was opened from,
//! named after it ("Back to Library", a collection's, a game's or a Store
//! game's name), or to the Library (the Store, for a Store game's page).

use crate::data::{use_game, use_query};
use crate::router::href::back_target;
use crate::router::route::Remembered;
use crate::router::{Route, use_router};
use crate::ui::Link;
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

/// Where library game pages live.
pub const LIBRARY_GAMES: &str = "/games";
/// Where the Store's catalogue game pages live (#102).
pub const STORE_GAMES: &str = "/store/games";
/// Where Back goes from a library game opened from nowhere.
const LIBRARY: &str = "/library";

fn back_to(name: Option<String>) -> String {
    match name {
        Some(name) => t!(
            note = "{name} is the page, collection or game the game page was opened from",
            "Back to {name}",
            name = name
        ),
        None => t!(
            note = "The game page's link back to where it was opened from",
            "Back"
        ),
    }
}

/// A page by the sidebar's name for it.
fn page_name(route: &Route) -> Option<String> {
    match route {
        Route::Home => Some(tp!(
            note = "The sidebar link to the home view",
            "navigation",
            "Home"
        )),
        Route::Library(_) => Some(t!("Library")),
        Route::Store(_) => Some(t!("Store")),
        Route::Collections { .. } => Some(t!("Collections")),
        Route::Activity { .. } => Some(t!("Activity")),
        _ => None,
    }
}

#[component]
pub fn GameBackLink(
    game_id: String,
    /// The game route's `from`.
    from: Option<String>,
    #[props(default)] class: String,
    /// Where the page lives: library games, or the Store's
    /// ([`STORE_GAMES`]).
    #[props(default = LIBRARY_GAMES)]
    base: &'static str,
    /// Where Back goes without a `from`.
    #[props(default = LIBRARY)]
    fallback: &'static str,
) -> Element {
    let router = use_router();
    let href = back_target(&game_id, from.as_deref(), base).unwrap_or_else(|| fallback.to_owned());
    let route = Route::parse(&href, &Remembered::default());
    rsx! {
        Link {
            href: href.clone(),
            class,
            "data-part": "back",
            on_press: move |_| {
                // A step back when the page before is the one it was
                // opened from, so it comes back as it was left; else the
                // page, in this one's place, so Back and the page never
                // bounce between each other (`origin.ts`'s `useGoBack`).
                let wanted = Route::parse(&href, &Remembered::default()).href();
                if router.previous().is_some_and(|previous| previous.href() == wanted) {
                    router.back();
                } else {
                    router.replace(&href);
                }
            },
            span { "aria-hidden": "true", "← " }
            match route {
                Route::Game { id, .. } => rsx! { GameName { id } },
                Route::Collection { id } => rsx! { CollectionName { id } },
                Route::StoreGame { igdb_id, .. } => rsx! { StoreGameName { id: igdb_id } },
                other => rsx! { {back_to(page_name(&other))} },
            }
        }
    }
}

#[component]
fn GameName(id: String) -> Element {
    let game = use_game(id);
    let title = game
        .read()
        .as_ref()
        .and_then(|game| game.as_ref().ok().cloned().flatten())
        .map(|game| game.game.title);
    rsx! { {back_to(title)} }
}

#[component]
fn CollectionName(id: String) -> Element {
    let collections = use_query(DataKey::Collections, |library| async move {
        library.collections().await
    });
    let name = collections
        .read()
        .as_ref()
        .and_then(|list| list.as_ref().ok())
        .and_then(|list| list.iter().find(|collection| collection.id == id))
        .map(|collection| collection.name.clone());
    rsx! { {back_to(name)} }
}

/// A Store game's name, from the catalogue.
#[component]
fn StoreGameName(id: String) -> Element {
    let data = crate::store::data::use_store_data();
    let lookup = use_resource(move || (data.games)(vec![id.clone()]));
    let title = lookup
        .read()
        .as_ref()
        .and_then(|lookup| lookup.as_ref().ok())
        .and_then(|lookup| lookup.games.first())
        .map(|game| game.title.clone());
    rsx! { {back_to(title)} }
}
