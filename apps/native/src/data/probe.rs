//! A check that the data layer works in the running app (#89): with
//! `CASKADE_DATA_PROBE=1` in a debug build, a hidden view searches the
//! library for a game the probe then adds and favourites, two core writes,
//! and logs what it renders each time. Run under Xvfb with an isolated
//! profile; release builds don't have it.

use super::*;
use caskade_core::bus::Event as BusEvent;
use caskade_core::model::{Game, GameFilter, GameSort};

/// The probe's game title, which no real library has.
const TITLE: &str = "Caskade Data Probe";

/// Whether the probe runs: `CASKADE_DATA_PROBE=1`, in debug builds only.
pub fn enabled() -> bool {
    cfg!(debug_assertions) && std::env::var("CASKADE_DATA_PROBE").is_ok_and(|value| value == "1")
}

#[component]
pub fn Probe() -> Element {
    let query = use_signal(|| ListGames {
        query: Some(TITLE.into()),
        filter: GameFilter::All,
        sort: GameSort::Title,
        limit: 10,
        ..ListGames::default()
    });
    let page = use_library_page(query.into());
    let library = use_library();
    use_hook(move || {
        spawn(async move {
            // A core write from outside the view: the game is added, as a
            // scan or another device would add it.
            let added = caskade_app::runtime::spawn(async {
                let services = crate::shell::services()?;
                let game = services.store.upsert_game(&Game::new(TITLE)).await.ok()?;
                services.bus.emit(BusEvent::LibraryGameAdded {
                    game_id: game.id.clone(),
                });
                Some(game.id)
            })
            .await
            .ok()
            .flatten();
            let Some(id) = added else {
                tracing::warn!("data probe: no services to write to");
                return;
            };
            // A moment later, so the two writes show as two updates (library
            // events within 200 ms arrive as one).
            // Made on the runtime: a timer needs its reactor.
            let _ = caskade_app::runtime::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            })
            .await;
            // Then a change through the data layer, as a view's toggle.
            if let Err(error) = change(async move { library.set_favorite(&id, true).await }).await {
                tracing::warn!(%error, "data probe: the favourite didn't change");
            }
        });
    });
    let shown = page
        .read()
        .as_ref()
        .and_then(|loaded| loaded.as_ref().ok())
        .map(|page| (page.total, page.items.first().map(|game| game.favorite)));
    if let Some((games, favorite)) = shown {
        tracing::info!(games, ?favorite, "data probe: the view shows");
    }
    rsx! {}
}
