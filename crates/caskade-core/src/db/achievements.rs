//! Achievements in the library (#185): each store account's sets, their
//! achievements, and releases' links to an achievement service.

use super::{LibraryStore, decode};
use crate::{
    Result,
    achievements::{
        Achievement, AchievementDefinition, AchievementLink, AchievementState, SetSync,
    },
    model::{new_id, validation as v},
};
use std::collections::{BTreeMap, BTreeSet};

/// What applying a fetch changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Achievements unlocked by this fetch that weren't before.
    pub newly_unlocked: Vec<String>,
    /// Anything about the set changed.
    pub changed: bool,
}

impl LibraryStore {
    /// The set of `store_account_id` for `product_id`: its id and sync.
    pub async fn achievement_set(
        &self,
        store_account_id: &str,
        product_id: &str,
    ) -> Result<Option<(String, SetSync)>> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT id,sync_json FROM achievement_sets WHERE store_account_id=? AND product_id=?",
        )
        .bind(store_account_id)
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|(id, sync)| Ok((id, decode(sync)?)))
        .transpose()
    }

    /// The set's id, made when it doesn't exist yet.
    pub async fn ensure_achievement_set(
        &self,
        store_account_id: &str,
        product_id: &str,
    ) -> Result<String> {
        v::id(store_account_id)?;
        v::text(product_id, "product id", false, 256)?;
        sqlx::query(
            "INSERT INTO achievement_sets(id,store_account_id,product_id,created_at_ms) \
             VALUES(?,?,?,?) ON CONFLICT(store_account_id,product_id) DO NOTHING",
        )
        .bind(new_id())
        .bind(store_account_id)
        .bind(product_id)
        .bind(crate::now_ms())
        .execute(&self.pool)
        .await?;
        Ok(sqlx::query_scalar(
            "SELECT id FROM achievement_sets WHERE store_account_id=? AND product_id=?",
        )
        .bind(store_account_id)
        .bind(product_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn save_achievement_sync(&self, set_id: &str, sync: &SetSync) -> Result<()> {
        sqlx::query("UPDATE achievement_sets SET sync_json=? WHERE id=?")
            .bind(v::json(sync)?)
            .bind(set_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A set's achievements in the service's order.
    pub async fn achievements(&self, set_id: &str) -> Result<Vec<Achievement>> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT definition_json,state_json FROM achievement_items WHERE set_id=? \
             ORDER BY position,achievement_id",
        )
        .bind(set_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(definition, state)| {
            Ok(Achievement {
                definition: decode(definition)?,
                state: decode(state)?,
            })
        })
        .collect()
    }

    /// Records a fetch, in one transaction. Definitions are replaced in the
    /// service's order. An unlock already known stays unlocked whatever the
    /// answer says. Only a `complete` answer drops achievements it doesn't
    /// list; an incomplete one changes only what it lists.
    pub async fn apply_achievements(
        &self,
        set_id: &str,
        definitions: &[AchievementDefinition],
        states: &BTreeMap<String, AchievementState>,
        complete: bool,
    ) -> Result<Applied> {
        let mut tx = self.begin_write().await?;
        let known: BTreeMap<String, (String, String, i64)> =
            sqlx::query_as::<_, (String, String, String, i64)>(
                "SELECT achievement_id,definition_json,state_json,position FROM achievement_items \
                 WHERE set_id=?",
            )
            .bind(set_id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|(id, definition, state, position)| (id, (definition, state, position)))
            .collect();
        let mut applied = Applied::default();
        let mut listed = BTreeSet::new();
        // A complete answer sets the order; an incomplete one keeps it and
        // puts what is new after.
        let mut next = known.values().map(|(_, _, at)| at + 1).max().unwrap_or(0);
        for (index, definition) in definitions.iter().enumerate() {
            if definition.id.is_empty() || !listed.insert(definition.id.clone()) {
                continue;
            }
            let old = known.get(&definition.id);
            let old_state: Option<AchievementState> = old
                .map(|(_, state, _)| serde_json::from_str(state))
                .transpose()?;
            let fresh = states.get(&definition.id).cloned().unwrap_or_default();
            let state = match &old_state {
                Some(old) => fresh.keeping(old),
                None => fresh,
            };
            if state.unlocked && !old_state.as_ref().is_some_and(|old| old.unlocked) {
                applied.newly_unlocked.push(definition.id.clone());
            }
            let position = match old {
                _ if complete => index as i64,
                Some((_, _, at)) => *at,
                None => {
                    next += 1;
                    next - 1
                }
            };
            let definition_json = v::json(definition)?;
            let state_json = v::json(&state)?;
            if old.is_some_and(|(d, s, at)| {
                *d == definition_json && *s == state_json && *at == position
            }) {
                continue;
            }
            applied.changed = true;
            sqlx::query(
                "INSERT INTO achievement_items(set_id,achievement_id,position,definition_json,state_json,unlocked_at_ms) \
                 VALUES(?,?,?,?,?,?) ON CONFLICT(set_id,achievement_id) DO UPDATE SET \
                 position=excluded.position,definition_json=excluded.definition_json,\
                 state_json=excluded.state_json,unlocked_at_ms=excluded.unlocked_at_ms",
            )
            .bind(set_id)
            .bind(&definition.id)
            .bind(position)
            .bind(definition_json)
            .bind(state_json)
            .bind(state.unlocked_at_ms)
            .execute(&mut *tx)
            .await?;
        }
        if complete {
            for id in known.keys().filter(|id| !listed.contains(*id)) {
                applied.changed = true;
                sqlx::query("DELETE FROM achievement_items WHERE set_id=? AND achievement_id=?")
                    .bind(set_id)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(applied)
    }

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
        let unresolved = matches!(
            link.method,
            crate::achievements::LinkMethod::Unresolved { .. }
        );
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

    /// Games played since `since_ms`, most recent first: what background
    /// achievement refreshes look at.
    pub async fn games_played_since(&self, since_ms: i64, limit: u32) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT game_id FROM play_sessions WHERE started_at_ms>=? GROUP BY game_id \
             ORDER BY max(started_at_ms) DESC LIMIT ?",
        )
        .bind(since_ms)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Games with releases: what an account's refresh links and fetches.
    pub async fn games_with_entries(&self) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT DISTINCT game_id FROM game_entries ORDER BY game_id")
                .fetch_all(&self.pool)
                .await?,
        )
    }
}
