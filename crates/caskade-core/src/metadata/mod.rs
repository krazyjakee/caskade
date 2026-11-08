//! IGDB metadata for the library's games.
//!
//! [`Metadata`] gives each library game an IGDB id and fills the game in
//! from it. An id comes from, in order: the game's RomM ROM (RomM's own
//! match), a name match in the Hasheous dumps of the game's platforms, and
//! the metadata server's match for the game's store ids (Steam, GOG, Epic,
//! …). The user can set one by hand, or say the game has none.
//!
//! Applying a match fills the description, genres, tags, release date,
//! aliases and age ratings (with the minimum age they come to,
//! `crate::igdb::age`), and adds IGDB's cover and icon; its hero and screenshots come
//! when the game's page first opens ([`Metadata::page_images`]). A field is only
//! filled when it is empty or still holds what the last apply wrote, so
//! values from the game's sources or the user stay; artwork of a kind the
//! game already has from elsewhere stays too, so a RomM game keeps RomM's
//! cover and screenshots and gets IGDB's hero. IGDB's icon joins an
//! installer's.
//!
//! [`Metadata::watch`] matches new games after library changes and now and
//! again. Games without a match are looked up in the Hasheous dumps again
//! once they change, and asked about on the metadata server again after a
//! week. A run stops at the server's first failure, or once an image host
//! can't be reached, and the next waits longer after each failed one: up
//! to an hour while the network is down, six hours while the server sends
//! nothing usable. An image that can never come (a 404, not an image)
//! leaves the game without it rather than pending.
//!
//! Images download outside the lock that orders changes to matches, so a
//! change on the game page never waits for a download.

mod apply;
#[cfg(feature = "net")]
mod epic;
mod find;
mod page;
mod selected;
mod watch;
pub use selected::{RefreshMode, RefreshStatus};

#[cfg(feature = "net")]
pub(crate) use apply::SOURCE as ARTWORK_SOURCE;
use apply::offline;
#[cfg(all(test, feature = "net"))]
mod tests;

use crate::{
    Error, Result,
    artwork::ArtworkCache,
    bus::{Bus, Event},
    db::LibraryStore,
    emulation::Catalog,
    hasheous::Hasheous,
    igdb::{Igdb, IgdbAgeRating, IgdbGame},
    model::ExternalId,
    now_ms,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

/// Games matched per round: one `/match` and one `/games` request each.
const CHUNK: usize = 50;
/// When games without a match are tried again, if the dumps stay the same.
const RETRY_NONE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How a game got its IGDB id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    /// RomM matched the game's ROM.
    Romm,
    /// A Hasheous dump has a game of that name.
    Hasheous,
    /// The metadata server matched a store id.
    Store,
    /// Another device's match for a game that came from it, sent with the
    /// game (#75): used when nothing here finds one.
    Device,
    /// The user chose it, or chose that there is none.
    Manual,
    /// Nothing found one.
    None,
}

impl MatchSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Romm => "romm",
            Self::Hasheous => "hasheous",
            Self::Store => "store",
            Self::Device => "device",
            Self::Manual => "manual",
            Self::None => "none",
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "romm" => Self::Romm,
            "hasheous" => Self::Hasheous,
            "store" => Self::Store,
            "device" => Self::Device,
            "manual" => Self::Manual,
            "none" => Self::None,
            other => {
                return Err(Error::Invalid(crate::tr!(
                    "unknown match source {source}",
                    source = format!("{other:?}")
                )));
            }
        })
    }
}

/// What an apply wrote into the library game; `None` (or empty) where it
/// left the game's own value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Applied {
    pub description: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub release_date: Option<String>,
    /// Aliases it added.
    pub aliases: Vec<String>,
    /// The age ratings it wrote (`games.age_ratings_json`).
    pub age_ratings: Option<Vec<IgdbAgeRating>>,
}

/// A library game's match, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct IgdbMatch {
    pub game_id: String,
    pub igdb_id: Option<u64>,
    pub source: MatchSource,
    /// Saved, but not applied to the game yet.
    pub pending: bool,
    pub checked_at_ms: i64,
    /// The Hasheous dumps a [`MatchSource::None`] was found against.
    pub dumps: String,
    pub applied: Applied,
}

impl IgdbMatch {
    fn new(game_id: &str, igdb_id: Option<u64>, source: MatchSource, dumps: &str) -> Self {
        Self {
            game_id: game_id.to_owned(),
            igdb_id,
            source,
            pending: true,
            checked_at_ms: now_ms(),
            dumps: dumps.to_owned(),
            applied: Applied::default(),
        }
    }
}

/// A game's match for the game page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameIgdb {
    /// The library game the match is on: not the one asked about when
    /// matching merged that one into a game that is the same game.
    pub game_id: String,
    #[serde(with = "crate::igdb::decimal_option")]
    #[specta(type = Option<String>)]
    pub igdb_id: Option<u64>,
    pub source: MatchSource,
    /// Matched, but the game isn't filled in yet (the server couldn't be
    /// reached).
    pub pending: bool,
    /// The IGDB game, when the cache has it or the server answered.
    pub game: Option<IgdbGame>,
}

/// What one [`Metadata::run`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// Games given an id (or found to have none) this run.
    pub matched: usize,
    pub none: usize,
    /// Games filled in.
    pub applied: usize,
    /// Games left for the next run: the server couldn't be reached or
    /// sent no game, or an image failed to download and may come later.
    pub deferred: usize,
    /// The deferred games with nothing recorded to show it: Epic's offers
    /// couldn't be read or the dumps searched, so "not found" isn't known.
    pub unanswered: Vec<String>,
    /// The metadata server failed or an image host couldn't be reached,
    /// so the run stopped there.
    pub unreachable: bool,
    /// Games merged into another that is the same game.
    pub merged: usize,
}

/// How far a [`Metadata::run`] is, shown on its background activity.
struct Progress {
    activity: caskade_jobs::Activity,
    done: usize,
    total: usize,
}

impl Progress {
    fn new(total: usize) -> Self {
        let progress = Self {
            activity: caskade_jobs::global().activity(crate::tr!("Matching games with IGDB")),
            done: 0,
            total,
        };
        progress.show();
        progress
    }

    /// One more game is being filled in.
    fn advance(&mut self) {
        self.reach(self.done + 1);
    }

    /// At least `done` games are through.
    fn reach(&mut self, done: usize) {
        let done = done.min(self.total);
        if done > self.done {
            self.done = done;
            self.show();
        }
    }

    fn show(&self) {
        let percent = self.done * 100 / self.total.max(1);
        self.activity.set_label(crate::tr!(
            "Matching games with IGDB · {done}/{total} ({percent}%)",
            done = self.done,
            total = self.total,
            percent = percent
        ));
    }
}

/// IGDB metadata for the library. Cheap to clone.
#[derive(Clone)]
pub struct Metadata {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    igdb: Igdb,
    hasheous: Hasheous,
    catalog: Arc<Catalog>,
    artwork: Option<ArtworkCache>,
    /// Epic games' store offers, which IGDB knows them by.
    #[cfg(feature = "net")]
    epic: epic::EpicOffers,
    /// One apply at a time, a game at a time. Held only for reads and
    /// writes of the library, never across a request.
    lock: Mutex<()>,
    /// Refreshing chosen games (#176).
    refresh: selected::RefreshJob,
    /// Games arrived without a library event (from another device, #75):
    /// [`Metadata::watch`] runs soon.
    wake: tokio::sync::Notify,
}

impl Metadata {
    pub fn new(
        store: LibraryStore,
        bus: Bus,
        igdb: Igdb,
        hasheous: Hasheous,
        catalog: Arc<Catalog>,
        artwork: Option<ArtworkCache>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                igdb,
                hasheous,
                catalog,
                artwork,
                #[cfg(feature = "net")]
                epic: epic::EpicOffers::default(),
                lock: Mutex::new(()),
                refresh: selected::RefreshJob::default(),
                wake: tokio::sync::Notify::new(),
            }),
        }
    }

    /// Matches every game that needs it and applies what was found.
    pub async fn run(&self) -> Result<RunReport> {
        let dumps = self.dumps().await;
        let games = self
            .inner
            .store
            .igdb_pending(dumps.as_deref(), retry_none_before())
            .await?;
        let mut report = RunReport::default();
        if !games.is_empty() {
            let mut progress = Progress::new(games.len());
            for (index, chunk) in games.chunks(CHUNK).enumerate() {
                self.run_games(chunk, dumps.as_deref(), &mut report, Some(&mut progress))
                    .await?;
                progress.reach((index + 1) * CHUNK);
                if report.unreachable {
                    // The rest wait for the next run, not for more failures.
                    report.deferred += games.len().saturating_sub((index + 1) * CHUNK);
                    break;
                }
            }
        }
        // Libraries from before merging are merged on their first run.
        // Merging needs only the library, so it goes ahead offline too.
        self.merge_same(dumps.as_deref(), &mut report).await?;
        if games.is_empty() && report.merged == 0 {
            return Ok(report);
        }
        tracing::info!(
            matched = report.matched,
            none = report.none,
            applied = report.applied,
            deferred = report.deferred,
            merged = report.merged,
            "IGDB matching done"
        );
        Ok(report)
    }

    /// Merges the games that are one game (`LibraryStore::same_games`:
    /// the same IGDB id, or title and era) into one, whose entries are then
    /// its releases on each platform, and applies the kept games'
    /// matches again; after a run that couldn't reach the server or an
    /// image host, the next run applies them (they stay pending). Callers
    /// must not hold the lock. The games merged away, each with the game it
    /// went into.
    async fn merge_same(
        &self,
        dumps: Option<&str>,
        report: &mut RunReport,
    ) -> Result<HashMap<String, String>> {
        let lock = self.inner.lock.lock().await;
        let store = &self.inner.store;
        let mut kept = Vec::new();
        let mut moved = HashMap::new();
        for group in store.same_games().await? {
            let Some((keep, donors)) = group.split_first() else {
                continue;
            };
            let mut merged = false;
            for donor in donors {
                match store.merge_games(keep, donor).await {
                    Ok(_) => {
                        tracing::debug!(keep, donor, "same game merged");
                        merged = true;
                        report.merged += 1;
                        moved.insert(donor.clone(), keep.clone());
                        self.inner.bus.emit(Event::LibraryGameRemoved {
                            game_id: donor.clone(),
                        });
                    }
                    // Both running: the next run tries again.
                    Err(error) => tracing::warn!(
                        %error,
                        keep,
                        donor,
                        "same game not merged; the next run tries again"
                    ),
                }
            }
            if merged {
                self.inner.bus.emit(Event::LibraryGameUpdated {
                    game_id: keep.clone(),
                });
                kept.push(keep.clone());
            }
        }
        drop(lock);
        if kept.is_empty() {
            return Ok(moved);
        }
        if report.unreachable {
            tracing::debug!(
                games = kept.len(),
                "merged games applied again on the next run: the server can't be reached"
            );
            report.deferred += kept.len();
            return Ok(moved);
        }
        self.run_games(&kept, dumps, report, None).await?;
        Ok(moved)
    }

    /// Matches `game_ids` that need it, then applies every pending match
    /// among them. Callers must not hold the lock.
    async fn run_games(
        &self,
        game_ids: &[String],
        dumps: Option<&str>,
        report: &mut RunReport,
        progress: Option<&mut Progress>,
    ) -> Result<()> {
        let store = &self.inner.store;
        let retry_before = retry_none_before();
        let mut rows: Vec<IgdbMatch> = Vec::with_capacity(game_ids.len());
        let mut unmatched = Vec::new();
        for game_id in game_ids {
            let row = store.igdb_match(game_id).await?;
            // A match, or the user's choice of none.
            if let Some(row) = &row
                && (row.igdb_id.is_some() || row.source == MatchSource::Manual)
            {
                rows.push(row.clone());
                continue;
            }
            let Some(game) = store.game(game_id).await? else {
                continue;
            };
            let entries = store.entries_for_game(game_id).await?;
            let local = self.find_local(&game, &entries).await?;
            // The entries' ids, then those of the game on other devices: a
            // game that came by sync has no entries here until it's
            // installed, and is matched by what its devices know it as (#75).
            let mut ids: Vec<ExternalId> = entries
                .iter()
                .flat_map(|entry| entry.ids.iter().cloned())
                .collect();
            // Nothing here knows the game, but the device it came from
            // matched it: a ROM matched by name or by RomM there has no
            // store id to ask about here (#75). Store ids of its own here
            // are asked about instead.
            if !matches!(local, find::Local::Found(..))
                && find::store_ids(&ids, &HashMap::new()).is_empty()
                && let Some(igdb_id) = store.synced_igdb_id(game_id).await?
            {
                rows.extend(
                    self.matched(game_id, Some(igdb_id), MatchSource::Device, dumps)
                        .await?,
                );
                report.matched += 1;
                continue;
            }
            ids.extend(store.synced_store_ids(game_id).await?);
            if matches!(local, find::Local::Unsure)
                && find::store_ids(&ids, &HashMap::new()).is_empty()
            {
                // Nothing else to ask, and the dumps couldn't answer: left
                // for the next run instead of recorded as none (#160).
                report.deferred += 1;
                report.unanswered.push(game_id.clone());
                continue;
            }
            match local {
                find::Local::Found(igdb_id, source) => {
                    rows.extend(self.matched(game_id, Some(igdb_id), source, dumps).await?);
                    report.matched += 1;
                }
                find::Local::NotFound | find::Local::Unsure => match row {
                    // Due only because the dumps changed: the server's
                    // answer for its store ids stands until the week is up.
                    Some(mut row)
                        if row.source == MatchSource::None && row.checked_at_ms >= retry_before =>
                    {
                        if let Some(dumps) = dumps {
                            dumps.clone_into(&mut row.dumps);
                        }
                        rows.extend(store.save_auto_igdb_match(&row).await?.then_some(row));
                    }
                    _ => {
                        if find::store_ids(&ids, &HashMap::new()).is_empty() {
                            // Nothing to ask the server about (a ROM RomM
                            // left without an id), so none, offline too.
                            report.none += 1;
                            rows.extend(
                                self.matched(game_id, None, MatchSource::None, dumps)
                                    .await?,
                            );
                        } else {
                            unmatched.push((game_id.clone(), ids));
                        }
                    }
                },
            }
        }
        let namespaces: BTreeSet<String> = unmatched
            .iter()
            .flat_map(|(_, ids)| find::epic_namespaces(ids))
            .collect();
        let offers = self.epic_offers(&namespaces).await;
        // A game is asked about with every id it has, else it waits for the
        // next run unless one of the others is found.
        let unmatched: Vec<_> = unmatched
            .into_iter()
            .map(|(game_id, ids)| {
                let complete = find::epic_namespaces(&ids)
                    .iter()
                    .all(|namespace| offers.contains_key(namespace));
                (game_id, find::store_ids(&ids, &offers), complete)
            })
            .collect();
        let asked: Vec<_> = unmatched
            .iter()
            .flat_map(|(_, ids, _)| ids.clone())
            .collect();
        let found = if asked.is_empty() {
            Some(HashMap::new())
        } else {
            match self.inner.igdb.match_stores(&asked).await {
                Ok(found) => Some(found),
                Err(error) => {
                    // Left as they were, so the next run asks again.
                    tracing::warn!(%error, "IGDB store matching failed");
                    report.deferred += unmatched.len();
                    report.unreachable = true;
                    None
                }
            }
        };
        if let Some(found) = found {
            for (game_id, ids, complete) in unmatched {
                let igdb_id = ids.iter().find_map(|id| found.get(id).copied());
                if igdb_id.is_none() && !complete {
                    report.deferred += 1;
                    report.unanswered.push(game_id);
                    continue;
                }
                let source = if igdb_id.is_some() {
                    report.matched += 1;
                    MatchSource::Store
                } else {
                    report.none += 1;
                    MatchSource::None
                };
                rows.extend(self.matched(&game_id, igdb_id, source, dumps).await?);
            }
        }
        rows.retain(|row| row.pending);
        self.apply_rows(rows, report, progress).await
    }

    /// The base-game offers of each Epic namespace Epic answered for.
    async fn epic_offers(&self, namespaces: &BTreeSet<String>) -> HashMap<String, Vec<String>> {
        #[cfg(feature = "net")]
        if !namespaces.is_empty() {
            return self.inner.epic.base_games(namespaces).await;
        }
        #[cfg(not(feature = "net"))]
        let _ = namespaces;
        HashMap::new()
    }

    /// Saves an automatic answer for `game_id`, keeping what earlier
    /// applies wrote so this one can replace it, and the dumps it was found
    /// against when they are unknown now. `None` when the user chose a
    /// match meanwhile.
    async fn matched(
        &self,
        game_id: &str,
        igdb_id: Option<u64>,
        source: MatchSource,
        dumps: Option<&str>,
    ) -> Result<Option<IgdbMatch>> {
        let store = &self.inner.store;
        let mut row = IgdbMatch::new(game_id, igdb_id, source, dumps.unwrap_or_default());
        if let Some(old) = store.igdb_match(game_id).await? {
            row.applied = old.applied;
            if dumps.is_none() {
                row.dumps = old.dumps;
            }
        }
        // Nothing to undo for a game nothing was ever applied to.
        row.pending = igdb_id.is_some() || row.applied != Applied::default();
        Ok(store.save_auto_igdb_match(&row).await?.then_some(row))
    }

    /// Fetches the IGDB games of `rows` and applies them. Rows stay pending
    /// when the server can't be reached or sends no game for their id.
    async fn apply_rows(
        &self,
        rows: Vec<IgdbMatch>,
        report: &mut RunReport,
        progress: Option<&mut Progress>,
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let ids: Vec<u64> = rows.iter().filter_map(|row| row.igdb_id).collect();
        // The games the cache has go in even when the server can't be
        // reached for the rest.
        // `outdated`: games the cache kept from before a change worth
        // fetching them again for (age ratings), whose fetch failed.
        let (lookup, failure, outdated) = if ids.is_empty() {
            Default::default()
        } else {
            self.inner.igdb.lookup(&ids).await?
        };
        if let Some(error) = failure {
            tracing::warn!(%error, "IGDB games not fetched");
            report.unreachable = true;
        }
        let games: HashMap<u64, IgdbGame> = lookup
            .games
            .into_iter()
            .map(|game| (game.id, game))
            .collect();
        // IGDB's answer is "no such game" only for these; for the rest the
        // server sent nothing usable, which must not undo the last apply.
        let not_found: HashSet<u64> = lookup.not_found.into_iter().collect();
        // Downloaded several games at a time, outside the lock, so the
        // applies below only write.
        let prefetched = self.prefetch(&rows, &games, progress).await?;
        if offline(&prefetched) {
            tracing::warn!("IGDB images not downloaded: their host can't be reached");
            report.unreachable = true;
        }
        for row in rows {
            let _lock = self.inner.lock.lock().await;
            // The game page may have changed it meanwhile.
            let Some(mut row) = self
                .inner
                .store
                .igdb_match(&row.game_id)
                .await?
                .filter(|now| {
                    now.pending && now.igdb_id == row.igdb_id && now.source == row.source
                })
            else {
                continue;
            };
            // An outdated answer waits too: applying it would clear the
            // row, and the fields it lacks would never come.
            if let Some(id) = row.igdb_id
                && ((!games.contains_key(&id) && !not_found.contains(&id))
                    || outdated.contains(&id))
            {
                report.deferred += 1;
                continue;
            }
            let game = row.igdb_id.and_then(|id| games.get(&id));
            if self.apply(&mut row, game, &prefetched).await? {
                report.applied += 1;
            } else {
                report.deferred += 1;
            }
        }
        Ok(())
    }

    /// The Hasheous dumps on disk, as one string that changes when they do;
    /// `None` when they can't be read.
    async fn dumps(&self) -> Option<String> {
        match self.inner.hasheous.dumps().await {
            Ok(mut dumps) => {
                dumps.sort_by(|a, b| a.name.cmp(&b.name));
                Some(
                    dumps
                        .iter()
                        .map(|dump| format!("{}:{}", dump.name, dump.md5))
                        .collect::<Vec<_>>()
                        .join(";"),
                )
            }
            Err(error) => {
                tracing::warn!(%error, "Hasheous dumps unavailable for IGDB matching");
                None
            }
        }
    }
}

/// Games without a match found before this are asked about again.
fn retry_none_before() -> i64 {
    now_ms().saturating_sub(RETRY_NONE.as_millis().try_into().unwrap_or(i64::MAX))
}
