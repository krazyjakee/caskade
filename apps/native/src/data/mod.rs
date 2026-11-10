//! The native UI's data hooks (#89): `caskade_ui_data` as Dioxus resources.
//! A view reads through [`use_query`] (or a hook built on it) and loads
//! again when an app event makes what it reads stale, and only then
//! (`caskade_ui_data::Changed`). No IPC, no JSON and no query cache.
//!
//! Loads run on the app's async runtime (`caskade_app::runtime`), which the
//! core's database needs, and the component awaits them from Dioxus's own
//! executor.
//!
//! [`DataSource`] is the app's services in the app, or the in-memory fixture
//! library in fixtures mode (`caskade_ui_data::fixtures`): views don't know
//! which.

use caskade_app::events::Event;
use caskade_core::model::{GameDetails, GamePage, LibraryStats, ListGames};
use caskade_ui_data::{Changed, ErrorReport, Key as DataKey, Library, fixtures};
use dioxus_native::prelude::*;
use std::future::Future;
use tokio::sync::broadcast;

/// What views read through: the library and the app's events.
#[derive(Clone)]
pub struct DataSource {
    pub library: Library,
    pub events: broadcast::Sender<Event>,
}

impl DataSource {
    /// The running app's data, once its services have started
    /// (`shell::services`); its events from the moment the core opened.
    pub fn app() -> Option<Self> {
        let services = crate::shell::services()?;
        Some(Self {
            library: Library::of(services),
            events: crate::shell::events().clone(),
        })
    }

    /// Fixtures mode's data: the in-memory library of
    /// `caskade_ui_data::fixtures`, with `count` plain games.
    pub async fn fixtures(count: usize) -> caskade_core::Result<Self> {
        let fixtures = caskade_app::runtime::spawn(fixtures::open(count))
            .await
            .map_err(|error| caskade_core::Error::Invalid(error.to_string()))??;
        Ok(Self {
            library: fixtures.library,
            events: fixtures.events,
        })
    }
}

/// Whether the UI runs on the fixture library: `CASKADE_FIXTURES=1`, in
/// debug builds (and the `bench` feature's) only (the webview's `VITE_CASKADE_FIXTURES`).
pub fn fixtures_mode() -> bool {
    cfg!(any(debug_assertions, feature = "bench"))
        && std::env::var("CASKADE_FIXTURES").is_ok_and(|value| value == "1")
}

/// How many plain games fixtures mode makes: `CASKADE_FIXTURE_COUNT`, or the
/// webview's 24.
pub fn fixture_count() -> usize {
    std::env::var("CASKADE_FIXTURE_COUNT")
        .ok()
        .and_then(|count| count.parse().ok())
        .unwrap_or(fixtures::DEFAULT_COUNT)
}

/// Gives the components below `source`.
pub fn use_data_source(source: impl FnOnce() -> DataSource) -> DataSource {
    use_context_provider(source)
}

impl PartialEq for DataSource {
    /// One source per app: the same events mean the same data.
    fn eq(&self, other: &Self) -> bool {
        self.events.same_channel(&other.events)
    }
}

/// The source this run reads: the fixture library in fixtures mode, else
/// the app's services, once they've started.
async fn resolve() -> Option<DataSource> {
    if fixtures_mode() {
        return DataSource::fixtures(fixture_count())
            .await
            .inspect_err(|error| tracing::error!(%error, "the fixture library didn't open"))
            .ok();
    }
    // The services start beside the window, about a second in.
    caskade_app::runtime::spawn(async {
        loop {
            if let Some(source) = DataSource::app() {
                return source;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .ok()
}

/// Gives `children` the data source once it's ready. Until then they render
/// without it; once it is, they render again inside it.
#[component]
pub fn DataRoot(children: Element) -> Element {
    let source = use_resource(resolve);
    let ready = source.read().clone().flatten();
    match ready {
        Some(source) => rsx! {
            Provided { source, {children} }
        },
        None => children,
    }
}

#[component]
fn Provided(source: DataSource, children: Element) -> Element {
    use_data_source(move || source);
    rsx! {
        {children}
        if probe::enabled() {
            probe::Probe {}
        }
    }
}

mod probe;

/// The library, for a view's changes: run them through [`change`].
pub fn use_library() -> Library {
    use_context::<DataSource>().library
}

/// Runs a change on the app's runtime, which the core needs, from a view;
/// the event it causes refreshes what it changed.
pub async fn change<T, Fut>(work: Fut) -> Result<T, ErrorReport>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T, ErrorReport>> + Send + 'static,
{
    caskade_app::runtime::spawn(work)
        .await
        .unwrap_or_else(|error| Err(ErrorReport::from_error(&error)))
}

/// What a query gives a view: its data, or the core's report of what went
/// wrong; `None` while the first load runs.
pub type Query<T> = Resource<Result<T, ErrorReport>>;

/// The data `load` reads, loaded again whenever an app event makes `key`
/// stale, or when events were missed. Signals `load` reads before its
/// future starts (a page's query, say) load it again too.
pub fn use_query<T, F, Fut>(key: DataKey, mut load: F) -> Query<T>
where
    T: Send + 'static,
    F: FnMut(Library) -> Fut + 'static,
    Fut: Future<Output = Result<T, ErrorReport>> + Send + 'static,
{
    let source = use_context::<DataSource>();
    let library = source.library.clone();
    let mut resource = use_resource(move || change(load(library.clone())));
    use_hook(move || {
        let mut events = source.events.subscribe();
        spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) if Changed::from_event(&event).affects(&key) => resource.restart(),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => resource.restart(),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
    resource
}

/// A page of the library for `query`; at most 100 summaries.
pub fn use_library_page(query: ReadSignal<ListGames>) -> Query<GamePage> {
    use_query(DataKey::Games, move |library| {
        let query = query();
        async move { library.page(&query).await }
    })
}

/// The top bar's search: the best few matches for `text`.
// The views (#96 onwards) use it.
#[allow(dead_code)]
pub fn use_library_search(text: ReadSignal<String>) -> Query<GamePage> {
    use_query(DataKey::Games, move |library| {
        let text = text();
        async move { library.search(&text).await }
    })
}

/// A game with its releases; `None` when it's gone.
// The views (#96 onwards) use it.
#[allow(dead_code)]
pub fn use_game(id: String) -> Query<Option<GameDetails>> {
    use_query(DataKey::Game(id.clone()), move |library| {
        let id = id.clone();
        async move { library.game(&id).await }
    })
}

/// The library's totals.
// The views (#96 onwards) use it.
#[allow(dead_code)]
pub fn use_library_stats() -> Query<LibraryStats> {
    use_query(
        DataKey::Summary,
        |library| async move { library.stats().await },
    )
}

#[cfg(test)]
mod tests;
