//! Which game here another device's game is (#229): earlier matches, its
//! identity (IGDB id, store ids, title, platforms and year), and the games
//! sync makes or separates when nothing here matches.

use std::collections::{BTreeSet, HashSet};

use serde_json::{Value, json};

use super::LibraryStore;
use super::game_edit::{Overrides, automatic};
use super::games::write_game;
use crate::Result;
use crate::model::Game;

impl LibraryStore {
    /// The local game another device's game is, by an earlier match.
    pub async fn sync_mapped_game(&self, origin: &str, origin_id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT m.game_id FROM sync_game_ids m JOIN games g ON g.id=m.game_id \
             WHERE m.origin=? AND m.origin_id=?",
        )
        .bind(origin)
        .bind(origin_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn set_sync_mapped_game(
        &self,
        origin: &str,
        origin_id: &str,
        game: &str,
        method: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sync_game_ids(origin,origin_id,game_id,method) VALUES(?,?,?,?) \
             ON CONFLICT(origin,origin_id) DO UPDATE SET game_id=excluded.game_id,method=excluded.method",
        )
        .bind(origin)
        .bind(origin_id)
        .bind(game)
        .bind(method)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Whether a game exists here.
    pub async fn sync_game_exists(&self, id: &str) -> Result<bool> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM games WHERE id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?
                > 0,
        )
    }

    /// The game here an identity names (#229): by IGDB id, then a store id
    /// its entries share, then title, platform and year. Hidden games count:
    /// hiding is a user value like any other. With the method that matched.
    pub async fn sync_find_game(&self, identity: &Value) -> Result<Option<(String, &'static str)>> {
        if let Some(igdb) = identity.get("igdb").and_then(Value::as_i64) {
            let found: Option<String> = sqlx::query_scalar(
                "SELECT game_id FROM igdb_matches WHERE igdb_id=? AND source<>'none' ORDER BY game_id LIMIT 1",
            )
            .bind(igdb)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(found) = found {
                return Ok(Some((found, "igdb")));
            }
        }
        let wanted: HashSet<&str> = identity
            .get("stores")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !wanted.is_empty() {
            let rows = sqlx::query_as::<_, (String, String)>(
                "SELECT e.game_id,json(j.value) FROM game_entries e, json_each(e.record_json,'$.ids') j \
                 WHERE e.source_id<>'manual' ORDER BY e.game_id",
            )
            .fetch_all(&self.pool)
            .await?;
            for (game, id) in rows {
                if store_key(&id).is_some_and(|key| wanted.contains(key.as_str())) {
                    return Ok(Some((game, "store")));
                }
            }
        }
        let title = identity
            .get("title")
            .and_then(Value::as_str)
            .map(normal_title);
        let Some(title) = title.filter(|title| !title.is_empty()) else {
            return Ok(None);
        };
        let platforms: HashSet<&str> = identity
            .get("platforms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let year = identity.get("year").and_then(Value::as_i64);
        for (id, record, overrides) in sqlx::query_as::<_, (String, String, String)>(
            "SELECT id,record_json,overrides_json FROM games ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?
        {
            let (Ok(shown), Ok(overrides)) = (
                serde_json::from_str::<Game>(&record),
                serde_json::from_str::<Overrides>(&overrides),
            ) else {
                continue;
            };
            let base = automatic(&shown, &overrides)?;
            if normal_title(&base.title) != title {
                continue;
            }
            let base_year = base
                .release_date
                .as_deref()
                .and_then(|date| date.get(..4))
                .and_then(|year| year.parse::<i64>().ok());
            if year.is_some() && base_year.is_some() && year != base_year {
                continue;
            }
            let shares_platform = platforms.is_empty()
                || base.platforms.is_empty()
                || base
                    .platforms
                    .iter()
                    .any(|p| platforms.contains(p.as_str()));
            if shares_platform {
                return Ok(Some((id, "title")));
            }
        }
        Ok(None)
    }

    /// One game's identity as sync sends it (#229), for another library to
    /// find it by ([`LibraryStore::sync_find_game`]): what shares send with
    /// a save or a collection (#248). `None` when there's no such game.
    pub async fn game_identity(&self, game: &str) -> Result<Option<Value>> {
        let Some((record, overrides)) = sqlx::query_as::<_, (String, String)>(
            "SELECT record_json,overrides_json FROM games WHERE id=?",
        )
        .bind(game)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let shown: Game = serde_json::from_str(&record)?;
        let overrides: Overrides = serde_json::from_str(&overrides)?;
        let base = automatic(&shown, &overrides)?;
        let year = base
            .release_date
            .as_deref()
            .and_then(|date| date.get(..4))
            .and_then(|year| year.parse::<i64>().ok());
        let igdb: Option<i64> = sqlx::query_scalar(
            "SELECT igdb_id FROM igdb_matches WHERE game_id=? AND igdb_id IS NOT NULL AND source<>'none'",
        )
        .bind(game)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        let stores: BTreeSet<String> = sqlx::query_scalar::<_, String>(
            "SELECT json(j.value) FROM game_entries e, json_each(e.record_json,'$.ids') j \
             WHERE e.game_id=? AND e.source_id<>'manual'",
        )
        .bind(game)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .filter_map(|id| store_key(id))
        .collect();
        Ok(Some(json!({
            "title": base.title,
            "platforms": base.platforms,
            "year": year,
            "igdb": igdb,
            "stores": stores,
        })))
    }

    /// A title-only game for another device's game that has none here: its
    /// title, platforms and release year; entries come when it's installed
    /// here (#232, #233).
    pub async fn sync_create_game(&self, identity: &Value) -> Result<String> {
        let title = identity
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Untitled");
        let mut game = Game::new(title.trim());
        game.platforms = identity
            .get("platforms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if let Some(year) = identity.get("year").and_then(Value::as_i64) {
            game.release_date = Some(format!("{year:04}-01-01"));
        }
        let mut tx = self.begin_write().await?;
        super::notifications::not_an_addition(&mut tx, &game.id).await?;
        write_game(&mut tx, &game, None).await?;
        tx.commit().await?;
        Ok(game.id)
    }

    /// The local game's entries with the store ids each has.
    pub async fn sync_game_entry_stores(
        &self,
        game: &str,
    ) -> Result<Vec<(String, BTreeSet<String>)>> {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM game_entries WHERE game_id=? ORDER BY id")
                .bind(game)
                .fetch_all(&self.pool)
                .await?;
        let mut out = Vec::new();
        for id in ids {
            let keys = sqlx::query_scalar::<_, String>(
                "SELECT json(j.value) FROM game_entries e, json_each(e.record_json,'$.ids') j \
                 WHERE e.id=? AND e.source_id<>'manual'",
            )
            .bind(&id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .filter_map(|id| store_key(id))
            .collect();
            out.push((id, keys));
        }
        Ok(out)
    }

    /// Moves `entries` out of their game into a new game kept apart, as
    /// Separate does: what another device's kept-apart game is here.
    pub async fn sync_separate(&self, entries: &[String]) -> Result<Option<String>> {
        let Some((first, rest)) = entries.split_first() else {
            return Ok(None);
        };
        // Both games keep the platforms the game had: which entry runs on
        // which isn't known here.
        let platforms: Vec<String> = sqlx::query_scalar::<_, String>(
            "SELECT g.record_json FROM games g JOIN game_entries e ON e.game_id=g.id WHERE e.id=?",
        )
        .bind(first)
        .fetch_optional(&self.pool)
        .await?
        .and_then(|record| serde_json::from_str::<Game>(&record).ok())
        .map(|game| game.platforms)
        .unwrap_or_default();
        let game = self.separate_entry(first, &platforms, &platforms).await?;
        for entry in rest {
            sqlx::query(
                "UPDATE game_entries SET game_id=?,record_json=json_set(record_json,'$.gameId',?) WHERE id=?",
            )
            .bind(&game.id)
            .bind(&game.id)
            .bind(entry)
            .execute(&self.pool)
            .await?;
        }
        Ok(Some(game.id))
    }
}

/// A title as identity: lower case letters and digits, single-spaced, so
/// "Bubsy II" and "BUBSY: II" are one title.
pub(super) fn normal_title(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A store id key (`store_accounts::key`) from its JSON; `None` for ids that
/// differ per machine (a non-Steam shortcut's id comes from local paths).
pub(super) fn store_key(json: &str) -> Option<String> {
    let id: crate::model::ExternalId = serde_json::from_str(json).ok()?;
    if matches!(id, crate::model::ExternalId::SteamShortcut { .. }) {
        return None;
    }
    super::store_accounts::key(&id).ok().flatten()
}
