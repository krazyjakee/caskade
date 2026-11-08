use super::LibraryStore;
use crate::{Result, igdb::popularity::PopularityResponse};

impl LibraryStore {
    /// Matched ids whose popularity has no answer younger than `since`.
    pub(crate) async fn popularity_due(&self, since: i64) -> Result<Vec<u64>> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT m.igdb_id FROM igdb_matches m LEFT JOIN igdb_popularity p \
             ON p.igdb_id=m.igdb_id WHERE m.igdb_id IS NOT NULL \
             AND (p.fetched_at_ms IS NULL OR p.fetched_at_ms<=?) ORDER BY m.igdb_id",
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().map(i64::cast_unsigned).collect())
    }

    pub(crate) async fn save_popularity(
        &self,
        response: &PopularityResponse,
        at: i64,
    ) -> Result<()> {
        let mut tx = self.begin_write().await?;
        for score in &response.scores {
            sqlx::query("INSERT INTO igdb_popularity(igdb_id,popularity_type,value,calculated_at,fetched_at_ms) VALUES(?,?,?,?,?) ON CONFLICT(igdb_id) DO UPDATE SET popularity_type=excluded.popularity_type,value=excluded.value,calculated_at=excluded.calculated_at,fetched_at_ms=excluded.fetched_at_ms")
                .bind(score.game_id as i64).bind(score.popularity_type as i64).bind(score.value)
                .bind(score.calculated_at).bind(at).execute(&mut *tx).await?;
        }
        for id in &response.not_found {
            sqlx::query("INSERT INTO igdb_popularity(igdb_id,fetched_at_ms) VALUES(?,?) ON CONFLICT(igdb_id) DO UPDATE SET popularity_type=NULL,value=NULL,calculated_at=NULL,fetched_at_ms=excluded.fetched_at_ms")
                .bind(*id as i64).bind(at).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
