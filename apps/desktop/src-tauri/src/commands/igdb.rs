//! IGDB metadata through Caskade's metadata server. The core caches every
//! answer, so asking again for an id costs no request. Library games'
//! matches are `caskade_core::metadata`'s.
//!
//! The Store's catalog (#163) reads all of IGDB: `igdb_catalog_*`. The
//! Store links out to storefronts and installs nothing itself; installs it
//! may start later go through the install commands and their `install`
//! permission (#205). Forwarded to `caskade_app::commands::igdb`.
use caskade_app::commands::igdb as app;
use caskade_core::{
    db::{GameAgeRatings, IgdbOwned, LibraryStore},
    igdb::{
        CatalogReleases, CatalogSearch, Igdb, IgdbCatalogFilters, IgdbCatalogPage, IgdbGame,
        IgdbLookup,
    },
    metadata::{GameIgdb, Metadata},
};
use tauri::State;

/// The games with `ids`, from the cache or, for ids it has no answer for
/// yet, the server.
#[tauri::command]
#[specta::specta]
pub async fn igdb_games(igdb: State<'_, Igdb>, ids: Vec<String>) -> Result<IgdbLookup, String> {
    app::igdb_games(&igdb, &ids).await
}

/// Fetches `ids` again, replacing the cached answers older than ten
/// minutes.
#[tauri::command]
#[specta::specta]
pub async fn igdb_refresh(igdb: State<'_, Igdb>, ids: Vec<String>) -> Result<IgdbLookup, String> {
    app::igdb_refresh(&igdb, &ids).await
}

/// IGDB's best matches for the title `query`, best first, at most
/// `limit`. Always asks the server.
#[tauri::command]
#[specta::specta]
pub async fn igdb_search(
    igdb: State<'_, Igdb>,
    query: String,
    limit: u32,
) -> Result<Vec<IgdbGame>, String> {
    app::igdb_search(&igdb, &query, limit).await
}

/// A library game's IGDB match and its IGDB game; `null` before matching
/// has looked at the game.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb(
    metadata: State<'_, Metadata>,
    game_id: String,
) -> Result<Option<GameIgdb>, String> {
    app::game_igdb(&metadata, &game_id).await
}

/// Matches a library game to an IGDB game by hand, or to none with
/// `null`, and fills the game in from it.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb_set(
    metadata: State<'_, Metadata>,
    game_id: String,
    igdb_id: Option<String>,
) -> Result<GameIgdb, String> {
    app::game_igdb_set(&metadata, &game_id, igdb_id).await
}

/// Forgets a library game's match and matches it automatically again.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb_rematch(
    metadata: State<'_, Metadata>,
    game_id: String,
) -> Result<Option<GameIgdb>, String> {
    app::game_igdb_rematch(&metadata, &game_id).await
}

/// Fetches a library game's IGDB game again and fills the game in from it.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb_refresh(
    metadata: State<'_, Metadata>,
    game_id: String,
) -> Result<Option<GameIgdb>, String> {
    app::game_igdb_refresh(&metadata, &game_id).await
}

/// Moves an entry out of its game into a game of its own, which IGDB
/// matching never merges back: for releases merged as one game that aren't
/// the same game. The new game's id.
#[tauri::command]
#[specta::specta]
pub async fn game_separate_entry(
    metadata: State<'_, Metadata>,
    entry_id: String,
) -> Result<String, String> {
    app::game_separate_entry(&metadata, &entry_id).await
}

/// A page of the IGDB catalog: games matching a search, or the most-rated
/// without a query. Kept a few minutes; `stale` when it is the last copy
/// because the server couldn't be reached.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_search(
    igdb: State<'_, Igdb>,
    search: CatalogSearch,
) -> Result<IgdbCatalogPage, String> {
    app::igdb_catalog_search(&igdb, &search).await
}

/// A page of the last 90 days' releases, newest first.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_recent(
    igdb: State<'_, Igdb>,
    releases: CatalogReleases,
) -> Result<IgdbCatalogPage, String> {
    app::igdb_catalog_recent(&igdb, &releases).await
}

/// A page of releases to come, soonest first; with `tbd`, undated ones.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_upcoming(
    igdb: State<'_, Igdb>,
    releases: CatalogReleases,
) -> Result<IgdbCatalogPage, String> {
    app::igdb_catalog_upcoming(&igdb, &releases).await
}

/// The platforms and genres the catalog can be narrowed by.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_filters(igdb: State<'_, Igdb>) -> Result<IgdbCatalogFilters, String> {
    app::igdb_catalog_filters(&igdb).await
}

/// Games for the Store's pages: cached answers younger than a day, the
/// server for the rest, and the cached answer when it can't be reached.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_games(
    igdb: State<'_, Igdb>,
    ids: Vec<String>,
) -> Result<IgdbLookup, String> {
    app::igdb_catalog_games(&igdb, &ids).await
}

/// The library games matched to IGDB games `ids`: what the Store shows as
/// in the library or installed.
#[tauri::command]
#[specta::specta]
pub async fn igdb_owned(
    store: State<'_, LibraryStore>,
    ids: Vec<String>,
) -> Result<Vec<IgdbOwned>, String> {
    app::igdb_owned(&store, &ids).await
}

/// A library game's age ratings and the minimum age they come to; `null`
/// for no such game.
#[tauri::command]
#[specta::specta]
pub async fn game_age_ratings(
    store: State<'_, LibraryStore>,
    game_id: String,
) -> Result<Option<GameAgeRatings>, String> {
    app::game_age_ratings(&store, &game_id).await
}
