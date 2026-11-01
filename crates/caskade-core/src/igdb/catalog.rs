//! The Store's catalog (#163): all of IGDB through the metadata server's
//! `/catalog/*` endpoints, never only the games the library has.
//!
//! Pages are kept in memory, keyed by the request, for a short while:
//! search and browse pages for [`SEARCH_TTL`], recent and upcoming
//! releases for [`RELEASES_TTL`], the filters for [`FILTERS_TTL`] (the
//! server keeps its own for 10 min, 1 h and 1 day). Requests for the same
//! page at the same time share one request. A page that can't be fetched
//! (the server failed, or requests are paused after a failure) is answered
//! with the last copy of it, marked `stale`; without one, the failure is
//! the answer. The games a page brings go into the game cache, so opening
//! one costs no request; [`Igdb::catalog_games`] fetches answers older than
//! a day again, so release dates and store links follow IGDB.
//!
//! Recent releases are the last 90 days' (UTC, to the request), dated to
//! the day, newest first; upcoming ones are dated after now, soonest first,
//! keeping month, quarter and year precision and cancelled releases;
//! `tbd` lists the undated ones. Both list a game once per release, so a
//! game released on one platform and coming to another is on both. The
//! server's paging counts release rows: a game can come up again on a
//! later page for another platform, and the webview keeps the first.

use super::{Igdb, IgdbGame, IgdbReleaseDate, answer, decimal, decimals, readable, unique};
use crate::{Error, Result, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) const SEARCH_TTL: Duration = Duration::from_secs(5 * 60);
pub(super) const RELEASES_TTL: Duration = Duration::from_secs(30 * 60);
pub(super) const FILTERS_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Pages kept for stale answers; the oldest go first.
const MAX_PAGES: usize = 128;
/// The server's limits.
const MAX_LIMIT: u32 = 50;
const DEFAULT_LIMIT: u32 = 20;
const MAX_OFFSET: u32 = 5000;
const MAX_FILTER_IDS: usize = 50;

/// Ordering for catalog browse/search; relevance keeps IGDB's default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CatalogSort {
    #[default]
    Relevance,
    MostRated,
    Popular,
    Newest,
}

/// A catalog search, or a browse without `query` (most-rated first).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSearch {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub sort: CatalogSort,
    #[serde(default)]
    pub offset: u32,
    /// 1–50; 0 for the server's 20.
    #[serde(default)]
    pub limit: u32,
    /// IGDB platform ids (`IgdbCatalogFilters`); a game on any matches.
    #[serde(default, with = "decimals")]
    #[specta(type = Vec<String>)]
    pub platforms: Vec<u64>,
    #[serde(default, with = "decimals")]
    #[specta(type = Vec<String>)]
    pub genres: Vec<u64>,
    /// First released on or after this UTC day, `YYYY-MM-DD`.
    #[serde(default)]
    pub released_from: Option<String>,
    /// First released on or before this UTC day, `YYYY-MM-DD`.
    #[serde(default)]
    pub released_to: Option<String>,
}

/// A page of recent or upcoming releases.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogReleases {
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: u32,
    /// The release's own platform.
    #[serde(default, with = "decimals")]
    #[specta(type = Vec<String>)]
    pub platforms: Vec<u64>,
    #[serde(default, with = "decimals")]
    #[specta(type = Vec<String>)]
    pub genres: Vec<u64>,
    /// Upcoming only: undated (TBD) releases instead of dated ones.
    #[serde(default)]
    pub tbd: bool,
}

/// A page of the catalog.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbCatalogPage {
    pub games: Vec<IgdbGame>,
    /// Recent and upcoming: the release that put each game on the page.
    pub releases: Vec<IgdbListedRelease>,
    /// The offset of the next page; `None` on the last. There is no
    /// total: IGDB's catalog is too large to count.
    pub next_offset: Option<u32>,
    /// The last copy of the page: the server couldn't be reached.
    pub stale: bool,
}

/// The release that listed a game.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbListedRelease {
    #[serde(with = "decimal")]
    #[specta(type = String)]
    pub game: u64,
    pub release: IgdbReleaseDate,
}

/// What the catalog can be narrowed by.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbCatalogFilters {
    pub platforms: Vec<IgdbFilterOption>,
    pub genres: Vec<IgdbFilterOption>,
    /// The last copy: the server couldn't be reached.
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbFilterOption {
    #[serde(with = "decimal")]
    #[specta(type = String)]
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub abbreviation: Option<String>,
}

/// A catalog page as the server sends it. `games` is required, so stray
/// JSON is a failure; releases are read one by one, so an odd one doesn't
/// sink the page.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawPage {
    games: Vec<Value>,
    #[serde(default)]
    releases: Vec<Value>,
    #[serde(default)]
    next_offset: Option<u32>,
}

#[derive(Deserialize)]
struct RawRelease {
    game: u64,
    #[serde(flatten)]
    release: IgdbReleaseDate,
}

/// What `/catalog/search` is asked: ids as numbers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<&'a str>,
    #[serde(skip_serializing_if = "is_relevance")]
    sort: CatalogSort,
    offset: u32,
    limit: u32,
    platforms: &'a [u64],
    genres: &'a [u64],
    #[serde(skip_serializing_if = "Option::is_none")]
    released_from: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    released_to: Option<&'a str>,
}

/// What `/catalog/recent` and `/catalog/upcoming` are asked.
#[derive(Debug, Serialize)]
pub(super) struct ReleasesBody<'a> {
    offset: u32,
    limit: u32,
    platforms: &'a [u64],
    genres: &'a [u64],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    tbd: bool,
}

/// Pages kept in memory, and the requests for them under way.
pub(super) struct Pages<T> {
    /// Each page and when it came; `None` once expired (tests).
    kept: std::sync::Mutex<HashMap<String, (Option<Instant>, T)>>,
    flights: std::sync::Mutex<HashMap<String, super::Flight>>,
}

impl<T> Default for Pages<T> {
    fn default() -> Self {
        Self {
            kept: std::sync::Mutex::default(),
            flights: std::sync::Mutex::default(),
        }
    }
}

/// An answer that can say it is the last copy.
pub(super) trait Stale {
    fn stale(self) -> Self;
}

impl Stale for IgdbCatalogPage {
    fn stale(mut self) -> Self {
        self.stale = true;
        self
    }
}

impl Stale for IgdbCatalogFilters {
    fn stale(mut self) -> Self {
        self.stale = true;
        self
    }
}

impl<T: Clone + Stale> Pages<T> {
    fn kept(&self, key: &str, ttl: Option<Duration>) -> Option<T> {
        let kept = crate::lock(&self.kept);
        let (at, value) = kept.get(key)?;
        ttl.is_none_or(|ttl| at.is_some_and(|at| at.elapsed() < ttl))
            .then(|| value.clone())
    }

    /// Expires every kept page, as if its time had passed.
    #[cfg(all(test, feature = "net"))]
    pub(super) fn expire(&self) {
        let mut kept = crate::lock(&self.kept);
        for (at, _) in kept.values_mut() {
            *at = None;
        }
    }

    fn keep(&self, key: String, value: T) {
        let mut kept = crate::lock(&self.kept);
        kept.insert(key, (Some(Instant::now()), value));
        while kept.len() > MAX_PAGES {
            let Some(oldest) = kept
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            kept.remove(&oldest);
        }
    }

    /// The page `key`: kept if younger than `ttl`, else from `request`,
    /// shared with any other request for it under way, else the last copy.
    async fn get<F>(&self, key: String, ttl: Duration, request: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        if let Some(page) = self.kept(&key, Some(ttl)) {
            return Ok(page);
        }
        let flight = {
            let mut flights = crate::lock(&self.flights);
            Arc::clone(flights.entry(key.clone()).or_default())
        };
        let guard = flight.lock().await;
        // The request this one waited for brought it.
        let result = match self.kept(&key, Some(ttl)) {
            Some(page) => Ok(page),
            None => match request.await {
                Ok(page) => {
                    self.keep(key.clone(), page.clone());
                    Ok(page)
                }
                Err(error) => match self.kept(&key, None) {
                    Some(page) => {
                        tracing::debug!(%error, "catalog page answered from its last copy");
                        Ok(page.stale())
                    }
                    None => Err(error),
                },
            },
        };
        drop(guard);
        let mut flights = crate::lock(&self.flights);
        if flights
            .get(&key)
            .is_some_and(|other| Arc::ptr_eq(other, &flight))
        {
            flights.remove(&key);
        }
        result
    }
}

fn check_ids(ids: &[u64], what: &str) -> Result<()> {
    if ids.len() > MAX_FILTER_IDS {
        return Err(Error::Invalid(if what == "genres" {
            crate::tr!("at most {max} genres at a time", max = MAX_FILTER_IDS)
        } else {
            crate::tr!("at most {max} platforms at a time", max = MAX_FILTER_IDS)
        }));
    }
    if let Some(id) = ids.iter().find(|id| i64::try_from(**id).is_err()) {
        return Err(Error::Invalid(crate::tr!(
            "{id} is not an IGDB id",
            id = id
        )));
    }
    Ok(())
}

fn page(offset: u32, limit: u32) -> Result<(u32, u32)> {
    if offset > MAX_OFFSET {
        return Err(Error::Invalid(crate::tr!(
            "the catalog pages only {max} games deep",
            max = MAX_OFFSET
        )));
    }
    let limit = if limit == 0 { DEFAULT_LIMIT } else { limit };
    Ok((offset, limit.min(MAX_LIMIT)))
}

fn day(date: Option<&str>) -> Result<Option<&str>> {
    let date = date.map(str::trim).filter(|date| !date.is_empty());
    if let Some(date) = date {
        crate::model::validation::validate_date(date).map_err(|_| {
            Error::Invalid(crate::tr!(
                "{date} is not a YYYY-MM-DD date",
                date = format!("{date:?}")
            ))
        })?;
    }
    Ok(date)
}

impl Igdb {
    /// Expires the kept catalog pages and filters.
    #[cfg(all(test, feature = "net"))]
    pub(crate) fn expire_catalog(&self) {
        self.inner.pages.expire();
        self.inner.filters.expire();
    }

    /// A page of the catalog's games matching `search`, best first, or the
    /// most-rated ones without a query.
    pub async fn catalog_search(&self, search: &CatalogSearch) -> Result<IgdbCatalogPage> {
        let (offset, limit) = page(search.offset, search.limit)?;
        check_ids(&search.platforms, "platforms")?;
        check_ids(&search.genres, "genres")?;
        let body = SearchBody {
            query: search
                .query
                .as_deref()
                .map(str::trim)
                .filter(|query| !query.is_empty()),
            sort: search.sort,
            offset,
            limit,
            platforms: &unique(&search.platforms),
            genres: &unique(&search.genres),
            released_from: day(search.released_from.as_deref())?,
            released_to: day(search.released_to.as_deref())?,
        };
        self.catalog_page("/catalog/search", serde_json::to_value(&body)?, SEARCH_TTL)
            .await
    }

    /// A page of the releases of the last 90 days, newest first.
    pub async fn catalog_recent(&self, releases: &CatalogReleases) -> Result<IgdbCatalogPage> {
        let request = CatalogReleases {
            tbd: false,
            ..releases.clone()
        };
        self.catalog_releases("/catalog/recent", &request).await
    }

    /// A page of releases to come, soonest first; with `tbd`, undated ones.
    pub async fn catalog_upcoming(&self, releases: &CatalogReleases) -> Result<IgdbCatalogPage> {
        self.catalog_releases("/catalog/upcoming", releases).await
    }

    /// The platforms and genres the catalog can be narrowed by.
    pub async fn catalog_filters(&self) -> Result<IgdbCatalogFilters> {
        let request = async {
            self.check_pause()?;
            let mut filters = self.outcome(super::fetch::filters(&self.inner.base).await)?;
            filters.stale = false;
            Ok(filters)
        };
        self.inner
            .filters
            .get("filters".to_owned(), FILTERS_TTL, request)
            .await
    }

    /// Games for the Store's pages: like [`Igdb::games`], but answers older
    /// than a day are fetched again; when that fails, the cached answer
    /// stands. Fails only when nothing could be answered.
    pub async fn catalog_games(&self, ids: &[u64]) -> Result<super::IgdbLookup> {
        let ids = unique(ids);
        let store = self.store().await?;
        let fresh = super::CATALOG_FRESH
            .as_millis()
            .try_into()
            .unwrap_or(i64::MAX);
        let since = now_ms().saturating_sub(fresh);
        let mut known = store.get(&store.fetched_since(&ids, since).await?).await?;
        let stale: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| !known.contains_key(id))
            .collect();
        let (claim, waits) = self.claim(&stale);
        let fetched = self.fetch(store, &claim.ids, &mut known).await;
        drop(claim);
        for flight in waits {
            drop(flight.lock().await);
        }
        // What the server didn't answer: the cache's answer, however old.
        let rest: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| !known.contains_key(id))
            .collect();
        known.extend(store.get(&rest).await?);
        match fetched {
            Err(error) if known.is_empty() => Err(error),
            _ => Ok(answer(&ids, known)),
        }
    }

    async fn catalog_releases(
        &self,
        path: &'static str,
        releases: &CatalogReleases,
    ) -> Result<IgdbCatalogPage> {
        let (offset, limit) = page(releases.offset, releases.limit)?;
        check_ids(&releases.platforms, "platforms")?;
        check_ids(&releases.genres, "genres")?;
        let body = ReleasesBody {
            offset,
            limit,
            platforms: &unique(&releases.platforms),
            genres: &unique(&releases.genres),
            tbd: releases.tbd,
        };
        self.catalog_page(path, serde_json::to_value(&body)?, RELEASES_TTL)
            .await
    }

    async fn catalog_page(
        &self,
        path: &'static str,
        body: Value,
        ttl: Duration,
    ) -> Result<IgdbCatalogPage> {
        let key = format!("{path} {body}");
        let request = async {
            let store = self.store().await?;
            self.check_pause()?;
            let raw = self.outcome(super::fetch::catalog(&self.inner.base, path, &body).await)?;
            let games = readable(raw.games);
            store.put(&games, &[]).await?;
            let listed: HashSet<u64> = games.iter().map(|(game, _)| game.id).collect();
            let releases = raw
                .releases
                .into_iter()
                .filter_map(|raw| serde_json::from_value::<RawRelease>(raw).ok())
                .filter(|release| listed.contains(&release.game))
                .map(|release| IgdbListedRelease {
                    game: release.game,
                    release: release.release,
                })
                .collect();
            Ok(IgdbCatalogPage {
                games: games.into_iter().map(|(game, _)| game).collect(),
                releases,
                next_offset: raw.next_offset,
                stale: false,
            })
        };
        self.inner.pages.get(key, ttl, request).await
    }
}

fn is_relevance(sort: &CatalogSort) -> bool {
    *sort == CatalogSort::Relevance
}
