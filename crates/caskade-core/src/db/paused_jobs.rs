//! Installs the user paused (`paused_jobs`, #131), kept as JSON for
//! `crate::install` to restore at start.

use super::LibraryStore;
use crate::Result;

impl LibraryStore {
    /// Every paused job's record, oldest pause first.
    pub async fn paused_jobs(&self) -> Result<Vec<(String, String)>> {
        Ok(sqlx::query_as::<_, (String, String)>(
            "SELECT id,record_json FROM paused_jobs ORDER BY paused_at_ms,id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn save_paused_job(&self, id: &str, record_json: &str, at_ms: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO paused_jobs(id,record_json,paused_at_ms) VALUES(?,?,?) \
             ON CONFLICT(id) DO UPDATE SET record_json=excluded.record_json,\
             paused_at_ms=excluded.paused_at_ms",
        )
        .bind(id)
        .bind(record_json)
        .bind(at_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_paused_job(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM paused_jobs WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
