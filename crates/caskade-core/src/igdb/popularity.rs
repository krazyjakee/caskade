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

/// How long a server without popularity (it doesn't know the endpoint, or
/// refuses the request's shape) is left alone, for popularity only. Short
/// enough that the sorts appear soon after the server gains them.
pub(crate) const UNSUPPORTED_PAUSE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// An id asked about to learn whether the server has popularity: any id
/// gets an answer from one that does.
const PROBE_ID: u64 = 1942;

/// Statuses a server answers when it doesn't offer popularity yet, rather
/// than being down or busy.
pub(crate) fn unsupported(status: Option<u16>) -> bool {
    matches!(status, Some(400 | 404 | 405 | 422 | 501))
}

impl Igdb {
    /// Refreshes due matched ids once across concurrent callers. Earlier
    /// batches survive a later failure, and failures keep every old score.
    ///
    /// A server that doesn't offer popularity yet (#253) pauses popularity
    /// alone, for [`UNSUPPORTED_PAUSE`]: game lookups and the Store go on,
    /// and sorting keeps the last scores (by title without any). Other
    /// failures follow the client's shared pause and Retry-After rules.
    pub(crate) async fn refresh_popularity(&self, store: &LibraryStore) -> Result<bool> {
        let _guard = self.inner.popularity.lock().await;
        let ids = store
            .popularity_due(now_ms().saturating_sub(TTL_MS))
            .await?;
        for batch in ids.chunks(BATCH) {
            self.check_pause()?;
            self.check_popularity_pause()?;
            let response = match super::fetch::popularity(&self.inner.base, batch).await {
                Err(failure) if unsupported(failure.status) => {
                    self.popularity_missing();
                    return Err(failure.error);
                }
                result => self.outcome(result)?,
            };
            self.popularity_answered();
            store.save_popularity(&response, now_ms()).await?;
        }
        Ok(!ids.is_empty())
    }

    fn popularity_missing(&self) {
        tracing::debug!("the metadata server has no popularity yet");
        self.inner
            .popularity_offered
            .store(false, std::sync::atomic::Ordering::Relaxed);
        *crate::lock(&self.inner.popularity_paused_until) =
            Some(std::time::Instant::now() + UNSUPPORTED_PAUSE);
    }

    fn popularity_answered(&self) {
        self.inner
            .popularity_offered
            .store(true, std::sync::atomic::Ordering::Relaxed);
        *crate::lock(&self.inner.popularity_paused_until) = None;
    }

    /// Whether the server has popularity, and so the catalog's sorts: an
    /// older one ignores a sort it doesn't know and answers in its usual
    /// order, which would pass for the sort asked. Learned from any
    /// popularity answer, else asked once with one id; while it has none,
    /// asked again after [`UNSUPPORTED_PAUSE`]. A server that can't be
    /// reached gets the benefit of the doubt: the catalog request fails
    /// on its own.
    pub(crate) async fn popularity_offered(&self) -> bool {
        if self
            .inner
            .popularity_offered
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return true;
        }
        if self.popularity_retry_in().is_some() {
            return false;
        }
        match super::fetch::popularity(&self.inner.base, &[PROBE_ID]).await {
            Ok(_) => {
                self.popularity_answered();
                true
            }
            Err(failure) if unsupported(failure.status) => {
                self.popularity_missing();
                false
            }
            Err(_) => true,
        }
    }

    /// How long popularity alone waits, after a server without it.
    pub(crate) fn popularity_retry_in(&self) -> Option<std::time::Duration> {
        crate::lock(&self.inner.popularity_paused_until)
            .and_then(|until| until.checked_duration_since(std::time::Instant::now()))
            .filter(|wait| !wait.is_zero())
    }

    fn check_popularity_pause(&self) -> Result<()> {
        match self.popularity_retry_in() {
            Some(wait) => Err(crate::Error::Io(std::io::Error::other(crate::tr!(
                "popularity isn't available from the metadata server yet; trying again in {seconds}s",
                seconds = wait.as_secs().max(1)
            )))),
            None => Ok(()),
        }
    }
}
