//! The sidebar (`shell/Sidebar.tsx`, `LibraryNav.tsx`, `nav.ts`): the brand,
//! the pages, the library's filters with their counts while the library is
//! open, and Settings below the divider.

use crate::router::{LIBRARY_FILTERS, Route, RouteLink, filter_name, href::Href, use_router};
use crate::ui::{Icon, Svg, icons};
use caskade_core::model::{GameFilter, LibraryStats};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

const ITEM: &str = "relative flex items-center gap-3 rounded-control px-3 py-2.5 text-sm text-fg-muted transition-[color,background-color,translate] duration-(--cs-motion-duration-fast) hover:translate-x-0.5 hover:bg-surface-hover hover:text-fg data-[status=active]:text-fg data-focus-visible:focus-ring";

/// A link of the sidebar's: where it goes, and whether only the whole href
/// makes it current (Home's `/`).
struct NavItem {
    label: fn() -> String,
    icon: Icon,
    to: &'static str,
    exact: bool,
}

/// Above the divider (`primaryNav`).
const PRIMARY: [NavItem; 4] = [
    NavItem {
        label: || {
            tp!(
                note = "The sidebar link to the home view",
                "navigation",
                "Home"
            )
        },
        icon: icons::HOUSE,
        to: "/",
        exact: true,
    },
    // The library as it was last left: its remembered filter, sort and
    // layout.
    NavItem {
        label: || t!("Library"),
        icon: icons::LAYOUT_GRID,
        to: "/library",
        exact: false,
    },
    NavItem {
        label: || t!("Store"),
        icon: icons::STORE,
        to: "/store",
        exact: false,
    },
    NavItem {
        label: || t!("Collections"),
        icon: icons::SQUARE_LIBRARY,
        to: "/collections",
        exact: false,
    },
];

/// Below the divider (`secondaryNav`).
const SECONDARY: [NavItem; 1] = [NavItem {
    label: || t!("Settings"),
    icon: icons::SETTINGS,
    to: "/settings",
    exact: false,
}];

/// A filter's name, as the library's tabs say it.
pub fn filter_label(filter: GameFilter) -> String {
    match filter {
        GameFilter::All => tp!("library filter", "All Games"),
        GameFilter::Installed => tp!("library filter", "Installed"),
        GameFilter::Owned => tp!(
            note = "Games a connected store account owns that aren't installed",
            "library filter",
            "Ready to Install"
        ),
        GameFilter::Recent => tp!("library filter", "Recently Played"),
        GameFilter::Emulated => tp!("library filter", "Emulated"),
        GameFilter::Web => tp!(
            note = "The library's tab of games played in the browser",
            "library filter",
            "Web"
        ),
        GameFilter::Favorites => tp!("library filter", "Favorites"),
    }
}

fn count(stats: &LibraryStats, filter: GameFilter) -> u64 {
    match filter {
        GameFilter::All => stats.total,
        GameFilter::Installed => stats.installed,
        GameFilter::Owned => stats.owned,
        GameFilter::Recent => stats.recent,
        GameFilter::Emulated => stats.emulated,
        GameFilter::Web => stats.web,
        GameFilter::Favorites => stats.favorites,
    }
}

#[component]
fn NavLink(index: usize, primary: bool) -> Element {
    let item = if primary {
        &PRIMARY[index]
    } else {
        &SECONDARY[index]
    };
    rsx! {
        RouteLink {
            to: item.to,
            exact: item.exact,
            class: ITEM,
            "data-part": "nav-item",
            Svg { icon: item.icon, part: "icon", class: "size-5 shrink-0" }
            span { "data-part": "label", class: "flex-1", {(item.label)()} }
        }
    }
}

#[component]
pub fn Sidebar() -> Element {
    rsx! {
        aside {
            "data-cs": "sidebar",
            "data-focus-section": "sidebar",
            class: "relative z-10 flex w-sidebar shrink-0 flex-col border-r border-border bg-surface-raised/80 px-4 pb-6",
            // The brand shares the title bar's row and moves the window
            // like it; on macOS the traffic lights take that row and the
            // brand sits below.
            div {
                "data-part": "brand",
                "data-window-drag": "true",
                class: if cfg!(target_os = "macos") { "-mx-4 mb-4 flex shrink-0 items-center gap-3 px-6 pt-topbar" } else { "-mx-4 mb-4 flex h-topbar shrink-0 items-center gap-3 px-6" },
                Brand {}
                span { class: "font-display text-xl font-semibold tracking-[0.3em]", "CASKADE" }
            }
            nav { "data-part": "nav", class: "flex flex-col gap-1",
                for index in 0..PRIMARY.len() {
                    NavLink { key: "{index}", index, primary: true }
                    if PRIMARY[index].to == "/library" {
                        LibraryNav {}
                    }
                }
            }
            hr { "data-part": "divider", class: "my-4 border-border" }
            nav { "data-part": "nav", class: "flex flex-col gap-1",
                for index in 0..SECONDARY.len() {
                    NavLink { key: "{index}", index, primary: false }
                }
            }
        }
    }
}

/// Caskade's mark (`public/caskade.svg`), at `class`'s size (Settings →
/// About shows it too).
#[component]
pub(crate) fn Brand(#[props(default = "size-9".to_owned())] class: String) -> Element {
    rsx! {
        svg {
            class: "{class}",
            "aria-hidden": "true",
            view_box: "112 112 800 800",
            circle {
                cx: "512",
                cy: "512",
                r: "340",
                fill: "none",
                stroke: "#38bdf8",
                stroke_width: "80",
            }
            polygon {
                points: "625.1,314 710,398.9 398.9,710 314,625.1",
                fill: "#a78bfa",
            }
        }
    }
}

/// The library's filters under its link, while the library is open (a
/// closed list is out of the tab order and the pad's reach). A filter keeps
/// the library's search; its sort and layout are the remembered ones.
#[component]
fn LibraryNav() -> Element {
    let router = use_router();
    let route = router.route();
    let Route::Library(library) = &route else {
        return rsx! {};
    };
    let stats = crate::data::use_library_stats();
    let stats = stats
        .read()
        .as_ref()
        .and_then(|stats| stats.as_ref().ok())
        .cloned();
    rsx! {
        div { "data-part": "sub-nav", "data-open": "true", class: "mb-0",
            nav {
                "aria-label": t!("Library filters"),
                class: "ml-5 flex min-h-0 flex-col gap-0.5 overflow-hidden border-l border-border pl-2",
                for filter in LIBRARY_FILTERS {
                    {
                        let to = Href::new("/library")
                            .text("filter", Some(filter_name(filter)))
                            .text("q", library.q.as_deref())
                            .to_string();
                        let shown = stats.as_ref().map(|stats| count(stats, filter).to_string());
                        rsx! {
                            RouteLink {
                                key: "{filter_name(filter)}",
                                to,
                                class: "flex items-center gap-2 rounded-control px-3 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover hover:text-fg data-focus-visible:bg-surface-hover data-focus-visible:text-fg data-[status=active]:bg-accent/15 data-[status=active]:text-fg",
                                current: library.filter == filter,
                                "data-part": "sub-item",
                                span { "data-part": "label", class: "flex-1 truncate", {filter_label(filter)} }
                                span { "data-part": "count", class: "font-mono text-xs text-fg-subtle",
                                    {shown.unwrap_or_default()}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
