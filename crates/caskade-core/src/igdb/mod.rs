//! IGDB game metadata through Caskade's metadata server (caskade-server),
//! which holds the Twitch credentials IGDB needs and reshapes its rows into
//! the fields the UI uses.
//!
//! [`Igdb`] keeps every answer in a local SQLite file, games and the ids
//! IGDB has no game for alike, and never asks the server about an id it
//! already has an answer for. Lookups of the same id at the same time
//! share one request: the later ones wait for it and read its answer from
//! the cache. Lookups of other ids don't wait. [`Igdb::refresh`] is the
//! only way to fetch an id again, and it leaves answers younger than ten
//! minutes alone.
//!
//! A failed request pauses every request for a minute, or for as long as
//! the server's `Retry-After` asks (up to six hours); until then they
//! fail without asking. An id the server leaves unanswered, or answers
//! with a game this build can't read, is not asked about again for six
//! hours.
//!
//! [`Igdb::match_stores`] asks the server which IGDB game a store id (a
//! Steam app id, a GOG product id, …) belongs to. Those answers are not
//! kept here: the library keeps the matches it makes.
//!
//! [`Igdb::search`] asks the server for games by title. Searches always
//! go to the server; the games they find are cached like any other.
//!
//! The Store's catalog (`catalog.rs`: search, recent and upcoming
//! releases, filters) is kept in memory for a short while instead, and
//! [`Igdb::catalog_games`] fetches answers older than a day again, so
//! release dates and store links don't stay stale.
//!
//! Answers cached before a change to the game's shape worth fetching for
//! (age ratings, #204) are outdated (`store.rs`): they still answer, but
//! the next lookup that needs them fetches them again, once, under the
//! same pause and backoff. A failed fetch keeps them.

pub mod age;
mod catalog;
#[cfg(feature = "net")]
mod fetch;
pub(crate) mod popularity;
#[cfg(all(test, feature = "net"))]
mod popularity_tests;
mod store;
#[cfg(all(test, feature = "net"))]
mod tests;

use crate::{Error, Result, now_ms};
pub use catalog::{
    CatalogReleases, CatalogSearch, CatalogSort, IgdbCatalogFilters, IgdbCatalogPage,
    IgdbFilterOption, IgdbListedRelease,
};
use catalog::{Pages, RawPage};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use specta::Type;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use store::Store;
use tokio::sync::{Mutex, OnceCell, OwnedMutexGuard};

pub const BASE_URL: &str = "https://caskade-server-production.up.railway.app";
/// The most ids (game or store) the server takes in one request, and the
/// most the webview may ask for at once.
pub const BATCH: usize = 500;
/// How long requests wait after a failure without a `Retry-After`.
const PAUSE: Duration = Duration::from_secs(60);
/// The longest `Retry-After` honoured.
const MAX_PAUSE: Duration = Duration::from_secs(6 * 60 * 60);
/// How long an id without a usable answer waits to be asked again.
const UNANSWERED: Duration = Duration::from_secs(6 * 60 * 60);
/// How old an answer must be before [`Igdb::refresh`] fetches it again.
const FRESH: Duration = Duration::from_secs(10 * 60);
/// How old an answer may be for the Store's game pages before
/// [`Igdb::catalog_games`] fetches it again: release dates and store links
/// change.
const CATALOG_FRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// The most games one search returns.
pub const SEARCH_LIMIT: usize = 50;

/// A game as the metadata server describes it. Ids cross to the webview
/// as strings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbGame {
    #[serde(with = "decimal")]
    #[specta(type = String)]
    pub id: u64,
    #[serde(default)]
    pub slug: Option<String>,
    /// The game's IGDB page.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub storyline: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Themes, then game modes, then player perspectives.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<IgdbPlatform>,
    /// `YYYY-MM-DD` (UTC).
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// IGDB's aggregate rating (0–100), not the user's own.
    #[serde(default)]
    pub total_rating: Option<f64>,
    /// Store ids (Steam, GOG, Epic, …), for matching library entries.
    #[serde(default)]
    pub external_ids: Vec<IgdbExternalId>,
    /// IGDB's raw `game_type` reference.
    #[serde(default)]
    pub game_type: Option<Value>,
    /// Set on editions and other versions of a parent game.
    #[serde(default, with = "decimal_option")]
    #[specta(type = Option<String>)]
    pub version_parent: Option<u64>,
    #[serde(default)]
    pub images: IgdbImages,
    /// Each board's rating ([`age`] turns them into a minimum age). Empty
    /// when IGDB has none, and in answers from servers before ratings.
    #[serde(default)]
    pub age_ratings: Vec<IgdbAgeRating>,
    /// One per platform and region, dated ones first.
    #[serde(default)]
    pub release_dates: Vec<IgdbReleaseDate>,
    /// Storefront pages IGDB knows for the game. A link says nothing of
    /// whether it is on sale there, or at what price.
    #[serde(default)]
    pub store_links: Vec<IgdbStoreLink>,
    /// Every other website: official, wiki, social, video, community.
    #[serde(default)]
    pub websites: Vec<IgdbWebsite>,
    /// IGDB's similar games, without the game itself or duplicates.
    #[serde(default, with = "decimals")]
    #[specta(type = Vec<String>)]
    pub similar_games: Vec<u64>,
    /// IGDB's game status: `Released`, `Early Access`, `Cancelled`, …
    #[serde(default)]
    pub status: Option<String>,
    /// Trailers and other videos on YouTube, in IGDB's order. Empty in
    /// answers from servers before videos.
    #[serde(default)]
    pub videos: Vec<IgdbVideo>,
}

/// A video of the game on YouTube.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbVideo {
    /// The YouTube video id; the server sends only well-formed ones.
    pub video_id: String,
    /// IGDB's name for it: `Trailer`, `Gameplay video`, …
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbPlatform {
    /// IGDB's platform id, for the catalog's platform filter.
    #[serde(default, with = "decimal_option")]
    #[specta(type = Option<String>)]
    pub id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub abbreviation: Option<String>,
}

/// One board's rating: `board` is `pegi`, `esrb`, `usk`, `cero`, `acb`,
/// `grac`, `classind` or a board IGDB adds later; `rating` its label
/// (`18`, `M`, `E10+`, `MA15+`, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbAgeRating {
    pub board: String,
    pub rating: String,
}

/// How much of a release date is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ReleasePrecision {
    Day,
    Month,
    Quarter,
    Year,
    /// Undated, or a precision this build doesn't know.
    #[default]
    #[serde(other)]
    Tbd,
}

/// A release on one platform in one region.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbReleaseDate {
    #[serde(default)]
    pub platform: Option<IgdbPlatform>,
    /// IGDB's region name: `europe`, `north_america`, `worldwide`, …
    #[serde(default)]
    pub region: Option<String>,
    /// The UTC day IGDB files it under (`YYYY-MM-DD`). For month, quarter
    /// and year precision it stands in for the whole span: sort by it, but
    /// show only what `precision` covers.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub precision: ReleasePrecision,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub month: Option<u8>,
    /// 1–4, for quarter precision.
    #[serde(default)]
    pub quarter: Option<u8>,
    /// IGDB's own text for it ("Q3 2026", "TBD").
    #[serde(default)]
    pub human: Option<String>,
    /// `Full Release`, `Early Access`, `Cancelled`, …
    #[serde(default)]
    pub status: Option<String>,
}

/// A storefront's page for the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbStoreLink {
    /// `steam`, `gog`, `epic`, `itch`, `playstation`, `nintendo`, …
    pub store: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
    /// Platform names it is for; empty when IGDB doesn't say.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// ISO 3166-1 numeric codes it is listed for; empty when IGDB doesn't say.
    #[serde(default)]
    pub countries: Vec<u32>,
}

/// Any other website of the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbWebsite {
    /// `official`, `wiki`, `wikipedia`, `social`, `video`, `community` or `other`.
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbExternalId {
    pub uid: String,
    /// IGDB's raw `external_game_source` reference.
    #[serde(default)]
    pub source: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbImages {
    #[serde(default)]
    pub cover: Option<IgdbImage>,
    /// The best banner among the artworks.
    #[serde(default)]
    pub hero: Option<IgdbImage>,
    #[serde(default)]
    pub logo: Option<IgdbImage>,
    #[serde(default)]
    pub artworks: Vec<IgdbImage>,
    #[serde(default)]
    pub screenshots: Vec<IgdbImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbImage {
    pub image_id: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub alpha_channel: Option<bool>,
    #[serde(default)]
    pub image_type: Option<Value>,
    /// The download URL, at the size the server picked for this kind.
    pub url: String,
}

/// What a lookup found.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbLookup {
    /// In the order asked for, without duplicates.
    pub games: Vec<IgdbGame>,
    /// Ids IGDB has no game for.
    #[serde(with = "decimals")]
    #[specta(type = Vec<String>)]
    pub not_found: Vec<u64>,
    /// Ids without an answer yet: the server couldn't be reached or sent nothing usable for them.
    #[serde(with = "decimals")]
    #[specta(type = Vec<String>)]
    pub pending: Vec<u64>,
}

/// The server's answer to `POST /games` and `POST /search`, each game kept
/// as sent. Both lists are required, so that a proxy's error page or any
/// other stray JSON is a failure, not an answer of "no such games".
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    games: Vec<Value>,
    not_found: Vec<u64>,
}

/// What `POST /search` is asked.
#[derive(Debug, Serialize)]
struct SearchRequest<'a> {
    query: &'a str,
    limit: usize,
}

/// A game's id in a store IGDB links to. `store` is the server's name for
/// it: `steam`, `gog`, `epic`, `itch`, `amazon`, `amazon_adg` or
/// `microsoft`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoreId {
    pub store: String,
    pub uid: String,
}

impl StoreId {
    pub fn new(store: &str, uid: impl Into<String>) -> Self {
        Self {
            store: store.to_owned(),
            uid: uid.into(),
        }
    }
}

/// The server's answer to `POST /match`. `matches` is required: a body
/// without it is a failure, not "no matches" (which would be saved for a
/// week).
#[derive(Debug, Deserialize)]
struct MatchResponse {
    matches: Vec<StoreMatch>,
}

#[derive(Debug, Deserialize)]
struct StoreMatch {
    store: String,
    uid: String,
    game: u64,
}

/// The metadata server's client and its local cache.
#[derive(Clone)]
pub struct Igdb {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    base: String,
    store: OnceCell<Store>,
    /// The ids being fetched, each with its fetch's lock, held until the
    /// answer is cached; a lookup of the same id waits for it rather than
    /// fetching the id twice.
    inflight: std::sync::Mutex<HashMap<u64, Flight>>,
    /// No request before this, after a failure.
    paused_until: std::sync::Mutex<Option<Instant>>,
    /// Ids without a usable answer, and when they may be asked about again.
    unanswered: std::sync::Mutex<HashMap<u64, Instant>>,
    /// The catalog's pages, kept a short while (`catalog.rs`).
    pages: Pages<IgdbCatalogPage>,
    filters: Pages<IgdbCatalogFilters>,
    /// Serializes background popularity refreshes, separate from game lookups.
    popularity: Mutex<()>,
    /// No popularity request before this: a server without popularity yet
    /// (#253). Other requests go on.
    popularity_paused_until: std::sync::Mutex<Option<Instant>>,
    /// The server answered a popularity request: it has popularity and the
    /// catalog's sorts (#253).
    popularity_offered: std::sync::atomic::AtomicBool,
}

impl Igdb {
    /// Keeps the cache in `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_base_url(dir, BASE_URL)
    }

    /// Asks another server (tests, self-hosting).
    pub fn with_base_url(dir: impl Into<PathBuf>, base: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                dir: dir.into(),
                base: base.into().trim_end_matches('/').to_owned(),
                store: OnceCell::new(),
                inflight: std::sync::Mutex::default(),
                paused_until: std::sync::Mutex::default(),
                unanswered: std::sync::Mutex::default(),
                pages: Pages::default(),
                filters: Pages::default(),
                popularity: Mutex::new(()),
                popularity_paused_until: std::sync::Mutex::default(),
                popularity_offered: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    async fn store(&self) -> Result<&Store> {
        self.inner
            .store
            // A damaged cache is started again, not an error forever (#160).
            .get_or_try_init(|| {
                crate::cache_db::open(self.inner.dir.join("games.sqlite"), "IGDB", Store::open)
            })
            .await
    }

    /// The games with `ids`: from the cache, and from the server for the
    /// ids the cache has no answer for yet. Ids still without an answer
    /// are `pending`. Fails only when the server failed and none of `ids`
    /// could be answered.
    pub async fn games(&self, ids: &[u64]) -> Result<IgdbLookup> {
        let (lookup, failure, _) = self.lookup(ids).await?;
        match failure {
            Some(error) if lookup.games.is_empty() && lookup.not_found.is_empty() => Err(error),
            _ => Ok(lookup),
        }
    }

    /// [`Igdb::games`], with the server's failure (if any) beside what
    /// could be answered without it, and those of the games that are
    /// outdated answers still (their fetch failed or waits).
    pub(crate) async fn lookup(
        &self,
        ids: &[u64],
    ) -> Result<(IgdbLookup, Option<Error>, HashSet<u64>)> {
        let ids = unique(ids);
        let store = self.store().await?;
        let mut known = store.get(&ids).await?;
        let mut outdated: HashSet<u64> = store.outdated(&ids).await?.into_iter().collect();
        let mut failure = None;
        // A second round fetches what a lookup this one waited for left
        // unanswered (it failed); no more, so a lookup always ends.
        for _ in 0..2 {
            let due = self.due(&ids, &known, &outdated);
            if due.is_empty() {
                break;
            }
            let (claim, waits) = self.claim(&due);
            if !claim.ids.is_empty() {
                // Another lookup may have fetched some just before the claim.
                known.extend(store.get(&claim.ids).await?);
                reread(store, &claim.ids, &mut outdated).await?;
                let missing = self.due(&claim.ids, &known, &outdated);
                tracing::debug!(
                    cached = ids.len() - due.len(),
                    fetching = missing.len(),
                    waiting = waits.len(),
                    "IGDB lookup"
                );
                if let Err(error) = self.fetch(store, &missing, &mut known).await {
                    failure = Some(error);
                }
                reread(store, &missing, &mut outdated).await?;
            }
            drop(claim);
            if waits.is_empty() || failure.is_some() {
                break;
            }
            for flight in waits {
                drop(flight.lock().await);
            }
            let waited = self.due(&ids, &known, &outdated);
            known.extend(store.get(&waited).await?);
            reread(store, &waited, &mut outdated).await?;
        }
        outdated.retain(|id| matches!(known.get(id), Some(Some(_))));
        Ok((answer(&ids, known), failure, outdated))
    }

    /// Fetches `ids` again, replacing what the cache has for them, except
    /// answers fetched in the last ten minutes.
    pub async fn refresh(&self, ids: &[u64]) -> Result<IgdbLookup> {
        let ids = unique(ids);
        let store = self.store().await?;
        let since = now_ms().saturating_sub(FRESH.as_millis().try_into().unwrap_or(i64::MAX));
        let fresh = store.fetched_since(&ids, since).await?;
        let mut known = store.get(&fresh).await?;
        let stale: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| !known.contains_key(id))
            .collect();
        let (claim, waits) = self.claim(&stale);
        let fetched = self.fetch(store, &claim.ids, &mut known).await;
        drop(claim);
        // Ids another lookup was fetching are fresh once it is done.
        for flight in waits {
            drop(flight.lock().await);
        }
        let rest: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| !known.contains_key(id))
            .collect();
        known.extend(store.get(&store.fetched_since(&rest, since).await?).await?);
        fetched?;
        Ok(answer(&ids, known))
    }

    /// Claims those of `ids` no other lookup is fetching, and returns the
    /// fetches to wait for for the rest.
    fn claim(&self, ids: &[u64]) -> (Claim<'_>, Vec<Flight>) {
        let flight = Flight::default();
        let Ok(guard) = Arc::clone(&flight).try_lock_owned() else {
            unreachable!("a new lock is free");
        };
        let mut inflight = crate::lock(&self.inner.inflight);
        let mut mine = Vec::new();
        let mut waits: Vec<Flight> = Vec::new();
        for &id in ids {
            match inflight.get(&id) {
                Some(other) => {
                    if !waits.iter().any(|wait| Arc::ptr_eq(wait, other)) {
                        waits.push(Arc::clone(other));
                    }
                }
                None => {
                    inflight.insert(id, Arc::clone(&flight));
                    mine.push(id);
                }
            }
        }
        drop(inflight);
        let claim = Claim {
            inflight: &self.inner.inflight,
            flight,
            ids: mine,
            _guard: guard,
        };
        (claim, waits)
    }

    /// The IGDB game of each of `ids` the server has one for. Asked in
    /// batches; a failure loses only the batches not yet answered, but
    /// returns an error.
    pub async fn match_stores(&self, ids: &[StoreId]) -> Result<HashMap<StoreId, u64>> {
        let mut seen = HashSet::new();
        let ids: Vec<&StoreId> = ids.iter().filter(|id| seen.insert(*id)).collect();
        let mut found = HashMap::new();
        for batch in ids.chunks(BATCH) {
            let batch: Vec<StoreId> = batch.iter().map(|id| (*id).clone()).collect();
            self.check_pause()?;
            let response = self.outcome(fetch::matches(&self.inner.base, &batch).await)?;
            let asked: HashSet<&StoreId> = batch.iter().collect();
            for found_match in response.matches {
                let id = StoreId {
                    store: found_match.store,
                    uid: found_match.uid,
                };
                // Only ids asked for, and only ids IGDB can have.
                if asked.contains(&id) && i64::try_from(found_match.game).is_ok() {
                    found.insert(id, found_match.game);
                }
            }
        }
        Ok(found)
    }

    /// IGDB's best matches for the title `query`, best first, at most
    /// `limit` (up to [`SEARCH_LIMIT`]). Editions are left out. An empty
    /// query finds nothing without asking.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<IgdbGame>> {
        let query = query.trim();
        let limit = limit.min(SEARCH_LIMIT);
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let store = self.store().await?;
        self.check_pause()?;
        let request = SearchRequest { query, limit };
        let response = self.outcome(fetch::search(&self.inner.base, &request).await)?;
        let games = readable(response.games);
        store.put(&games, &[]).await?;
        Ok(games.into_iter().map(|(game, _)| game).collect())
    }

    /// Game `id` from the cache alone, never asking the server.
    pub async fn cached_game(&self, id: u64) -> Result<Option<IgdbGame>> {
        Ok(self.store().await?.get(&[id]).await?.remove(&id).flatten())
    }

    /// How many ids the cache has an answer for.
    pub async fn cached(&self) -> Result<u64> {
        self.store().await?.count().await
    }

    /// Those of `ids` the cache has an answer for, a game or "none".
    pub async fn answered(&self, ids: &[u64]) -> Result<HashSet<u64>> {
        Ok(self.store().await?.get(ids).await?.into_keys().collect())
    }

    /// How long until the server may be asked again, while paused after a
    /// failure.
    pub fn retry_in(&self) -> Option<Duration> {
        let paused_until = *crate::lock(&self.inner.paused_until);
        paused_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|wait| !wait.is_zero())
    }

    /// Caches `game` as an answer of an older shape, fetched long ago, as a
    /// build before age ratings would have.
    #[cfg(all(test, feature = "net"))]
    pub(crate) async fn cache_outdated(&self, id: u64, game: &Value) {
        let store = self.store().await.unwrap();
        store.put_shape(id, game, 1).await.unwrap();
    }

    /// Ends a pause, as if its time had passed.
    #[cfg(all(test, feature = "net"))]
    pub(crate) fn unpause(&self) {
        *crate::lock(&self.inner.paused_until) = None;
    }

    fn check_pause(&self) -> Result<()> {
        match self.retry_in() {
            Some(wait) => Err(Error::Io(std::io::Error::other(crate::tr!(
                "the metadata server is unavailable; trying again in {seconds}s",
                seconds = wait.as_secs().max(1)
            )))),
            None => Ok(()),
        }
    }

    /// Ends a pause on success, and starts one on failure.
    fn outcome<T>(&self, result: Result<T, fetch::Failure>) -> Result<T> {
        let mut paused_until = crate::lock(&self.inner.paused_until);
        match result {
            Ok(value) => {
                *paused_until = None;
                Ok(value)
            }
            Err(failure) => {
                let wait = failure.retry_after.unwrap_or(PAUSE).min(MAX_PAUSE);
                tracing::debug!(seconds = wait.as_secs(), "metadata server paused");
                *paused_until = Some(Instant::now() + wait);
                Err(failure.error)
            }
        }
    }

    /// Those of `ids` without an answer in `known`, or with an `outdated`
    /// one, that may be asked about.
    fn due(
        &self,
        ids: &[u64],
        known: &HashMap<u64, Option<IgdbGame>>,
        outdated: &HashSet<u64>,
    ) -> Vec<u64> {
        let mut unanswered = crate::lock(&self.inner.unanswered);
        let now = Instant::now();
        unanswered.retain(|_, until| *until > now);
        ids.iter()
            .copied()
            .filter(|id| {
                (!known.contains_key(id) || outdated.contains(id)) && !unanswered.contains_key(id)
            })
            .collect()
    }

    /// Asks the server for `ids` in batches and caches each batch as it
    /// arrives, so a failure keeps what earlier batches brought.
    async fn fetch(
        &self,
        store: &Store,
        ids: &[u64],
        known: &mut HashMap<u64, Option<IgdbGame>>,
    ) -> Result<()> {
        // IGDB ids are signed 64-bit, and a larger one fails the whole
        // request: answer those here, without asking or caching.
        let (ids, invalid): (Vec<u64>, Vec<u64>) =
            ids.iter().partition(|id| i64::try_from(**id).is_ok());
        known.extend(invalid.into_iter().map(|id| (id, None)));
        for batch in ids.chunks(BATCH) {
            self.check_pause()?;
            let response = self.outcome(fetch::games(&self.inner.base, batch).await)?;
            // A game this build can't read is answered as not found, and
            // asked for again later.
            let mut games = readable(response.games);
            // Only the ids asked for: an answer for anything else is not
            // the server's to give.
            let asked: HashSet<u64> = batch.iter().copied().collect();
            games.retain(|(game, _)| asked.contains(&game.id));
            let found: HashSet<u64> = games.iter().map(|(game, _)| game.id).collect();
            let not_found: Vec<u64> = response
                .not_found
                .into_iter()
                .filter(|id| asked.contains(id) && !found.contains(id))
                .collect();
            store.put(&games, &not_found).await?;
            let mut unanswered = crate::lock(&self.inner.unanswered);
            let until = Instant::now() + UNANSWERED;
            for id in batch {
                if found.contains(id) || not_found.contains(id) {
                    unanswered.remove(id);
                } else {
                    unanswered.insert(*id, until);
                }
            }
            drop(unanswered);
            known.extend(not_found.into_iter().map(|id| (id, None)));
            known.extend(games.into_iter().map(|(game, _)| (game.id, Some(game))));
        }
        Ok(())
    }
}

/// Rereads which of `ids` are outdated answers: a fetch replaced some.
async fn reread(store: &Store, ids: &[u64], outdated: &mut HashSet<u64>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let asked: HashSet<u64> = ids.iter().copied().collect();
    outdated.retain(|id| !asked.contains(id));
    outdated.extend(store.outdated(ids).await?);
    Ok(())
}

/// A fetch's lock: locked while it runs.
pub(crate) type Flight = Arc<Mutex<()>>;

/// Ids one lookup is fetching. Dropping it lets the lookups waiting for
/// them read the answers from the cache.
struct Claim<'a> {
    inflight: &'a std::sync::Mutex<HashMap<u64, Flight>>,
    flight: Flight,
    ids: Vec<u64>,
    /// Released after `drop` has taken the ids out of `inflight`.
    _guard: OwnedMutexGuard<()>,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        let mut inflight = crate::lock(self.inflight);
        for id in &self.ids {
            if inflight
                .get(id)
                .is_some_and(|flight| Arc::ptr_eq(flight, &self.flight))
            {
                inflight.remove(id);
            }
        }
    }
}

/// The games this build can read, each with the server's original. One it
/// can't must not sink the rest.
fn readable(raw: Vec<Value>) -> Vec<(IgdbGame, Value)> {
    raw.into_iter()
        .filter_map(
            |raw| match serde_json::from_value::<IgdbGame>(raw.clone()) {
                Ok(game) => Some((game, raw)),
                Err(error) => {
                    tracing::warn!(%error, "unreadable IGDB game skipped");
                    None
                }
            },
        )
        .collect()
}

/// `ids` without duplicates, in order.
fn unique(ids: &[u64]) -> Vec<u64> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// The answers for `ids`, in order. An id without one (the server failed,
/// or left it out of both lists) is pending.
fn answer(ids: &[u64], mut known: HashMap<u64, Option<IgdbGame>>) -> IgdbLookup {
    let mut lookup = IgdbLookup::default();
    for id in ids {
        match known.remove(id) {
            Some(Some(game)) => lookup.games.push(game),
            Some(None) => lookup.not_found.push(*id),
            None => lookup.pending.push(*id),
        }
    }
    lookup
}

/// An IGDB id as the server (a number) or the webview (a string) sends it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Decimal {
    Number(u64),
    Text(String),
}

impl Decimal {
    fn parse<E: de::Error>(self) -> Result<u64, E> {
        match self {
            Self::Number(id) => Ok(id),
            Self::Text(text) => text.parse().map_err(E::custom),
        }
    }
}

pub(crate) mod decimal {
    use super::*;

    pub fn serialize<S: Serializer>(id: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(id)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        Decimal::deserialize(deserializer)?.parse()
    }
}

pub(crate) mod decimal_option {
    use super::*;

    pub fn serialize<S: Serializer>(id: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        id.map(|id| id.to_string()).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Option::<Decimal>::deserialize(deserializer)?
            .map(Decimal::parse)
            .transpose()
    }
}

pub(crate) mod decimals {
    use super::*;

    pub fn serialize<S: Serializer>(ids: &[u64], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(ids.iter().map(u64::to_string))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u64>, D::Error> {
        Vec::<Decimal>::deserialize(deserializer)?
            .into_iter()
            .map(Decimal::parse)
            .collect()
    }
}

/// Builds without `net` answer from the cache alone.
#[cfg(not(feature = "net"))]
mod fetch {
    use super::{IgdbCatalogFilters, MatchResponse, RawPage, Response, SearchRequest, StoreId};
    use crate::Error;

    pub(super) struct Failure {
        pub(super) error: Error,
        pub(super) retry_after: Option<std::time::Duration>,
        pub(super) status: Option<u16>,
    }

    fn offline() -> Failure {
        Failure {
            error: Error::Io(std::io::Error::other(crate::tr!(
                "built without network support"
            ))),
            retry_after: None,
            status: None,
        }
    }

    pub(super) async fn popularity(
        _base: &str,
        _ids: &[u64],
    ) -> Result<super::popularity::PopularityResponse, Failure> {
        Err(offline())
    }

    pub(super) async fn games(_base: &str, _ids: &[u64]) -> Result<Response, Failure> {
        Err(offline())
    }

    pub(super) async fn matches(_base: &str, _ids: &[StoreId]) -> Result<MatchResponse, Failure> {
        Err(offline())
    }

    pub(super) async fn search(
        _base: &str,
        _request: &SearchRequest<'_>,
    ) -> Result<Response, Failure> {
        Err(offline())
    }

    pub(super) async fn catalog(
        _base: &str,
        _path: &str,
        _body: &serde_json::Value,
    ) -> Result<RawPage, Failure> {
        Err(offline())
    }

    pub(super) async fn filters(_base: &str) -> Result<IgdbCatalogFilters, Failure> {
        Err(offline())
    }
}
