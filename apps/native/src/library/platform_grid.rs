//! The library's games broken down by platform (#99, the webview's
//! `views/library/platforms/PlatformGrid.tsx`), for a filter whose games span
//! several (Emulated): a card per platform, which opens the filter on it.

use super::platforms::{self, is_pc};
use super::view::library_href;
use crate::router::{LibraryRoute, RouteLink};
use crate::ui::{Svg, icons};
use caskade_core::model::PlatformSummary;
use caskade_ui_i18n::{format, t, tn};
use dioxus_native::prelude::*;

/// The platforms to break the route's games down by: Emulated's, with no
/// platform or search chosen, when there are several that aren't PC (a game
/// also out on PC is found under its console). `None` otherwise.
pub fn breakdown(
    library: &LibraryRoute,
    platforms: &[PlatformSummary],
) -> Option<Vec<PlatformSummary>> {
    if !wanted(library) {
        return None;
    }
    let consoles: Vec<PlatformSummary> = platforms
        .iter()
        .filter(|platform| !is_pc(&platform.name))
        .cloned()
        .collect();
    (consoles.len() > 1).then(|| platforms::sort(consoles, library.sort))
}

/// Whether the route breaks its games down by platform.
pub fn wanted(library: &LibraryRoute) -> bool {
    library.filter == caskade_core::model::GameFilter::Emulated
        && library.platform.is_none()
        && library.q.is_none()
}

#[component]
pub fn PlatformGrid(
    /// In the order to show them.
    platforms: Vec<PlatformSummary>,
    library: LibraryRoute,
) -> Element {
    rsx! {
        section {
            "aria-label": t!(note = "The library's games grouped by platform", "Platforms"),
            "data-focus-section": "platform-grid",
            class: "grid min-h-0 grow basis-0 auto-rows-min grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-4 overflow-auto overscroll-contain pb-4",
            for (index, platform) in platforms.into_iter().enumerate() {
                PlatformCard {
                    key: "{platform.name}",
                    to: library_href(&library, |library| library.platform = Some(platform.name.clone())),
                    platform,
                    index,
                }
            }
        }
    }
}

#[component]
fn PlatformCard(platform: PlatformSummary, index: usize, to: String) -> Element {
    let look = platforms::present(&platform.name);
    let games = tn!("# game", "# games", platform.games as usize);
    let playtime =
        (platform.playtime_ms > 0).then(|| format::duration(platform.playtime_ms / 1000));
    let last_played = platform
        .last_played_at_ms
        .map(|ms| super::relative_day(ms, true));
    let label = t!(
        note = "A platform's card: its name and how many games it has",
        "{title}, {games}",
        title = look.title.clone(),
        games = games.clone()
    );
    rsx! {
        RouteLink {
            to,
            current: false,
            "data-cs": "platform-card",
            "data-art": if look.known { "platform" } else { "generic" },
            "data-focus-key": "platform-card:{platform.name}",
            "aria-label": label,
            style: "--i: {index.min(12)}",
            class: "group relative flex animate-enter flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/60 hover:bg-surface-hover data-focus-visible:focus-ring-over",
            div {
                "data-part": "art",
                class: "relative aspect-[3/2] overflow-hidden bg-surface-base bg-[radial-gradient(ellipse_at_50%_75%,color-mix(in_oklab,var(--cs-color-accent)_35%,transparent)_0%,transparent_70%)]",
                div { "aria-hidden": "true", class: "absolute inset-x-6 bottom-3 h-6 rounded-[50%] bg-surface-base/70 blur-md" }
                // The padding is the box's, not the image's: Blitz lays out
                // a padded image that hasn't loaded with a negative size.
                div { class: "relative size-full p-4",
                    img {
                        src: "{look.art}",
                        alt: "",
                        draggable: "false",
                        class: "size-full object-contain drop-shadow-[0_10px_14px_rgb(0_0_0/0.45)] transition-transform duration-(--cs-motion-duration-base) group-hover:scale-105 group-data-focus-visible:scale-105",
                    }
                }
            }
            div { class: "flex flex-col gap-1 px-3 pt-2.5 pb-3",
                p { "data-part": "title", class: "truncate font-display text-base font-semibold", "{look.title}" }
                if !look.subtitle.is_empty() {
                    p { "data-part": "subtitle", class: "truncate text-xs text-fg-subtle", "{look.subtitle}" }
                }
                div { class: "mt-1.5 flex items-center justify-between gap-2 text-xs text-fg-muted",
                    span {
                        "data-part": "count",
                        class: "shrink-0 rounded-full border border-border bg-surface-base/60 px-2 py-0.5 font-mono whitespace-nowrap",
                        "{games}"
                    }
                    span {
                        "data-part": "meta",
                        class: "flex min-w-0 items-center gap-2 overflow-hidden font-mono whitespace-nowrap",
                        if let Some(playtime) = playtime {
                            span { "data-part": "playtime", class: "flex shrink-0 items-center gap-1",
                                Svg { icon: icons::CLOCK, class: "size-3" }
                                "{playtime}"
                            }
                        }
                        if let Some(last_played) = last_played {
                            span { "data-part": "last-played", class: "flex items-center gap-1 truncate",
                                Svg { icon: icons::HISTORY, class: "size-3" }
                                "{last_played}"
                            }
                        }
                    }
                }
            }
        }
    }
}
