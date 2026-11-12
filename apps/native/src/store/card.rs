//! A catalogue game in the Store (the webview's `views/store/CatalogCard`):
//! its cover, title and platforms, the release that listed it (recent and
//! upcoming) and, apart from those, its place in the library when it has
//! one. Its `data-status` is the library's: absent for a game the library
//! doesn't have, because a store link says nothing of what it costs or
//! whether it is on sale. One focus stop (`role="link"`): a click, Enter or
//! a pad's A opens its page.

use caskade_core::db::IgdbOwned;
use caskade_core::igdb::{IgdbGame, IgdbReleaseDate};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use super::releases::{ReleaseKind, platforms_line, release_kind, release_when, release_where};
use crate::router::{Route, use_router};
use crate::ui::GameArt;
use crate::ui::icons::{CIRCLE_CHECK, Icon, Svg};
use crate::ui::interact::{Keys, use_interaction};

/// Lucide's `library`: a store game the library has.
pub const LIBRARY: Icon = Icon(&["m16 6 4 14", "M12 6v14", "M8 8v12", "M4 4v16"]);

/// The library's copy among `owned` for IGDB game `id`: an installed one
/// wins over one that isn't.
pub fn owned_copy(owned: &[IgdbOwned], id: u64) -> Option<IgdbOwned> {
    owned
        .iter()
        .filter(|copy| copy.igdb_id == id)
        .max_by_key(|copy| copy.installed)
        .cloned()
}

/// Where a catalogue game's page is, coming from the page shown now.
pub fn open_store_game(router: crate::router::Router, id: u64) {
    let from = crate::router::href::valid_return(Some(&router.peek().href()));
    router.navigate(
        &Route::StoreGame {
            igdb_id: id.to_string(),
            from,
        }
        .href(),
    );
}

#[component]
pub fn CatalogCard(
    game: IgdbGame,
    /// The release that listed the game, on the release lists.
    #[props(default)]
    release: Option<IgdbReleaseDate>,
    /// Its library game, when the library has it.
    #[props(default)]
    owned: Option<IgdbOwned>,
    /// A grid sizes the art by its aspect; a shelf row fills its height.
    #[props(default)]
    shelf: bool,
    /// Its place in its grid: the enter animation's stagger.
    #[props(default)]
    index: usize,
) -> Element {
    let router = use_router();
    let interaction = use_interaction();
    let id = game.id;
    let open = move || open_store_game(router, id);
    let status = owned.as_ref().map(|copy| {
        if copy.installed {
            "installed"
        } else {
            "not-installed"
        }
    });
    let kind = release.as_ref().and_then(release_kind);
    let place = match &release {
        Some(release) => release_where(release),
        None => platforms_line(&game),
    };
    let year = game
        .release_date
        .as_deref()
        .and_then(|date| date.get(..4))
        .map(str::to_owned);
    let mut when = match &release {
        Some(release) => release_when(release),
        None => year.unwrap_or_else(|| {
            t!(
                note = "A release date that isn't known yet (to be determined)",
                "TBD"
            )
        }),
    };
    match kind {
        Some(ReleaseKind::Cancelled) => {
            when.push_str(&format!(
                " · {}",
                t!(note = "A game release's status", "Cancelled")
            ));
        }
        Some(ReleaseKind::EarlyAccess) => {
            when.push_str(&format!(
                " · {}",
                t!(note = "A game release's status", "Early access")
            ));
        }
        _ => {}
    }
    let cover = game.images.cover.as_ref().map(|cover| cover.url.clone());
    let href = Route::StoreGame {
        igdb_id: id.to_string(),
        from: None,
    }
    .href();
    rsx! {
        div {
            role: "link",
            tabindex: "0",
            "aria-label": "{game.title}",
            "data-href": "{href}",
            "data-cs": "game-card",
            "data-variant": "store",
            "data-status": status,
            "data-focus-key": "catalog-card:{id}",
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            style: "--i: {index};",
            class: "group relative flex h-full min-w-0 animate-enter flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none transition-colors duration-(--cs-motion-duration-fast) data-hovered:bg-surface-hover data-focus-visible:focus-ring-over",
            onfocusin: move |_| interaction.focus(),
            onfocusout: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onclick: move |_| open(),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::Enter, open);
            },
            GameArt {
                part: "art",
                image: cover,
                class: if shelf { "min-h-0 w-full flex-1" } else { "aspect-(--cs-card-aspect) w-full" },
            }
            div { class: "min-w-0 shrink-0 px-3 py-2.5",
                p { "data-part": "title", class: "truncate text-sm font-semibold", "{game.title}" }
                if !place.is_empty() {
                    p { "data-part": "subtitle", class: "truncate text-xs text-fg-subtle", "{place}" }
                }
                p { "data-part": "meta", class: "truncate font-mono text-[11px] text-fg-muted", "{when}" }
            }
            if let Some(copy) = owned {
                span {
                    "data-part": "owned",
                    class: "absolute top-2 left-2 inline-flex items-center gap-1 rounded-full bg-surface-base/85 px-2 py-0.5 text-[11px] font-medium text-status-installed",
                    Svg { icon: if copy.installed { CIRCLE_CHECK } else { LIBRARY }, class: "size-3" }
                    if copy.installed {
                        {caskade_ui_i18n::tp!("game status", "Installed")}
                    } else {
                        {t!(note = "A store game the library has but isn't installed", "In library")}
                    }
                }
            }
        }
    }
}
