//! Which game here another device's game is (#229): earlier matches, its
//! identity (IGDB id, store ids, title, platforms and year), and the games
//! sync makes or separates when nothing here matches.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Value, json};

use super::LibraryStore;
use super::game_edit::{Overrides, automatic};
use super::games::write_game;
use crate::Result;
use crate::model::Game;

/// Preloaded in-memory index of local games for `sync::engine::apply_pending`,
/// replacing O(N) full-table scans of `igdb_matches`, `game_entries`, and `games`
/// per incoming unmapped game with O(1) lookups.
type TitleCandidate = (String, Option<i64>, Vec<String>);

pub(crate) struct SyncGameIndex {
    igdb: HashMap<i64, String>,
    stores: HashMap<String, String>,
    titles: HashMap<String, Vec<TitleCandidate>>,
}

impl SyncGameIndex {
    pub(crate) fn find(&self, identity: &Value) -> Option<(String, &'static str)> {
        if let Some(igdb) = identity.get("igdb").and_then(Value::as_i64)
            && let Some(found) = self.igdb.get(&igdb)
        {
            return Some((found.clone(), "igdb"));
        }
        let wanted: HashSet<&str> = identity
            .get("stores")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !wanted.is_empty() {
            // Preserve `ORDER BY e.game_id` determinism by taking the smallest matching game_id.
            let mut best: Option<&str> = None;
            for key in &wanted {
                if let Some(game) = self.stores.get(*key)
                    && best.is_none_or(|b| game.as_str() < b)
                {
                    best = Some(game.as_str());
                }
            }
            if let Some(found) = best {
                return Some((found.to_owned(), "store"));
            }
        }
        let title = identity
            .get("title")
            .and_then(Value::as_str)
            .map(normal_title);
        let title = title.filter(|title| !title.is_empty())?;
        let candidates = self.titles.get(&title)?;
        let platforms: HashSet<&str> = identity
            .get("platforms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let year = identity.get("year").and_then(Value::as_i64);
        for (id, base_year, base_platforms) in candidates {
            if year.is_some() && base_year.is_some() && year != *base_year {
                continue;
            }
            let shares_platform = platforms.is_empty()
                || base_platforms.is_empty()
                || base_platforms
                    .iter()
                    .any(|p| platforms.contains(p.as_str()));
            if shares_platform {
                return Some((id.clone(), "title"));
            }
        }
        None
    }

    pub(crate) fn record_created(&mut self, id: String, identity: &Value) {
        let title = identity
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Untitled")
            .trim();
        let key = normal_title(title);
        if key.is_empty() {
            return;
        }
        let platforms: Vec<String> = identity
            .get("platforms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let year = identity.get("year").and_then(Value::as_i64);
        let list = self.titles.entry(key).or_default();
        let pos = list.partition_point(|(existing_id, _, _)| existing_id < &id);
        list.insert(pos, (id, year, platforms));
    }
}

impl LibraryStore {
    /// Builds a [`SyncGameIndex`] once for a batch of incoming remote games.
    pub(crate) async fn sync_game_index(&self) -> Result<SyncGameIndex> {
        let mut conn = self.pool.acquire().await?;
        let mut igdb = HashMap::new();
        for (id, game_id) in sqlx::query_as::<_, (i64, String)>(
            "SELECT igdb_id,game_id FROM igdb_matches \
             WHERE igdb_id IS NOT NULL AND source<>'none' ORDER BY game_id",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            igdb.entry(id).or_insert(game_id);
        }
        let mut stores = HashMap::new();
        for (game, id) in sqlx::query_as::<_, (String, String)>(
            "SELECT e.game_id,json(j.value) FROM game_entries e, json_each(e.record_json,'$.ids') j \
             WHERE e.source_id<>'manual' ORDER BY e.game_id",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            if let Some(key) = store_key(&id) {
                stores.entry(key).or_insert(game);
            }
        }
        let mut titles: HashMap<String, Vec<TitleCandidate>> = HashMap::new();
        for (id, record, overrides) in sqlx::query_as::<_, (String, String, String)>(
            "SELECT id,record_json,overrides_json FROM games ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            let (Ok(shown), Ok(overrides)) = (
                serde_json::from_str::<Game>(&record),
                serde_json::from_str::<Overrides>(&overrides),
            ) else {
                continue;
            };
            let (title, release_date, platforms) = if overrides.is_empty() {
                (shown.title, shown.release_date, shown.platforms)
            } else {
                let base = automatic(&shown, &overrides)?;
                (base.title, base.release_date, base.platforms)
            };
            let key = normal_title(&title);
            if key.is_empty() {
                continue;
            }
            let year = release_date
                .as_deref()
                .and_then(|date| date.get(..4))
                .and_then(|year| year.parse::<i64>().ok());
            titles.entry(key).or_default().push((id, year, platforms));
        }
        Ok(SyncGameIndex {
            igdb,
            stores,
            titles,
        })
    }
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
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT e.id, CASE WHEN e.source_id<>'manual' AND j.value IS NOT NULL THEN json(j.value) END \
             FROM game_entries e \
             LEFT JOIN json_each(e.record_json,'$.ids') j ON e.source_id<>'manual' \
             WHERE e.game_id=? \
             ORDER BY e.id",
        )
        .bind(game)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<(String, BTreeSet<String>)> = Vec::new();
        for (id, value) in rows {
            let keys = match out.last_mut() {
                Some((last_id, keys)) if last_id == &id => keys,
                _ => {
                    out.push((id, BTreeSet::new()));
                    &mut out.last_mut().expect("just pushed").1
                }
            };
            if let Some(key) = value.as_deref().and_then(store_key) {
                keys.insert(key);
            }
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
