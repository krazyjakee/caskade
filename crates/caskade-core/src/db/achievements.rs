//! Releases' links to an achievement service (#189). They are synced, so
//! they stay with the library; achievement sets and their achievements are
//! in `caskade-achievements`.

use super::{LibraryStore, decode};
use crate::{
    Result,
    model::{AchievementLink, LinkMethod, validation as v},
};
use sqlx::{QueryBuilder, Sqlite};
use std::collections::{HashMap, HashSet};

impl LibraryStore {
    /// The links of these releases, to any service.
    pub async fn achievement_links(&self, entry_ids: &[String]) -> Result<Vec<AchievementLink>> {
        if entry_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut unique: Vec<&str> = Vec::with_capacity(entry_ids.len());
        let mut seen = HashSet::with_capacity(entry_ids.len());
        for id in entry_ids {
            if seen.insert(id.as_str()) {
                unique.push(id.as_str());
            }
        }
        let mut by_entry: HashMap<String, Vec<AchievementLink>> = HashMap::new();
        for chunk in unique.chunks(500) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT entry_id, record_json FROM achievement_links WHERE entry_id IN (",
            );
            let mut list = query.separated(", ");
            for id in chunk {
                list.push_bind(*id);
            }
            query.push(") ORDER BY provider");
            let rows: Vec<(String, String)> = query.build_query_as().fetch_all(&self.pool).await?;
            for (entry_id, json) in rows {
                by_entry.entry(entry_id).or_default().push(decode(json)?);
            }
        }
        let mut links = Vec::new();
        for id in entry_ids {
            if let Some(found) = by_entry.get(id) {
                links.extend(found.iter().cloned());
            }
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
