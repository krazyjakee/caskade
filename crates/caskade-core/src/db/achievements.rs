//! Releases' links to an achievement service (#189). They are synced, so
//! they stay with the library; achievement sets and their achievements are
//! in `caskade-achievements`.

use super::{LibraryStore, decode};
use crate::{
    Result,
    model::{AchievementLink, LinkMethod, validation as v},
};

impl LibraryStore {
    /// The links of these releases, to any service.
    pub async fn achievement_links(&self, entry_ids: &[String]) -> Result<Vec<AchievementLink>> {
        let mut links = vec![];
        for entry in entry_ids {
            links.extend(
                sqlx::query_scalar::<_, String>(
                    "SELECT record_json FROM achievement_links WHERE entry_id=? ORDER BY provider",
                )
                .bind(entry)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(decode::<AchievementLink>)
                .collect::<Result<Vec<_>>>()?,
            );
        }
        Ok(links)
    }

    /// Sets a release's link to a service, replacing the one it had.
    pub async fn save_achievement_link(&self, link: &AchievementLink) -> Result<()> {
        v::id(&link.entry_id)?;
        v::text(&link.provider, "provider", false, 128)?;
        let unresolved = matches!(link.method, LinkMethod::Unresolved { .. });
        v::text(&link.product_id, "product id", unresolved, 256)?;
        sqlx::query(
            "INSERT INTO achievement_links(entry_id,provider,record_json) VALUES(?,?,?) \
             ON CONFLICT(entry_id,provider) DO UPDATE SET record_json=excluded.record_json",
        )
        .bind(&link.entry_id)
        .bind(&link.provider)
        .bind(v::json(link)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_achievement_link(&self, entry_id: &str, provider: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM achievement_links WHERE entry_id=? AND provider=?")
                .bind(entry_id)
                .bind(provider)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
}
