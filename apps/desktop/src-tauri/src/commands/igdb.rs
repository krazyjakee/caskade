//! IGDB metadata through Caskade's metadata server. The core caches every
//! answer, so asking again for an id costs no request. Library games'
//! matches are `caskade_core::metadata`'s.
//!
//! The Store's catalog (#163) reads all of IGDB: `igdb_catalog_*`. The
//! Store links out to storefronts and installs nothing itself; installs it
//! may start later go through the install commands and their `install`
//! permission (#205).
use caskade_core::{
    db::{GameAgeRatings, IgdbOwned, LibraryStore},
    igdb::{
        self, CatalogReleases, CatalogSearch, Igdb, IgdbCatalogFilters, IgdbCatalogPage, IgdbGame,
        IgdbLookup,
    },
    metadata::{GameIgdb, Metadata},
};
use tauri::State;

/// Ids cross as strings; anything that is not one is refused, and so is
/// more than one request's worth.
fn parse(ids: &[String]) -> Result<Vec<u64>, String> {
    if ids.len() > igdb::BATCH {
        return Err(caskade_core::tr!(
            "at most {count} IGDB ids at a time",
            count = igdb::BATCH
        ));
    }
    ids.iter()
        .map(|id| {
            id.trim()
                .parse()
                .map_err(|_| caskade_core::tr!("{id} is not an IGDB id", id = format!("{id:?}")))
        })
        .collect()
}

/// The games with `ids`, from the cache or, for ids it has no answer for
/// yet, the server.
#[tauri::command]
#[specta::specta]
pub async fn igdb_games(igdb: State<'_, Igdb>, ids: Vec<String>) -> Result<IgdbLookup, String> {
    igdb.games(&parse(&ids)?)
        .await
        .map_err(|error| error.to_string())
}

/// Fetches `ids` again, replacing the cached answers older than ten
/// minutes.
#[tauri::command]
#[specta::specta]
pub async fn igdb_refresh(igdb: State<'_, Igdb>, ids: Vec<String>) -> Result<IgdbLookup, String> {
    igdb.refresh(&parse(&ids)?)
        .await
        .map_err(|error| error.to_string())
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
    igdb.search(&query, limit as usize)
        .await
        .map_err(|error| error.to_string())
}

/// A library game's IGDB match and its IGDB game; `null` before matching
/// has looked at the game.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb(
    metadata: State<'_, Metadata>,
    game_id: String,
) -> Result<Option<GameIgdb>, String> {
    metadata
        .game(&game_id)
        .await
        .map_err(|error| error.to_string())
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
    let igdb_id = match igdb_id {
        Some(id) => Some(parse(std::slice::from_ref(&id))?[0]),
        None => None,
    };
    metadata
        .set(&game_id, igdb_id)
        .await
        .map_err(|error| error.to_string())
}

/// Forgets a library game's match and matches it automatically again.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb_rematch(
    metadata: State<'_, Metadata>,
    game_id: String,
) -> Result<Option<GameIgdb>, String> {
    metadata
        .rematch(&game_id)
        .await
        .map_err(|error| error.to_string())
}

/// Fetches a library game's IGDB game again and fills the game in from it.
#[tauri::command]
#[specta::specta]
pub async fn game_igdb_refresh(
    metadata: State<'_, Metadata>,
    game_id: String,
) -> Result<Option<GameIgdb>, String> {
    metadata
        .refresh(&game_id)
        .await
        .map_err(|error| error.to_string())
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
    metadata
        .separate(&entry_id)
        .await
        .map_err(|error| error.to_string())
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
    igdb.catalog_search(&search)
        .await
        .map_err(|error| error.to_string())
}

/// A page of the last 90 days' releases, newest first.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_recent(
    igdb: State<'_, Igdb>,
    releases: CatalogReleases,
) -> Result<IgdbCatalogPage, String> {
    igdb.catalog_recent(&releases)
        .await
        .map_err(|error| error.to_string())
}

/// A page of releases to come, soonest first; with `tbd`, undated ones.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_upcoming(
    igdb: State<'_, Igdb>,
    releases: CatalogReleases,
) -> Result<IgdbCatalogPage, String> {
    igdb.catalog_upcoming(&releases)
        .await
        .map_err(|error| error.to_string())
}

/// The platforms and genres the catalog can be narrowed by.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_filters(igdb: State<'_, Igdb>) -> Result<IgdbCatalogFilters, String> {
    igdb.catalog_filters()
        .await
        .map_err(|error| error.to_string())
}

/// Games for the Store's pages: cached answers younger than a day, the
/// server for the rest, and the cached answer when it can't be reached.
#[tauri::command]
#[specta::specta]
pub async fn igdb_catalog_games(
    igdb: State<'_, Igdb>,
    ids: Vec<String>,
) -> Result<IgdbLookup, String> {
    igdb.catalog_games(&parse(&ids)?)
        .await
        .map_err(|error| error.to_string())
}

/// The library games matched to IGDB games `ids`: what the Store shows as
/// in the library or installed.
#[tauri::command]
#[specta::specta]
pub async fn igdb_owned(
    store: State<'_, LibraryStore>,
    ids: Vec<String>,
) -> Result<Vec<IgdbOwned>, String> {
    store
        .igdb_owned(&parse(&ids)?)
        .await
        .map_err(|error| error.to_string())
}

/// A library game's age ratings and the minimum age they come to; `null`
/// for no such game.
#[tauri::command]
#[specta::specta]
pub async fn game_age_ratings(
    store: State<'_, LibraryStore>,
    game_id: String,
) -> Result<Option<GameAgeRatings>, String> {
    store
        .game_age_ratings(&game_id)
        .await
        .map_err(|error| error.to_string())
}
