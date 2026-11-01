//! Daily IGDB Visits PopScore, stored by IGDB id in the library's cache
//! table. Refresh runs with metadata's background watch, never from a sort.
use super::{BATCH, Igdb};
use crate::{Result, db::LibraryStore, now_ms};
use serde::{Deserialize, Serialize};
#[cfg(feature = "net")]
use std::collections::HashSet;

pub(crate) const TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// `POST /popularity`: numeric IGDB ids in, scores and explicit missing ids
/// out. The type is discovered by the server; values are IGDB Visits shares.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PopularityScore {
    pub game_id: u64,
    pub popularity_type: u64,
    pub value: f64,
    pub calculated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PopularityResponse {
    pub scores: Vec<PopularityScore>,
    pub not_found: Vec<u64>,
}

impl PopularityResponse {
    /// An incomplete or malformed batch is a failure, never an instruction
    /// to clear the last good scores or to cache a missing answer for a day.
    #[cfg(feature = "net")]
    pub(super) fn valid(&self, ids: &[u64]) -> bool {
        let asked: HashSet<_> = ids.iter().copied().collect();
        let mut answered = HashSet::new();
        self.scores.iter().all(|s| {
            asked.contains(&s.game_id)
                && answered.insert(s.game_id)
                && s.value.is_finite()
                && s.value >= 0.0
                && s.popularity_type > 0
                && s.popularity_type <= i64::MAX as u64
                && s.calculated_at >= 0
        }) && self
            .not_found
            .iter()
            .all(|id| asked.contains(id) && answered.insert(*id))
            && asked == answered
    }
}

impl Igdb {
    /// Refreshes due matched ids once across concurrent callers. Earlier
    /// batches survive a later failure, and failures keep every old score.
    pub(crate) async fn refresh_popularity(&self, store: &LibraryStore) -> Result<bool> {
        let _guard = self.inner.popularity.lock().await;
        let ids = store
            .popularity_due(now_ms().saturating_sub(TTL_MS))
            .await?;
        for batch in ids.chunks(BATCH) {
            self.check_pause()?;
            let response = self.outcome(super::fetch::popularity(&self.inner.base, batch).await)?;
            store.save_popularity(&response, now_ms()).await?;
        }
        Ok(!ids.is_empty())
    }
}
