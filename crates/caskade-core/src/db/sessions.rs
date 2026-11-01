//! Session persistence: the clock and process tracker live at the service edge.
use super::{LibraryStore, missing};
use crate::{
    Error, Result,
    model::{PlaySession, validation as v},
};
use sqlx::Row;

fn session(row: sqlx::sqlite::SqliteRow) -> Result<PlaySession> {
    let duration: i64 = row.try_get("duration_ms")?;
    let tracking: Option<String> = row.try_get("tracking_json")?;
    Ok(PlaySession {
        id: row.try_get("id")?,
        game_id: row.try_get("game_id")?,
        entry_id: row.try_get("entry_id")?,
        started_at_ms: row.try_get("started_at_ms")?,
        ended_at_ms: row.try_get("ended_at_ms")?,
        duration_ms: u64::try_from(duration)
            .map_err(|_| Error::Invalid(crate::tr!("negative session duration")))?,
        heartbeat_at_ms: row.try_get("heartbeat_at_ms")?,
        tracking: tracking.map(|s| serde_json::from_str(&s)).transpose()?,
    })
}
impl LibraryStore {
    pub async fn get_session(&self, id: &str) -> Result<Option<PlaySession>> {
        sqlx::query("SELECT * FROM play_sessions WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(session)
            .transpose()
    }
    pub async fn start_session(&self, record: &PlaySession) -> Result<PlaySession> {
        record.validate()?;
        if record.ended_at_ms.is_some() {
            return Err(Error::Invalid(
                "start_session requires an open session".into(),
            ));
        }
        insert(self, record).await?;
        Ok(record.clone())
    }
    /// Imports a finished historical session without overwriting an active one.
    pub async fn upsert_session(&self, record: &PlaySession) -> Result<PlaySession> {
        record.validate()?;
        if record.ended_at_ms.is_none() {
            return Err(Error::Invalid("use start_session for open sessions".into()));
        }
        let mut tx = self.pool.begin().await?;
        if let Some(open) =
            sqlx::query_scalar::<_, Option<i64>>("SELECT ended_at_ms FROM play_sessions WHERE id=?")
                .bind(&record.id)
                .fetch_optional(&mut *tx)
                .await?
            && open.is_none()
        {
            return Err(Error::Invalid(crate::tr!(
                "cannot replace an open session with an imported record"
            )));
        }
        sqlx::query("INSERT INTO play_sessions(id,game_id,entry_id,started_at_ms,ended_at_ms,duration_ms,heartbeat_at_ms,tracking_json) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET game_id=excluded.game_id,entry_id=excluded.entry_id,started_at_ms=excluded.started_at_ms,ended_at_ms=excluded.ended_at_ms,duration_ms=excluded.duration_ms,heartbeat_at_ms=excluded.heartbeat_at_ms,tracking_json=excluded.tracking_json")
            .bind(&record.id).bind(&record.game_id).bind(&record.entry_id).bind(record.started_at_ms).bind(record.ended_at_ms).bind(v::integer(record.duration_ms)?).bind(record.heartbeat_at_ms).bind(record.tracking.as_ref().map(v::json).transpose()?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(record.clone())
    }
    pub async fn activate_session(
        &self,
        id: &str,
        started_at_ms: i64,
        tracking: serde_json::Value,
    ) -> Result<PlaySession> {
        let value = v::json(&tracking)?;
        if value.len() > 1024 * 1024 {
            return Err(Error::Invalid(crate::tr!("tracking payload limit")));
        }
        let affected=sqlx::query("UPDATE play_sessions SET started_at_ms=?,heartbeat_at_ms=?,tracking_json=? WHERE id=? AND ended_at_ms IS NULL AND duration_ms=0").bind(started_at_ms).bind(started_at_ms).bind(value).bind(id).execute(&self.pool).await?.rows_affected();
        if affected == 0 {
            return Err(Error::Invalid(crate::tr!(
                "session cannot be activated after elapsed time or completion"
            )));
        }
        self.get_session(id)
            .await?
            .ok_or_else(|| missing("session", id))
    }
    pub async fn heartbeat_session(
        &self,
        id: &str,
        heartbeat_at_ms: i64,
        duration_ms: u64,
        tracking: Option<serde_json::Value>,
    ) -> Result<PlaySession> {
        let value = tracking.as_ref().map(v::json).transpose()?;
        if value.as_ref().is_some_and(|s| s.len() > 1024 * 1024) {
            return Err(Error::Invalid(crate::tr!("tracking payload limit")));
        }
        let affected=sqlx::query("UPDATE play_sessions SET heartbeat_at_ms=?,duration_ms=max(duration_ms,?),tracking_json=coalesce(?,tracking_json) WHERE id=? AND ended_at_ms IS NULL").bind(heartbeat_at_ms).bind(v::integer(duration_ms)?).bind(value).bind(id).execute(&self.pool).await?.rows_affected();
        if affected == 0 {
            return Err(Error::Invalid(crate::tr!("session is not open")));
        }
        self.get_session(id)
            .await?
            .ok_or_else(|| missing("session", id))
    }
    /// Idempotent: a duplicate exit event never changes an already finished row.
    pub async fn finish_session(
        &self,
        id: &str,
        ended_at_ms: i64,
        duration_ms: u64,
    ) -> Result<PlaySession> {
        sqlx::query("UPDATE play_sessions SET ended_at_ms=?,heartbeat_at_ms=?,duration_ms=max(duration_ms,?) WHERE id=? AND ended_at_ms IS NULL").bind(ended_at_ms).bind(ended_at_ms).bind(v::integer(duration_ms)?).bind(id).execute(&self.pool).await?;
        self.get_session(id)
            .await?
            .ok_or_else(|| missing("session", id))
    }
    pub async fn in_flight_sessions(&self) -> Result<Vec<PlaySession>> {
        sqlx::query(
            "SELECT * FROM play_sessions WHERE ended_at_ms IS NULL ORDER BY started_at_ms,id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(session)
        .collect()
    }
    pub async fn sessions_for_game(&self, game_id: &str, limit: u32) -> Result<Vec<PlaySession>> {
        sqlx::query(
            "SELECT * FROM play_sessions WHERE game_id=? ORDER BY started_at_ms DESC,id LIMIT ?",
        )
        .bind(game_id)
        .bind(limit.min(1000))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(session)
        .collect()
    }
    pub async fn recent_sessions(&self, limit: u32) -> Result<Vec<PlaySession>> {
        sqlx::query("SELECT * FROM play_sessions ORDER BY started_at_ms DESC,id LIMIT ?")
            .bind(limit.min(1000))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(session)
            .collect()
    }
    pub async fn delete_session(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM play_sessions WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }
}
async fn insert(store: &LibraryStore, record: &PlaySession) -> Result<()> {
    sqlx::query("INSERT INTO play_sessions(id,game_id,entry_id,started_at_ms,ended_at_ms,duration_ms,heartbeat_at_ms,tracking_json) VALUES(?,?,?,?,?,?,?,?)")
        .bind(&record.id).bind(&record.game_id).bind(&record.entry_id).bind(record.started_at_ms).bind(record.ended_at_ms).bind(v::integer(record.duration_ms)?).bind(record.heartbeat_at_ms).bind(record.tracking.as_ref().map(v::json).transpose()?).execute(&store.pool).await?;
    Ok(())
}
