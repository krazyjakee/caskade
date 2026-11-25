//! Refreshing chosen games' metadata and artwork (#176), through the same
//! matching (RomM ids, Hasheous names, store ids and exact web titles),
//! IGDB cache and applies as the background runs, so the cache's freshness,
//! the server's cooldown
//! and `Retry-After`, the user's fields and artwork choices, and "not on
//! IGDB" all hold. Store-id requests go in batches; unmatched web games
//! without store ids can also search by exact title.
//!
//! Two modes: [`RefreshMode::Missing`] fills in what the games lack from
//! what the cache has (matching the unmatched), and
//! [`RefreshMode::Automatic`] also fetches matched games again, replacing
//! the automatic values beneath the user's. One run at a time; its status
//! is watched by Activity, and it can be cancelled between batches, which
//! keeps every game as it was or as it was refreshed.

use super::{IgdbMatch, MatchSource, Metadata, RunReport, fetch_games};
use crate::{Result, db::LibraryStore, model::Game};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio_util::sync::CancellationToken;

/// Games a batch refreshes together.
const BATCH: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshMode {
    /// Fill in what the games lack, from the cache; match the unmatched.
    Missing,
    /// Also fetch matched games again and replace their automatic values.
    Automatic,
}

/// How a refresh of chosen games is going, or went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshStatus {
    pub mode: RefreshMode,
    pub running: bool,
    pub total: u32,
    pub done: u32,
    /// Games whose details or artwork changed.
    pub updated: u32,
    /// Games refreshed with nothing to change.
    pub unchanged: u32,
    /// Games IGDB has no match for.
    pub unmatched: u32,
    /// Games set to "not on IGDB", or gone from the library.
    pub skipped: u32,
    /// Games that couldn't be refreshed (the server, an image host or
    /// Epic's catalogue couldn't be reached, the dumps couldn't be searched,
    /// or the server is cooling down): retry these.
    pub failed: Vec<String>,
    /// Cancelled: `remaining` weren't looked at.
    pub cancelled: bool,
    pub remaining: Vec<String>,
}

impl RefreshStatus {
    fn new(mode: RefreshMode, total: usize) -> Self {
        Self {
            mode,
            running: true,
            total: u32::try_from(total).unwrap_or(u32::MAX),
            done: 0,
            updated: 0,
            unchanged: 0,
            unmatched: 0,
            skipped: 0,
            failed: Vec::new(),
            cancelled: false,
            remaining: Vec::new(),
        }
    }
}

/// Batch-loads sorted artwork content hashes for `game_ids`.
async fn fetch_artwork_hashes(
    store: &LibraryStore,
    game_ids: &[&str],
) -> Result<HashMap<String, Vec<String>>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for chunk in game_ids.chunks(500) {
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT game_id, content_hash FROM artwork WHERE game_id IN (",
        );
        let mut list = qb.separated(", ");
        for id in chunk {
            list.push_bind(*id);
        }
        qb.push(")");
        let rows: Vec<(String, String)> = qb.build_query_as().fetch_all(&store.pool).await?;
        for (game_id, hash) in rows {
            out.entry(game_id).or_default().push(hash);
        }
    }
    for hashes in out.values_mut() {
        hashes.sort();
    }
    Ok(out)
}

impl Metadata {
    async fn snapshots(&self, game_ids: &[String]) -> Result<HashMap<String, (Game, Vec<String>)>> {
        if game_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let store = &self.inner.store;
        let ids: Vec<&str> = game_ids.iter().map(String::as_str).collect();
        let games = fetch_games(store, &ids).await?;
        let mut artwork = fetch_artwork_hashes(store, &ids).await?;
        let mut out = HashMap::with_capacity(games.len());
        for (id, game) in games {
            let hashes = artwork.remove(&id).unwrap_or_default();
            out.insert(id, (game, hashes));
        }
        Ok(out)
    }

    /// Refreshes `game_ids` in batches, calling `progress` after each, until
    /// done or `cancel`led. The final status.
    pub async fn refresh_games(
        &self,
        game_ids: &[String],
        mode: RefreshMode,
        cancel: &CancellationToken,
        mut progress: impl FnMut(&RefreshStatus),
    ) -> Result<RefreshStatus> {
        let store = &self.inner.store;
        let mut status = RefreshStatus::new(mode, game_ids.len());
        let activity = caskade_jobs::global().activity(crate::tr!("Refreshing game details"));
        let dumps = self.dumps().await;
        let mut unreachable = false;
        progress(&status);
        for (index, batch) in game_ids.chunks(BATCH).enumerate() {
            if cancel.is_cancelled() {
                status.cancelled = true;
                status.remaining = game_ids[index * BATCH..].to_vec();
                break;
            }
            if unreachable {
                // The rest would fail the same way: retry them later.
                status.failed.extend(batch.iter().cloned());
                status.done += batch.len() as u32;
                continue;
            }
            let before_snapshots = self.snapshots(batch).await?;
            let before_matches = store.igdb_matches(batch).await?;
            let mut before = Vec::with_capacity(batch.len());
            let mut matched: Vec<IgdbMatch> = Vec::new();
            let mut unmatched = Vec::new();
            let mut looked: HashSet<String> = HashSet::with_capacity(batch.len());
            for id in batch {
                let snapshot = before_snapshots.get(id).cloned();
                let row = before_matches.get(id);
                match (&snapshot, row) {
                    (None, _) => status.skipped += 1,
                    // The user said it isn't on IGDB.
                    (Some(_), Some(row))
                        if row.source == MatchSource::Manual && row.igdb_id.is_none() =>
                    {
                        status.skipped += 1
                    }
                    (Some(_), Some(row)) if row.igdb_id.is_some() => {
                        matched.push(row.clone());
                        looked.insert(id.clone());
                    }
                    (Some(_), _) => {
                        unmatched.push(id.clone());
                        looked.insert(id.clone());
                    }
                }
                before.push((id.clone(), snapshot));
            }
            let mut report = RunReport::default();
            let mut failed_batch = false;
            if mode == RefreshMode::Automatic && !matched.is_empty() {
                let ids: Vec<u64> = matched.iter().filter_map(|row| row.igdb_id).collect();
                if let Err(error) = self.inner.igdb.refresh(&ids).await {
                    tracing::warn!(%error, "IGDB games not fetched again");
                    failed_batch = true;
                }
            }
            if !failed_batch {
                // Matched games apply again, from what the cache has now.
                for row in &mut matched {
                    row.pending = true;
                }
                store.save_igdb_matches(&matched).await?;
                self.apply_rows(matched, &mut report, None).await?;
                self.run_games(&unmatched, dumps.as_deref(), &mut report, None)
                    .await?;
                failed_batch = report.unreachable;
            }
            unreachable |= failed_batch;
            let looked_ids: Vec<String> = looked.iter().cloned().collect();
            let after_snapshots = self.snapshots(&looked_ids).await?;
            let after_matches = store.igdb_matches(&looked_ids).await?;
            let unanswered: HashSet<&str> = report.unanswered.iter().map(String::as_str).collect();
            for (id, earlier) in before {
                if !looked.contains(&id) {
                    continue;
                }
                let now = after_snapshots.get(&id);
                let row = after_matches.get(&id);
                let pending = row.is_some_and(|row| row.pending);
                if (failed_batch && (pending || now == earlier.as_ref()))
                    || unanswered.contains(id.as_str())
                {
                    status.failed.push(id);
                } else if now != earlier.as_ref() {
                    status.updated += 1;
                    self.inner
                        .bus
                        .emit(crate::bus::Event::LibraryGameUpdated { game_id: id });
                } else if row.is_none_or(|row| row.igdb_id.is_none()) {
                    status.unmatched += 1;
                } else {
                    status.unchanged += 1;
                }
            }
            status.done += batch.len() as u32;
            activity.set_label(crate::tr!(
                "Refreshing game details ({done} of {total})",
                done = status.done,
                total = status.total
            ));
            progress(&status);
        }
        status.running = false;
        progress(&status);
        Ok(status)
    }
}

/// The refresh running now, if any, and its last status.
pub(super) struct RefreshJob {
    status: tokio::sync::watch::Sender<Option<RefreshStatus>>,
    cancel: std::sync::Mutex<Option<CancellationToken>>,
}

impl Default for RefreshJob {
    fn default() -> Self {
        Self {
            status: tokio::sync::watch::Sender::new(None),
            cancel: std::sync::Mutex::default(),
        }
    }
}

impl Metadata {
    /// The status of the last refresh of chosen games, as it changes.
    pub fn refresh_updates(&self) -> tokio::sync::watch::Receiver<Option<RefreshStatus>> {
        self.inner.refresh.status.subscribe()
    }

    /// Starts refreshing `game_ids` in the background; refused while
    /// another refresh runs.
    pub fn start_refresh(&self, game_ids: Vec<String>, mode: RefreshMode) -> Result<()> {
        let token = {
            let mut running = crate::lock(&self.inner.refresh.cancel);
            if running.is_some() {
                return Err(crate::Error::Invalid(crate::tr!(
                    "game details are already being refreshed"
                )));
            }
            let token = CancellationToken::new();
            *running = Some(token.clone());
            token
        };
        let this = self.clone();
        tokio::spawn(async move {
            let job = &this.inner.refresh;
            let outcome = this
                .refresh_games(&game_ids, mode, &token, |status| {
                    job.status.send_replace(Some(status.clone()));
                })
                .await;
            if let Err(error) = outcome {
                tracing::warn!(%error, "refreshing game details failed");
                // What wasn't counted can be tried again.
                job.status.send_modify(|status| {
                    if let Some(status) = status {
                        let counted = status.done as usize;
                        status.failed.extend(game_ids.iter().skip(counted).cloned());
                        status.running = false;
                    }
                });
            }
            *crate::lock(&job.cancel) = None;
        });
        Ok(())
    }

    /// Cancels the running refresh after its current batch; whether one ran.
    pub fn cancel_refresh(&self) -> bool {
        let running = crate::lock(&self.inner.refresh.cancel);
        running.as_ref().inspect(|token| token.cancel()).is_some()
    }
}
