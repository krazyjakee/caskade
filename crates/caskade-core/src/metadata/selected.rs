//! Refreshing chosen games' metadata and artwork (#176), through the same
//! matching (RomM ids, Hasheous names, store ids), IGDB cache and applies
//! as the background runs, so the cache's freshness, the server's cooldown
//! and `Retry-After`, the user's fields and artwork choices, and "not on
//! IGDB" all hold. Requests go in batches, never a request per game.
//!
//! Two modes: [`RefreshMode::Missing`] fills in what the games lack from
//! what the cache has (matching the unmatched), and
//! [`RefreshMode::Automatic`] also fetches matched games again, replacing
//! the automatic values beneath the user's. One run at a time; its status
//! is watched by Activity, and it can be cancelled between batches, which
//! keeps every game as it was or as it was refreshed.

use super::{IgdbMatch, MatchSource, Metadata, RunReport};
use crate::{Result, model::Game};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

/// Games a batch refreshes together.
const BATCH: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RefreshMode {
    /// Fill in what the games lack, from the cache; match the unmatched.
    Missing,
    /// Also fetch matched games again and replace their automatic values.
    Automatic,
}

/// How a refresh of chosen games is going, or went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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
    /// Games that couldn't be refreshed (the server or an image host
    /// couldn't be reached, or the server is cooling down): retry these.
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

/// What a game looks like to a refresh: its details and artwork.
type Snapshot = Option<(Game, Vec<String>)>;

impl Metadata {
    async fn snapshot(&self, game_id: &str) -> Result<Snapshot> {
        let store = &self.inner.store;
        let Some(game) = store.game(game_id).await? else {
            return Ok(None);
        };
        let mut artwork: Vec<String> = store
            .artwork_for_game(game_id)
            .await?
            .into_iter()
            .map(|art| art.content_hash)
            .collect();
        artwork.sort();
        Ok(Some((game, artwork)))
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
            let mut before = Vec::with_capacity(batch.len());
            let mut matched: Vec<IgdbMatch> = Vec::new();
            let mut unmatched = Vec::new();
            let mut looked = Vec::new();
            for id in batch {
                let snapshot = self.snapshot(id).await?;
                let row = store.igdb_match(id).await?;
                match (&snapshot, &row) {
                    (None, _) => status.skipped += 1,
                    // The user said it isn't on IGDB.
                    (Some(_), Some(row))
                        if row.source == MatchSource::Manual && row.igdb_id.is_none() =>
                    {
                        status.skipped += 1
                    }
                    (Some(_), Some(row)) if row.igdb_id.is_some() => {
                        matched.push(row.clone());
                        looked.push(id.clone());
                    }
                    (Some(_), _) => {
                        unmatched.push(id.clone());
                        looked.push(id.clone());
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
                let mut rows = Vec::with_capacity(matched.len());
                for mut row in matched {
                    row.pending = true;
                    store.save_igdb_match(&row).await?;
                    rows.push(row);
                }
                self.apply_rows(rows, &mut report, None).await?;
                self.run_games(&unmatched, dumps.as_deref(), &mut report, None)
                    .await?;
                failed_batch = report.unreachable;
            }
            unreachable |= failed_batch;
            for (id, earlier) in before {
                if !looked.contains(&id) {
                    continue;
                }
                let now = self.snapshot(&id).await?;
                let row = store.igdb_match(&id).await?;
                let pending = row.as_ref().is_some_and(|row| row.pending);
                if failed_batch && (pending || now == earlier) {
                    status.failed.push(id);
                } else if now != earlier {
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
