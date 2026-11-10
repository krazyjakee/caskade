//! The app's pages, as the webview's router has them (`router.tsx`): each
//! parses from its href, checking its search as the webview's
//! `validateSearch` does, and writes the same href back.

use super::href::{self, Href, Value};
use crate::library::Layout;
use caskade_core::model::{GameFilter, GameSort};

/// The library's filters, in the sidebar's order.
pub const LIBRARY_FILTERS: [GameFilter; 7] = [
    GameFilter::Recent,
    GameFilter::Installed,
    GameFilter::Owned,
    GameFilter::Emulated,
    GameFilter::Web,
    GameFilter::Favorites,
    GameFilter::All,
];

const SORTS: [GameSort; 6] = [
    GameSort::Recent,
    GameSort::Title,
    GameSort::Playtime,
    GameSort::Added,
    GameSort::Released,
    GameSort::Popular,
];

/// Settings' tabs (`views/SettingsView.tsx`).
pub const SETTINGS_TABS: [&str; 17] = [
    "general",
    "appearance",
    "plugins",
    "library-discovery",
    "launchers",
    "stores",
    "storage",
    "installer",
    "emulators",
    "scripts",
    "devices",
    "streaming",
    "profile",
    "accounts",
    "backup",
    "statistics",
    "about",
];

const STORE_TABS: [&str; 3] = ["browse", "recent", "upcoming"];
const STORE_SORTS: [&str; 4] = ["relevance", "mostRated", "popular", "newest"];

pub fn filter_name(filter: GameFilter) -> &'static str {
    match filter {
        GameFilter::All => "all",
        GameFilter::Installed => "installed",
        GameFilter::Favorites => "favorites",
        GameFilter::Recent => "recent",
        GameFilter::Emulated => "emulated",
        GameFilter::Web => "web",
        GameFilter::Owned => "owned",
    }
}

fn sort_name(sort: GameSort) -> &'static str {
    match sort {
        GameSort::Title => "title",
        GameSort::Recent => "recent",
        GameSort::Playtime => "playtime",
        GameSort::Added => "added",
        GameSort::Released => "released",
        GameSort::Popular => "popular",
    }
}

fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Grid => "grid",
        Layout::List => "list",
    }
}

fn pick<T: Copy>(values: &[T], name: fn(T) -> &'static str, value: Option<&str>) -> Option<T> {
    let value = value?;
    values
        .iter()
        .copied()
        .find(|&candidate| name(candidate) == value)
}

/// The library's filter, sort and layout as the user last left them: a link
/// to `/library` without them comes back to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryPrefs {
    pub filter: GameFilter,
    pub sort: GameSort,
    pub layout: Layout,
}

impl Default for LibraryPrefs {
    fn default() -> Self {
        Self {
            filter: GameFilter::Recent,
            sort: GameSort::Recent,
            layout: Layout::Grid,
        }
    }
}

/// What a link may leave out comes from here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Remembered {
    pub library: LibraryPrefs,
    /// Settings' tab this run, if one was opened.
    pub settings_tab: Option<&'static str>,
}

/// The library's view, as its href holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryRoute {
    pub filter: GameFilter,
    pub sort: GameSort,
    pub layout: Layout,
    pub q: Option<String>,
    pub platform: Option<String>,
    /// No link named the filter: the user didn't choose it here.
    pub opening: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTab {
    Jobs,
    Notifications,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionFilter {
    All,
    Smart,
    Manual,
}

/// The Store's view, as its href holds it (`validateStoreSearch`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoreRoute {
    pub tab: &'static str,
    pub sort: Option<&'static str>,
    pub q: Option<String>,
    pub platform: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub tbd: bool,
}

/// A page of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Home,
    Library(LibraryRoute),
    Game {
        id: String,
        /// The page it was opened from, for its Back link.
        from: Option<String>,
    },
    Activity {
        tab: ActivityTab,
        unread: bool,
        open: Option<String>,
    },
    Store(StoreRoute),
    StoreGame {
        igdb_id: String,
        from: Option<String>,
    },
    Collections {
        filter: CollectionFilter,
    },
    Collection {
        id: String,
    },
    Settings {
        tab: &'static str,
        section: Option<String>,
    },
    Plugin {
        plugin: String,
        view: String,
    },
    NotFound {
        href: String,
    },
}

/// Trimmed text, if any.
fn text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The library route of `href`'s search, with what it leaves out from
/// `remembered`.
fn library(href: &Href, remembered: &Remembered) -> LibraryRoute {
    let saved = remembered.library;
    let filter_given = href.get("filter").is_some();
    LibraryRoute {
        filter: pick(&LIBRARY_FILTERS, filter_name, href.get_text("filter"))
            .unwrap_or(saved.filter),
        sort: pick(&SORTS, sort_name, href.get_text("sort")).unwrap_or(saved.sort),
        layout: pick(
            &[Layout::Grid, Layout::List],
            layout_name,
            href.get_text("layout"),
        )
        .unwrap_or(saved.layout),
        q: text(href.get_text("q")),
        platform: text(href.get_text("platform")),
        opening: !filter_given || href.get("opening") == Some(&Value::Bool(true)),
    }
}

fn store(href: &Href) -> StoreRoute {
    let id = |key: &str| {
        let raw = match href.get(key) {
            Some(Value::Number(number)) if number.fract() == 0.0 && *number >= 0.0 => {
                Some(format!("{number:.0}"))
            }
            Some(Value::Text(value)) => text(Some(value)),
            _ => None,
        };
        raw.filter(|raw| {
            (1..=18).contains(&raw.len()) && raw.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    let year = match href.get("year") {
        Some(Value::Number(year)) if year.fract() == 0.0 && (1950.0..=2100.0).contains(year) => {
            Some(*year as u16)
        }
        Some(Value::Text(year)) => year
            .parse()
            .ok()
            .filter(|year| (1950..=2100).contains(year)),
        _ => None,
    };
    StoreRoute {
        tab: STORE_TABS
            .iter()
            .copied()
            .find(|&tab| Some(tab) == href.get_text("tab"))
            .unwrap_or("browse"),
        sort: STORE_SORTS
            .iter()
            .copied()
            .find(|&sort| Some(sort) == href.get_text("sort")),
        q: text(href.get_text("q")).map(|q| q.chars().take(100).collect()),
        platform: id("platform"),
        genre: id("genre"),
        year,
        tbd: matches!(href.get("tbd"), Some(Value::Bool(true)))
            || href.get_text("tbd") == Some("true"),
    }
}

impl Route {
    /// The page `href` names; what its search leaves out comes from
    /// `remembered`.
    pub fn parse(href: &str, remembered: &Remembered) -> Self {
        let Some(parsed) = Href::parse(href) else {
            return Self::NotFound {
                href: href.to_owned(),
            };
        };
        let segments = parsed.segments();
        let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
        let from = || href::valid_return(parsed.get_text("from"));
        match segments.as_slice() {
            [] => Self::Home,
            ["library"] => Self::Library(library(&parsed, remembered)),
            ["games", id] => Self::Game {
                id: (*id).to_owned(),
                from: from(),
            },
            ["activity"] => Self::Activity {
                tab: match parsed.get_text("tab") {
                    Some("notifications") => ActivityTab::Notifications,
                    _ => ActivityTab::Jobs,
                },
                unread: parsed.get("unread") == Some(&Value::Bool(true)),
                open: text(parsed.get_text("open")),
            },
            ["store"] => Self::Store(store(&parsed)),
            ["store", "games", id] => Self::StoreGame {
                igdb_id: (*id).to_owned(),
                from: from(),
            },
            ["collections"] => Self::Collections {
                filter: match parsed.get_text("filter") {
                    Some("smart") => CollectionFilter::Smart,
                    Some("manual") => CollectionFilter::Manual,
                    _ => CollectionFilter::All,
                },
            },
            ["collections", id] => Self::Collection {
                id: (*id).to_owned(),
            },
            ["settings"] => Self::Settings {
                tab: SETTINGS_TABS
                    .iter()
                    .copied()
                    .find(|&tab| Some(tab) == parsed.get_text("tab"))
                    .or(remembered.settings_tab)
                    .unwrap_or("general"),
                section: parsed.get_text("section").map(str::to_owned),
            },
            ["plugins", plugin, view] => Self::Plugin {
                plugin: (*plugin).to_owned(),
                view: (*view).to_owned(),
            },
            _ => Self::NotFound {
                href: href.to_owned(),
            },
        }
    }

    /// The href that names this page.
    pub fn href(&self) -> String {
        let href = match self {
            Self::Home => Href::new("/"),
            Self::Library(library) => Href::new("/library")
                .text("filter", Some(filter_name(library.filter)))
                .text("sort", Some(sort_name(library.sort)))
                .text("layout", Some(layout_name(library.layout)))
                .text("q", library.q.as_deref())
                .text("platform", library.platform.as_deref())
                .with("opening", library.opening.then_some(Value::Bool(true))),
            Self::Game { id, from } => {
                Href::new(format!("/games/{}", href::segment(id))).text("from", from.as_deref())
            }
            Self::Activity { tab, unread, open } => Href::new("/activity")
                .text(
                    "tab",
                    Some(match tab {
                        ActivityTab::Jobs => "jobs",
                        ActivityTab::Notifications => "notifications",
                    }),
                )
                .with("unread", unread.then_some(Value::Bool(true)))
                .text("open", open.as_deref()),
            Self::Store(store) => Href::new("/store")
                .text("tab", Some(store.tab))
                .text("sort", store.sort)
                .text("q", store.q.as_deref())
                .text("platform", store.platform.as_deref())
                .text("genre", store.genre.as_deref())
                .with(
                    "year",
                    store.year.map(|year| Value::Number(f64::from(year))),
                )
                .with("tbd", store.tbd.then_some(Value::Bool(true))),
            Self::StoreGame { igdb_id, from } => {
                Href::new(format!("/store/games/{}", href::segment(igdb_id)))
                    .text("from", from.as_deref())
            }
            Self::Collections { filter } => Href::new("/collections").text(
                "filter",
                Some(match filter {
                    CollectionFilter::All => "all",
                    CollectionFilter::Smart => "smart",
                    CollectionFilter::Manual => "manual",
                }),
            ),
            Self::Collection { id } => Href::new(format!("/collections/{}", href::segment(id))),
            Self::Settings { tab, section } => Href::new("/settings")
                .text("tab", Some(tab))
                .text("section", section.as_deref()),
            Self::Plugin { plugin, view } => Href::new(format!(
                "/plugins/{}/{}",
                href::segment(plugin),
                href::segment(view)
            )),
            Self::NotFound { href } => return href.clone(),
        };
        href.to_string()
    }

    /// The page's path: what the sidebar's links match and a view is keyed
    /// by.
    pub fn path(&self) -> String {
        let href = self.href();
        href.split('?').next().unwrap_or_default().to_owned()
    }
}
