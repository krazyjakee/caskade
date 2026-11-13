//! A catalogue game's page in the Store (#102, the webview's
//! `views/store/StoreGameView.tsx` and `WhereToBuy.tsx`): what IGDB knows
//! of it, where it is sold, its releases on each platform and region, and
//! similar games. Opening it creates no library entry. A game the library
//! has (the same IGDB match) links to its library page.

use std::collections::HashSet;

use caskade_core::db::IgdbOwned;
use caskade_core::error::ErrorReport;
use caskade_core::igdb::{IgdbGame, IgdbLookup, IgdbStoreLink, IgdbWebsite};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{t, tp};
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::card::{CatalogCard, LIBRARY, owned_copy};
use super::data::use_store_data;
use super::releases::{
    platforms_line, region_name, release_kind, release_when, sorted_releases, status_name,
};
use crate::game::data::use_answer;
use crate::game::launch::Answer;
use crate::game::{
    AgeRatingLabel, ExternalLink, GameBackLink, STORE_GAMES, Screenshots, Shot, Videos,
};
use crate::router::{Route, use_router};
use crate::ui::icons::{EXTERNAL_LINK, SPARKLES};
use crate::ui::{GameArt, Link, Svg};

/// Similar games on the shelf, at most.
const SIMILAR: usize = 12;

/// Where Back goes from a catalogue game opened from nowhere.
const STORE: &str = "/store";

/// A release's platform or region IGDB doesn't give.
const UNKNOWN: &str = "—";

/// IGDB's sizes of a screenshot: the full one, and the row's.
const SHOT_FULL: &str = "/t_screenshot_huge/";
const SHOT_THUMB: &str = "/t_screenshot_med/";

/// Opens an `http(s)` page in the browser, outside the app.
fn open_url(url: &str) {
    if let Ok(url) = url::Url::parse(url) {
        caskade_app::navigation::open_in_browser(&url);
    }
}

/// What the catalogue said of the game asked for.
enum Lookup {
    Loading,
    Failed(String),
    Found(Box<IgdbGame>),
    NotFound,
    /// Neither the game nor that IGDB has none: the metadata server is
    /// still fetching it.
    Nothing,
}

fn looked_up(answer: Option<&Result<IgdbLookup, ErrorReport>>, id: &str) -> Lookup {
    match answer {
        None => Lookup::Loading,
        Some(Err(report)) => Lookup::Failed(report.summary.clone()),
        Some(Ok(lookup)) => match lookup.games.first() {
            Some(game) => Lookup::Found(Box::new(game.clone())),
            None if lookup
                .not_found
                .iter()
                .any(|missing| missing.to_string() == id) =>
            {
                Lookup::NotFound
            }
            None => Lookup::Nothing,
        },
    }
}

/// The page of IGDB game `igdb_id` (`/store/games/<id>?from=…`).
#[component]
pub fn StoreGameView(igdb_id: String, from: Option<String>) -> Element {
    let data = use_store_data();
    let lookup = use_resource(use_reactive!(|igdb_id| {
        let data = data.clone();
        async move { (data.games)(vec![igdb_id]).await }
    }));
    let shown = looked_up(lookup.read().as_ref(), &igdb_id);
    let back = rsx! {
        GameBackLink {
            game_id: igdb_id.clone(),
            from: from.clone(),
            base: STORE_GAMES,
            fallback: STORE,
            class: "text-sm text-fg-muted",
        }
    };
    let missing = match shown {
        Lookup::Found(game) => {
            return rsx! {
                article { "data-view": "store-game", class: "mt-4",
                    {back}
                    // Keyed, so another game's page is a page of its own.
                    for game in [*game] {
                        CatalogGame { key: "{game.id}", game }
                    }
                }
            };
        }
        missing => missing,
    };
    rsx! {
        article { "data-view": "store-game", class: "mt-4",
            {back}
            div { class: "mt-6 text-sm text-fg-muted",
                match missing {
                    Lookup::Loading => rsx! {
                        p { role: "status", {t!("Loading game…")} }
                    },
                    Lookup::Failed(reason) => rsx! {
                        p { role: "alert", {t!("Could not reach the game catalog: {reason}", reason = reason)} }
                    },
                    Lookup::NotFound => rsx! {
                        p { role: "alert", {t!("IGDB has no game {igdbId}.", igdbId = igdb_id.clone())} }
                    },
                    _ => rsx! {
                        p { role: "alert", {t!("The metadata server sent nothing for this game yet. Try again later.")} }
                    },
                }
            }
        }
    }
}

#[component]
fn CatalogGame(game: IgdbGame) -> Element {
    let data = use_store_data();
    let router = use_router();
    let id = game.id;
    // The library's copies come again when the library changes.
    let owned = use_answer(DataKey::Games, move || (data.owned)(vec![id.to_string()]));
    // Its library copy: whether installed, its id and its page.
    let library = owned
        .read()
        .as_ref()
        .and_then(|owned| owned.as_ref().ok())
        .and_then(|owned| owned_copy(owned, id))
        .map(|copy| {
            let href = Route::Game {
                id: copy.game_id.clone(),
                from: None,
            }
            .href();
            (copy.installed, copy.game_id, href)
        });
    let art = game
        .images
        .hero
        .as_ref()
        .or(game.images.screenshots.first())
        .map(|image| image.url.clone());
    let cover = game.images.cover.as_ref().map(|cover| cover.url.clone());
    let year = game
        .release_date
        .as_deref()
        .and_then(|date| date.get(..4))
        .map(str::to_owned);
    let meta = [
        Some(game.genres.join(", ")),
        Some(platforms_line(&game)),
        year,
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let eyebrow = match game.status.as_deref() {
        Some(status) if !status.is_empty() && status != "Released" => status_name(status),
        _ => t!(
            note = "The eyebrow above a store game's title: where the page comes from",
            "IGDB catalog"
        ),
    };
    let igdb_page = game.url.clone().filter(|url| !url.is_empty());
    rsx! {
        section {
            "data-cs": "hero",
            class: "relative mt-4 overflow-hidden rounded-panel border border-border bg-surface-raised",
            GameArt { part: "art", image: art, class: "h-56 w-full" }
            div { "data-part": "content", class: "flex flex-wrap items-start gap-6 p-6",
                GameArt { image: cover, class: "aspect-(--cs-card-aspect) w-36 shrink-0 rounded-card" }
                div { class: "min-w-0 flex-[1_1_28rem]",
                    p {
                        "data-part": "eyebrow",
                        class: "font-mono text-xs tracking-widest text-accent-alt uppercase",
                        "{eyebrow}"
                    }
                    h1 { "data-part": "title", class: "mt-2 font-display text-4xl font-semibold", "{game.title}" }
                    p {
                        "data-part": "meta",
                        class: "mt-2 flex flex-wrap items-center gap-x-2 text-sm text-fg-muted",
                        "{meta}"
                        AgeRatingLabel { ratings: game.age_ratings.clone() }
                    }
                    if let Some((installed, game_id, href)) = library {
                        p { class: "mt-3 flex items-center gap-2 text-sm text-status-installed",
                            Svg { icon: LIBRARY, class: "size-4" }
                            if installed {
                                {tp!("game status", "Installed")}
                            } else {
                                {t!(note = "A store game the library has but isn't installed", "In your library")}
                            }
                            " · "
                            Link {
                                href,
                                class: "text-accent-alt underline-offset-2 hover:underline",
                                on_press: move |_| router.open_game(&game_id),
                                {t!("Open in library")}
                            }
                        }
                    }
                    p {
                        "data-part": "description",
                        class: "mt-4 max-w-3xl text-sm leading-relaxed text-fg-muted",
                        if game.description.is_empty() {
                            {t!("No description on IGDB.")}
                        } else {
                            "{game.description}"
                        }
                    }
                }
                div { "data-part": "actions", class: "flex w-full flex-col gap-3 sm:w-80",
                    h2 { class: "font-display text-lg font-semibold", {t!("Where to buy")} }
                    WhereToBuy { links: game.store_links.clone() }
                    if let Some(url) = igdb_page {
                        ExternalLink {
                            href: url,
                            class: "inline-flex items-center gap-1.5 text-sm text-fg-muted hover:text-fg",
                            {t!("IGDB page")}
                        }
                    }
                }
            }
        }
        Screenshots { title: game.title.clone(), shots: screenshots(&game) }
        Videos { title: game.title.clone(), videos: game.videos.clone() }
        Releases { game: game.clone() }
        Websites { websites: game.websites.clone() }
        SimilarGames { game: game.clone() }
    }
}

/// IGDB's screenshots for the gallery: the server's full-size copy, and
/// its smaller size for the row.
fn screenshots(game: &IgdbGame) -> Vec<Shot> {
    game.images
        .screenshots
        .iter()
        .map(|shot| Shot {
            full: shot.url.clone(),
            thumb: shot.url.replace(SHOT_FULL, SHOT_THUMB),
        })
        .collect()
}

/// The storefronts IGDB links the game to. A link says IGDB knows the
/// page, not that the game is on sale there or at what price. The first
/// takes focus when the page opens.
#[component]
pub fn WhereToBuy(links: Vec<IgdbStoreLink>) -> Element {
    if links.is_empty() {
        return rsx! {
            p { "data-part": "no-stores", class: "text-sm text-fg-muted",
                {t!("IGDB lists no stores for this game.")}
            }
        };
    }
    rsx! {
        ul { "data-cs": "store-links", class: "flex flex-col gap-2",
            for (index, link) in links.into_iter().enumerate() {
                li { key: "{link.url}",
                    Link {
                        href: link.url.clone(),
                        "data-part": "store-link",
                        "data-store": link.store.clone(),
                        "data-focus-default": (index == 0).then_some("true"),
                        class: "flex items-center gap-2.5 rounded-control border border-border-strong bg-surface-overlay/70 px-4 py-2.5 text-sm transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/50 hover:bg-surface-hover focus-visible:focus-ring",
                        on_press: {
                            let url = link.url.clone();
                            move |_| open_url(&url)
                        },
                        span { class: "font-display font-semibold tracking-wide",
                            if link.name.is_empty() { "{link.store}" } else { "{link.name}" }
                        }
                        if !link.platforms.is_empty() {
                            span { class: "truncate text-xs text-fg-subtle", {link.platforms.join(", ")} }
                        }
                        Svg { icon: EXTERNAL_LINK, class: "ml-auto size-3.5 shrink-0 text-fg-subtle" }
                    }
                }
            }
        }
    }
}

/// Each release by platform and region, as precisely as IGDB dates it.
#[component]
fn Releases(game: IgdbGame) -> Element {
    let releases = sorted_releases(&game);
    rsx! {
        section { class: "mt-8",
            h2 { class: "mb-3 font-display text-xl font-semibold", {t!("Releases")} }
            if releases.is_empty() {
                p { class: "text-sm text-fg-muted", {t!("IGDB has no release dates for this game.")} }
            } else {
                table { class: "w-full max-w-3xl text-left text-sm",
                    thead { class: "text-xs text-fg-subtle",
                        tr {
                            th { class: "py-1 font-normal", {t!(note = "A column of a store game's releases", "Platform")} }
                            th { class: "py-1 font-normal", {t!(note = "A column of a store game's releases", "Region")} }
                            th { class: "py-1 font-normal", {t!(note = "A column of a store game's releases", "Date")} }
                            th { class: "py-1 font-normal", {t!(note = "A column of a store game's releases", "Status")} }
                        }
                    }
                    tbody {
                        for release in releases {
                            {
                                let platform = release.platform.as_ref();
                                // Releases have no id; their content is
                                // their identity.
                                let key = format!(
                                    "{}-{:?}-{:?}-{:?}-{:?}",
                                    platform
                                        .and_then(|platform| platform.id.map(|id| id.to_string()).or_else(|| platform.name.clone()))
                                        .unwrap_or_default(),
                                    release.region,
                                    release.date,
                                    release.precision,
                                    release.status,
                                );
                                let name = platform
                                    .and_then(|platform| platform.name.clone())
                                    .unwrap_or_else(|| UNKNOWN.to_owned());
                                let region = region_name(release.region.as_deref())
                                    .unwrap_or_else(|| UNKNOWN.to_owned());
                                let status = release.status.as_deref().map(status_name).unwrap_or_default();
                                rsx! {
                                    tr {
                                        key: "{key}",
                                        "data-release": release_kind(&release).map(|kind| kind.name()),
                                        class: "border-t border-border",
                                        td { class: "py-1.5", "{name}" }
                                        td { class: "py-1.5 text-fg-muted", "{region}" }
                                        td { class: "py-1.5 font-mono", {release_when(&release)} }
                                        td { class: "py-1.5 text-fg-muted", "{status}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A website's link text: its kind's name, else the name IGDB gives it.
fn website_label(site: &IgdbWebsite) -> String {
    match site.kind.as_str() {
        "official" => t!(note = "A link to a game's website", "Official site"),
        "wiki" => t!(note = "A link to a game's website", "Wiki"),
        "wikipedia" => t!(note = "A link to a game's website", "Wikipedia"),
        "social" => t!(note = "A link to a game's social media page", "Social"),
        "video" => t!(note = "A link to a game's video page", "Video"),
        "community" => t!(note = "A link to a game's community page", "Community"),
        _ => site
            .name
            .clone()
            .unwrap_or_else(|| t!(note = "A link to a game's website", "Website")),
    }
}

/// The game's other websites, apart from where to buy it.
#[component]
fn Websites(websites: Vec<IgdbWebsite>) -> Element {
    if websites.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "mt-8",
            h2 { class: "mb-3 font-display text-xl font-semibold", {t!("Links")} }
            ul { class: "flex flex-wrap gap-2",
                for site in websites {
                    li { key: "{site.url}",
                        Link {
                            href: site.url.clone(),
                            class: "inline-flex items-center gap-1.5 rounded-full border border-border px-3 py-1 text-xs text-fg-muted hover:text-fg focus-visible:focus-ring",
                            on_press: {
                                let url = site.url.clone();
                                move |_| open_url(&url)
                            },
                            {website_label(&site)}
                            Svg { icon: EXTERNAL_LINK, class: "size-3" }
                        }
                    }
                }
            }
        }
    }
}

/// IGDB's similar games, without the game itself or repeats, at most
/// [`SIMILAR`].
fn similar_ids(game: &IgdbGame) -> Vec<u64> {
    let mut seen = HashSet::new();
    game.similar_games
        .iter()
        .copied()
        .filter(|&id| id != game.id && seen.insert(id))
        .take(SIMILAR)
        .collect()
}

/// What the similar games' shelf says above its cards.
enum Similar {
    None,
    Loading,
    Failed,
    Unavailable,
    Shown(Vec<IgdbGame>),
}

/// IGDB's similar games, as a shelf of catalogue cards.
#[component]
fn SimilarGames(game: IgdbGame) -> Element {
    let data = use_store_data();
    let ids: Vec<String> = similar_ids(&game).iter().map(u64::to_string).collect();
    let lookup = {
        let (data, ids) = (data.clone(), ids.clone());
        use_resource(move || {
            let (data, ids) = (data.clone(), ids.clone());
            async move {
                if ids.is_empty() {
                    return Ok(IgdbLookup::default());
                }
                (data.games)(ids).await
            }
        })
    };
    let owned = {
        let ids = ids.clone();
        use_answer(DataKey::Games, move || -> Answer<Vec<IgdbOwned>> {
            if ids.is_empty() {
                return Box::pin(async { Ok::<Vec<IgdbOwned>, ErrorReport>(Vec::new()) });
            }
            (data.owned)(ids.clone())
        })
    };
    let owned: Vec<IgdbOwned> = owned
        .read()
        .as_ref()
        .and_then(|owned| owned.as_ref().ok().cloned())
        .unwrap_or_default();
    let state = match lookup.read().as_ref() {
        _ if ids.is_empty() => Similar::None,
        None => Similar::Loading,
        Some(Err(_)) => Similar::Failed,
        Some(Ok(lookup)) if lookup.games.is_empty() => Similar::Unavailable,
        Some(Ok(lookup)) => Similar::Shown(lookup.games.clone()),
    };
    let cards: Vec<(IgdbGame, Option<IgdbOwned>)> = match &state {
        Similar::Shown(games) => games
            .iter()
            .map(|other| (other.clone(), owned_copy(&owned, other.id)))
            .collect(),
        _ => Vec::new(),
    };
    rsx! {
        crate::home::ShelfRow {
            id: "store-similar",
            title: t!("Similar games"),
            icon: SPARKLES,
            notice: rsx! {
                match state {
                    Similar::None => rsx! {
                        p { class: "text-sm text-fg-muted", {t!("IGDB lists no similar games.")} }
                    },
                    Similar::Loading => rsx! {
                        p { role: "status", class: "text-sm text-fg-muted", {t!("Loading similar games…")} }
                    },
                    Similar::Failed => rsx! {
                        p { role: "alert", class: "text-sm text-fg-muted", {t!("Could not load similar games.")} }
                    },
                    Similar::Unavailable => rsx! {
                        p { class: "text-sm text-fg-muted", {t!("The similar games aren't available yet.")} }
                    },
                    Similar::Shown(_) => rsx! {},
                }
            },
            for (index, (other, copy)) in cards.into_iter().enumerate() {
                CatalogCard {
                    key: "{other.id}",
                    owned: copy,
                    game: other,
                    shelf: true,
                    index,
                }
            }
        }
    }
}

/// [`WhereToBuy`]'s list in the anatomy catalogue.
pub const STORE_LINKS: Entry = Entry {
    parts: &[("store-link", "One store; `data-store` names it.")],
    ..Entry::new(
        "store-links",
        "WhereToBuy (apps/native/src/store/game.rs)",
        Source::App,
        "Links to the stores that sell a game.",
    )
};

#[cfg(test)]
#[path = "game_tests.rs"]
mod tests;
