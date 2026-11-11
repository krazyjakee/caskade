//! A game page's way back (#195, the webview's `views/game/BackLink.tsx`
//! and `origin.ts`'s `returnPlace`): to the page the game was opened from,
//! named after it ("Back to Library", a collection's or a game's name), or
//! to the Library.

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
) -> Element {
    let router = use_router();
    let target = back_target(&game_id, from.as_deref(), LIBRARY_GAMES);
    let route = target
        .as_deref()
        .map(|href| Route::parse(href, &Remembered::default()));
    let href = target.unwrap_or_else(|| "/library".to_owned());
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
                None => rsx! { {back_to(Some(t!("Library")))} },
                Some(Route::Game { id, .. }) => rsx! { GameName { id } },
                Some(Route::Collection { id }) => rsx! { CollectionName { id } },
                Some(other) => rsx! { {back_to(page_name(&other))} },
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
