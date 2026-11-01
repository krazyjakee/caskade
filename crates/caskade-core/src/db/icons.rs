//! Executable icons already looked for (`icon_checks`). The service lives in
//! `crate::icons`.

use super::LibraryStore;
use crate::Result;

impl LibraryStore {
    /// Games with no icon artwork at all, by title; an icon the user chose
    /// doesn't count, so one comes beneath it for Reset (#175).
    pub async fn games_without_icon(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT g.id FROM games g WHERE NOT EXISTS(SELECT 1 FROM artwork a \
             WHERE a.game_id=g.id AND a.kind='icon' AND a.record_json->>'$.source'<>'user') ORDER BY g.sort_title COLLATE NOCASE,g.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// What was looked at last time for `game_id`'s icon.
    pub async fn icon_check(&self, game_id: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT signature FROM icon_checks WHERE game_id=?")
                .bind(game_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn save_icon_check(&self, game_id: &str, signature: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO icon_checks(game_id,signature,checked_at_ms) \
             SELECT id,?,? FROM games WHERE id=? \
             ON CONFLICT(game_id) DO UPDATE SET signature=excluded.signature,\
             checked_at_ms=excluded.checked_at_ms",
        )
        .bind(signature)
        .bind(crate::now_ms())
        .bind(game_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
