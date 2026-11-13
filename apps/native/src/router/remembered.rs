//! What navigation remembers across starts, as the webview keeps it in
//! local storage: the library's filter, sort and layout (a link to
//! `/library` without them comes back to these) and the page open last (for
//! "Open to → Last view", #114). Kept in `native-views.json` beside the
//! window's place; Settings' tab is remembered for the run only.

use super::href::{self, Href};
use super::route::{LibraryPrefs, Remembered, Route};
use crate::library::Layout;
use caskade_core::model::{GameFilter, GameSort};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    library: Option<SavedLibrary>,
    last_view: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedLibrary {
    filter: GameFilter,
    sort: GameSort,
    layout: String,
}

/// Where navigation's memory is kept: a file, or nowhere (fixtures mode,
/// tests).
#[derive(Debug, Clone, Default)]
pub struct Memory {
    file: Option<PathBuf>,
    remembered: Remembered,
    last_view: Option<String>,
}

impl Memory {
    /// The memory in `file`, if it can be read.
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved: Saved = file
            .as_deref()
            .and_then(crate::state_file::load)
            .unwrap_or_default();
        let library = saved
            .library
            .map(|library| LibraryPrefs {
                filter: library.filter,
                sort: library.sort,
                layout: Layout::parse(&library.layout),
            })
            .unwrap_or_default();
        Self {
            file,
            remembered: Remembered {
                library,
                settings_tab: None,
            },
            last_view: saved.last_view.as_deref().and_then(openable),
        }
    }

    /// The memory of the running app: `native-views.json` in its config
    /// folder.
    pub fn of_app() -> Self {
        Self::load(file(
            crate::shell::services().map(|services| &services.paths),
        ))
    }

    pub fn remembered(&self) -> &Remembered {
        &self.remembered
    }

    /// The page open when the app last quit, if one can be opened.
    pub fn last_view(&self) -> Option<&str> {
        self.last_view.as_deref()
    }

    /// Remembers what `route` shows: the library's view (a search or a
    /// filter nobody chose keeps the filter it had), Settings' tab, and the
    /// page itself for "Last view".
    pub fn visited(&mut self, route: &Route) {
        let mut changed = false;
        match route {
            Route::Library(library) => {
                let filter = if library.q.is_some() || library.opening {
                    self.remembered.library.filter
                } else {
                    library.filter
                };
                let prefs = LibraryPrefs {
                    filter,
                    sort: library.sort,
                    layout: library.layout,
                };
                changed |= prefs != self.remembered.library;
                self.remembered.library = prefs;
            }
            Route::Settings { tab, .. } => self.remembered.settings_tab = Some(tab),
            _ => {}
        }
        let href = route.href();
        if let Some(view) = openable(&href)
            && self.last_view.as_deref() != Some(view.as_str())
        {
            self.last_view = Some(view);
            changed = true;
        }
        if changed {
            self.save();
        }
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let library = self.remembered.library;
        let saved = Saved {
            library: Some(SavedLibrary {
                filter: library.filter,
                sort: library.sort,
                layout: library.layout.name().into(),
            }),
            last_view: self.last_view.clone(),
        };
        crate::state_file::save(file, &saved);
    }
}

/// Where the running app keeps it.
pub fn file(paths: Option<&caskade_core::AppPaths>) -> Option<PathBuf> {
    paths.map(|paths| paths.config.join("native-views.json"))
}

/// The pages "Last view" can open: those a game can go back to, and
/// Settings (`shell/startView.ts`).
pub fn openable(href: &str) -> Option<String> {
    if let Some(parsed) = Href::parse(href)
        && parsed.path == "/settings"
    {
        return Some(href.to_owned());
    }
    href::valid_return(Some(href))
}

/// Where the app opens, from `core.general.startView` (`home`, `library`,
/// `collections` or `last`): `None` for Home.
pub fn start_href(view: &str, last: Option<&str>) -> Option<String> {
    match view {
        "library" => Some("/library".into()),
        "collections" => Some("/collections".into()),
        "last" => last.filter(|last| *last != "/").map(str::to_owned),
        _ => None,
    }
}
