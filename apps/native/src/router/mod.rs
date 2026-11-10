//! Navigation between the app's pages (#96), in place of the webview's
//! TanStack Router: a page is an href as the webview writes it
//! (`/library?filter=installed`), parsed into a [`Route`] with what the
//! link left out filled in from what navigation remembers. History goes
//! back and forward, focus comes back to where each page was left (the
//! input layer, #92), and Back on a pad or Escape goes back a page.
//!
//! [`RouterRoot`] holds it; views read the page through [`use_router`] and
//! link with [`RouteLink`].

pub mod href;
mod remembered;
pub mod route;

#[cfg(test)]
mod tests;

pub use remembered::{Memory, start_href};
#[allow(unused_imports)]
pub use route::{
    ActivityTab, CollectionFilter, LIBRARY_FILTERS, LibraryRoute, Route, SETTINGS_TABS, StoreRoute,
    filter_name,
};

use crate::input::{use_focus_location, use_input};
use crate::ui::Link;
use dioxus_native::prelude::*;

/// The pages visited, and which is shown.
#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<Entry>,
    at: usize,
    next_key: u64,
    memory: Memory,
}

/// A visit to a page: its key tells two visits of the same page apart, so
/// focus comes back to where each was left.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub route: Route,
    pub key: String,
}

/// How far back history goes.
const MAX_ENTRIES: usize = 100;

impl History {
    /// History on Home, with `memory`.
    pub fn new(memory: Memory) -> Self {
        let mut history = Self {
            entries: Vec::new(),
            at: 0,
            next_key: 0,
            memory,
        };
        let entry = history.entry("/");
        history.entries.push(entry);
        history
    }

    fn entry(&mut self, href: &str) -> Entry {
        let route = Route::parse(href, self.memory.remembered());
        self.next_key += 1;
        Entry {
            route,
            key: format!("{}", self.next_key),
        }
    }

    pub fn current(&self) -> &Entry {
        &self.entries[self.at]
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Opens `href` after the current page; the pages ahead are dropped.
    /// The same page again changes nothing.
    pub fn push(&mut self, href: &str) {
        let entry = self.entry(href);
        if entry.route == self.current().route {
            return;
        }
        self.entries.truncate(self.at + 1);
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.at = self.entries.len() - 1;
        self.visited();
    }

    /// Shows `href` in place of the current page (a search typed in
    /// place, a filter corrected): Back skips what it replaced.
    pub fn replace(&mut self, href: &str) {
        let mut entry = self.entry(href);
        entry.key = self.current().key.clone();
        self.entries[self.at] = entry;
        self.visited();
    }

    #[allow(dead_code, reason = "the game page's Back (#100) asks")]
    pub fn can_go_back(&self) -> bool {
        self.at > 0
    }

    pub fn back(&mut self) -> bool {
        if self.at == 0 {
            return false;
        }
        self.at -= 1;
        self.visited();
        true
    }

    pub fn forward(&mut self) -> bool {
        if self.at + 1 >= self.entries.len() {
            return false;
        }
        self.at += 1;
        self.visited();
        true
    }

    /// Whether only the page the app opened on has been shown.
    pub fn untouched(&self) -> bool {
        self.entries.len() == 1 && self.current().key == "1"
    }

    fn visited(&mut self) {
        let route = self.current().route.clone();
        self.memory.visited(&route);
    }
}

/// The app's navigation, from any component under [`RouterRoot`].
#[derive(Clone, Copy, PartialEq)]
pub struct Router {
    history: Signal<History>,
}

impl Router {
    /// The page shown; a component that reads it renders again when it
    /// changes.
    pub fn route(&self) -> Route {
        self.history.read().current().route.clone()
    }

    /// The visit shown, for keying views and focus.
    pub fn key(&self) -> String {
        self.history.read().current().key.clone()
    }

    /// The page shown, without subscribing to it.
    pub fn peek(&self) -> Route {
        self.history.peek().current().route.clone()
    }

    pub fn navigate(&self, href: &str) {
        let mut history = self.history;
        history.write().push(href);
    }

    pub fn replace(&self, href: &str) {
        let mut history = self.history;
        history.write().replace(href);
    }

    pub fn back(&self) -> bool {
        let mut history = self.history;
        history.write().back()
    }

    pub fn forward(&self) -> bool {
        let mut history = self.history;
        history.write().forward()
    }

    #[allow(dead_code, reason = "the game page's Back (#100) asks")]
    pub fn can_go_back(&self) -> bool {
        self.history.read().can_go_back()
    }

    /// Game `id`'s page, going back to the page shown now.
    pub fn open_game(&self, id: &str) {
        let from = href::valid_return(Some(&self.peek().href()));
        let route = Route::Game {
            id: id.to_owned(),
            from,
        };
        self.navigate(&route.href());
    }

    /// What navigation remembers, for links that leave things out.
    pub fn memory(&self) -> Memory {
        self.history.peek().memory().clone()
    }
}

/// The app's navigation.
pub fn use_router() -> Router {
    use_context::<Router>()
}

/// Holds navigation for `children`, starting on Home (or `start`, else the
/// page "Open to" names, #114). Back from the input layer goes back a page,
/// a game's details open its page, and focus follows each visit.
#[component]
pub fn RouterRoot(
    /// The page to open on, in place of the "Open to" setting.
    #[props(default)]
    start: Option<String>,
    children: Element,
) -> Element {
    let router = use_context_provider(|| Router {
        history: Signal::new(History::new(Memory::of_app())),
    });
    let input = use_input();
    use_hook(move || {
        input.on_back(move || {
            router.back();
        });
        input.on_details(move |id| router.open_game(&id));
        let start = start.clone();
        spawn(async move {
            let href = match start {
                Some(href) => Some(href),
                None => open_to(router.memory().last_view().map(str::to_owned)).await,
            };
            // Only if nothing was opened meanwhile.
            if let Some(href) = href
                && router.history.peek().untouched()
            {
                router.replace(&href);
            }
        });
    });
    use_focus_location(router.key());
    children
}

/// Where "Open to" opens, if not Home.
async fn open_to(last: Option<String>) -> Option<String> {
    let services = crate::shell::services()?;
    let settings = services.settings.clone();
    let general = caskade_app::runtime::spawn(async move {
        settings
            .read_or_default::<caskade_core::settings::GeneralSettings>("core.general")
            .await
    })
    .await
    .ok()?;
    start_href(&general.start_view, last.as_deref())
}

/// A link to an app page: `to` is its href. It shows as current while its
/// page is (by path, or the whole page with `exact`, or as `current` says),
/// with `aria-current` and TanStack's `data-status="active"`.
#[component]
pub fn RouteLink(
    to: String,
    /// Current only when the whole page matches, not just the path.
    #[props(default)]
    exact: bool,
    /// Whether it's current, in place of matching the page.
    #[props(default)]
    current: Option<bool>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let router = use_router();
    let route = router.route();
    let current = current.unwrap_or_else(|| {
        let target = Route::parse(&to, router.memory().remembered());
        if exact {
            target == route
        } else {
            target.path() == route.path()
        }
    });
    let href = to.clone();
    let mut attributes = attributes;
    if current {
        attributes.push(Attribute::new("data-status", "active", None, false));
    }
    rsx! {
        Link {
            href: "{to}",
            current,
            class,
            on_press: move |_| router.navigate(&href),
            attributes,
            {children}
        }
    }
}
