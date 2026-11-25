//! IGDB metadata through Caskade's metadata server. The core caches every
//! answer, so asking again for an id costs no request. Library games'
//! matches are `caskade_core::metadata`'s.
//!
//! The Store's catalog (#163) reads all of IGDB: `igdb_catalog_*`. The
//! Store links out to storefronts and installs nothing itself; installs it
//! may start later go through the install commands and their `install`
//! permission (#205).
use crate::error::{CommandError, refused, report};
use caskade_core::{
    db::{GameAgeRatings, IgdbOwned, LibraryStore},
    igdb::{
        self, CatalogReleases, CatalogSearch, Igdb, IgdbCatalogFilters, IgdbCatalogPage, IgdbGame,
        IgdbLookup,
    },
    metadata::{GameIgdb, Metadata},
};

/// Ids cross as strings; anything that is not one is refused, and so is
/// more than one request's worth.
fn parse(ids: &[String]) -> Result<Vec<u64>, CommandError> {
    if ids.len() > igdb::BATCH {
        return Err(refused(caskade_core::tr!(
            "at most {count} IGDB ids at a time",
            count = igdb::BATCH
        )));
    }
    ids.iter()
        .map(|id| {
            id.trim().parse().map_err(|_| {
                refused(caskade_core::tr!(
                    "{id} is not an IGDB id",
                    id = format!("{id:?}")
                ))
            })
        })
        .collect()
}

/// IGDB's best matches for the title `query`, best first, at most
/// `limit`. Always asks the server.
pub async fn igdb_search(
    igdb: &Igdb,
    query: &str,
    limit: u32,
) -> Result<Vec<IgdbGame>, CommandError> {
    igdb.search(query, limit as usize).await.map_err(report)
}

/// A library game's IGDB match and its IGDB game; `None` before matching
/// has looked at the game.
pub async fn game_igdb(
    metadata: &Metadata,
    game_id: &str,
) -> Result<Option<GameIgdb>, CommandError> {
    metadata.game(game_id).await.map_err(report)
}

/// Matches a library game to an IGDB game by hand, or to none with
/// `None`, and fills the game in from it.
pub async fn game_igdb_set(
    metadata: &Metadata,
    game_id: &str,
    igdb_id: Option<String>,
) -> Result<GameIgdb, CommandError> {
    crate::permissions::enforce("game_igdb_set", serde_json::Value::default)?;
    let igdb_id = match igdb_id {
        Some(id) => Some(parse(std::slice::from_ref(&id))?[0]),
        None => None,
    };
    metadata.set(game_id, igdb_id).await.map_err(report)
}

/// Forgets a library game's match and matches it automatically again.
pub async fn game_igdb_rematch(
    metadata: &Metadata,
    game_id: &str,
) -> Result<Option<GameIgdb>, CommandError> {
    crate::permissions::enforce("game_igdb_rematch", serde_json::Value::default)?;
    metadata.rematch(game_id).await.map_err(report)
}

/// Fetches a library game's IGDB game again and fills the game in from it.
pub async fn game_igdb_refresh(
    metadata: &Metadata,
    game_id: &str,
) -> Result<Option<GameIgdb>, CommandError> {
    crate::permissions::enforce("game_igdb_refresh", serde_json::Value::default)?;
    metadata.refresh(game_id).await.map_err(report)
}

/// Moves an entry out of its game into a game of its own, which IGDB
/// matching never merges back: for releases merged as one game that aren't
/// the same game. The new game's id.
pub async fn game_separate_entry(
    metadata: &Metadata,
    entry_id: &str,
) -> Result<String, CommandError> {
    crate::permissions::enforce("game_separate_entry", serde_json::Value::default)?;
    metadata.separate(entry_id).await.map_err(report)
}

/// A page of the IGDB catalog: games matching a search, or the most-rated
/// without a query. Kept a few minutes; `stale` when it is the last copy
/// because the server couldn't be reached.
pub async fn igdb_catalog_search(
    igdb: &Igdb,
    search: &CatalogSearch,
) -> Result<IgdbCatalogPage, CommandError> {
    igdb.catalog_search(search).await.map_err(report)
}

/// A page of the last 90 days' releases, newest first.
pub async fn igdb_catalog_recent(
    igdb: &Igdb,
    releases: &CatalogReleases,
) -> Result<IgdbCatalogPage, CommandError> {
    igdb.catalog_recent(releases).await.map_err(report)
}

/// A page of releases to come, soonest first; with `tbd`, undated ones.
pub async fn igdb_catalog_upcoming(
    igdb: &Igdb,
    releases: &CatalogReleases,
) -> Result<IgdbCatalogPage, CommandError> {
    igdb.catalog_upcoming(releases).await.map_err(report)
}

/// The platforms and genres the catalog can be narrowed by.
pub async fn igdb_catalog_filters(igdb: &Igdb) -> Result<IgdbCatalogFilters, CommandError> {
    igdb.catalog_filters().await.map_err(report)
}

/// Games for the Store's pages: cached answers younger than a day, the
/// server for the rest, and the cached answer when it can't be reached.
pub async fn igdb_catalog_games(igdb: &Igdb, ids: &[String]) -> Result<IgdbLookup, CommandError> {
    igdb.catalog_games(&parse(ids)?).await.map_err(report)
}

/// The library games matched to IGDB games `ids`: what the Store shows as
/// in the library or installed.
pub async fn igdb_owned(
    store: &LibraryStore,
    ids: &[String],
) -> Result<Vec<IgdbOwned>, CommandError> {
    store.igdb_owned(&parse(ids)?).await.map_err(report)
}

/// A library game's age ratings and the minimum age they come to; `None`
/// for no such game.
pub async fn game_age_ratings(
    store: &LibraryStore,
    game_id: &str,
) -> Result<Option<GameAgeRatings>, CommandError> {
    store.game_age_ratings(game_id).await.map_err(report)
}
