//! Synced data as the change log sees it (#228–#230): a snapshot of every
//! synced field here, and the writes that apply a change.
//!
//! The snapshot is what `crate::sync::policy` declares synced, in a portable
//! form keyed by this device's ids. Capture diffs it against `sync_fields`,
//! so whatever wrote a value (an editor, a bulk edit, a merge), only the
//! user's values reach the log: automatic metadata isn't in the snapshot,
//! and a game's user values come from its overrides (#171), the receive
//! contract `policy::Receive::GameEdits` describes.
//!
//! Applying writes through the same layers: a game's user value goes on top
//! of its automatic one in `overrides_json`, and a reset brings the
//! automatic value back. Each apply is one transaction with its field's new
//! winner and the change marked applied, so a failure leaves nothing half
//! done.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value, json};
use sqlx::SqliteConnection;

use super::LibraryStore;
use super::game_edit::{Overrides, automatic, load_layers};
use super::games::{save_entry, write_game};
use super::sync_log::{FieldKey, FieldState, field, mark_applied, set_field};
use crate::Result;
use crate::model::{Game, GameEntry, GameField, SourceId};
use crate::sync::change::{Change, EXISTS, Entity, join_key, key_parts};

/// The source of entries holding other devices' play sessions for a game
/// that has no entry here (#229): uninstalled and unlaunchable.
pub const SESSIONS_SOURCE: &str = "sync:sessions";

/// The game fields whose values are lists.
const LIST_FIELDS: [GameField; 4] = [
    GameField::Aliases,
    GameField::Genres,
    GameField::Tags,
    GameField::Platforms,
];

/// The query fields of a smart collection that travel.
const QUERY_FIELDS: [&str; 6] = ["query", "filter", "platform", "sort", "offset", "limit"];

/// Every synced field here, and which games exist.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub values: HashMap<FieldKey, Value>,
    pub games: HashSet<String>,
}

impl Snapshot {
    fn put(&mut self, entity: Entity, key: String, field: impl Into<String>, value: Value) {
        self.values.insert((entity, key, field.into()), value);
    }
}

/// How an apply went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Written here; the change is the field's winner.
    Written,
    /// Something newer won, or the row was deleted: nothing to write.
    Superseded,
    /// Something it needs isn't here yet (its row, a game, an account).
    Wait,
}

fn is_list(field: GameField) -> bool {
    LIST_FIELDS.contains(&field)
}

/// A title as identity: lower case letters and digits, single-spaced.
pub fn normal_title(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A store id key (`store_accounts::key`) from its JSON; `None` for ids that
/// differ per machine (a non-Steam shortcut's id comes from local paths).
fn store_key(json: &str) -> Option<String> {
    let id: crate::model::ExternalId = serde_json::from_str(json).ok()?;
    if matches!(id, crate::model::ExternalId::SteamShortcut { .. }) {
        return None;
    }
    super::store_accounts::key(&id).ok().flatten()
}

fn record_map(game: &Game) -> Result<Map<String, Value>> {
    match serde_json::to_value(game)? {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

impl LibraryStore {
    /// Every synced field here, keyed by this device's ids.
    pub async fn sync_snapshot(&self) -> Result<Snapshot> {
        let mut snapshot = Snapshot::default();
        let mut conn = self.pool.acquire().await?;

        // Games: identity, favourite, hidden and the user's values.
        let mut stores: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (game, id) in sqlx::query_as::<_, (String, String)>(
            "SELECT e.game_id,json(j.value) FROM game_entries e, json_each(e.record_json,'$.ids') j \
             WHERE e.source_id<>'manual'",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            if let Some(key) = store_key(&id) {
                stores.entry(game).or_default().insert(key);
            }
        }
        let igdb: HashMap<String, i64> = sqlx::query_as(
            "SELECT game_id,igdb_id FROM igdb_matches WHERE igdb_id IS NOT NULL AND source<>'none'",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect();
        let mut same: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (game, origin, origin_id) in sqlx::query_as::<_, (String, String, String)>(
            "SELECT game_id,origin,origin_id FROM sync_game_ids",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            same.entry(game)
                .or_default()
                .insert(join_key(&[&origin, &origin_id]));
        }
        for (id, record, overrides) in sqlx::query_as::<_, (String, String, String)>(
            "SELECT id,record_json,overrides_json FROM games",
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
            snapshot.games.insert(id.clone());
            let base = automatic(&shown, &overrides)?;
            let year = base
                .release_date
                .as_deref()
                .and_then(|date| date.get(..4))
                .and_then(|year| year.parse::<i64>().ok());
            snapshot.put(
                Entity::Game,
                id.clone(),
                "identity",
                json!({
                    "title": base.title,
                    "platforms": base.platforms,
                    "year": year,
                    "igdb": igdb.get(&id),
                    "stores": stores.get(&id).cloned().unwrap_or_default(),
                    "same": same.get(&id).cloned().unwrap_or_default(),
                }),
            );
            if shown.favorite {
                snapshot.put(Entity::Game, id.clone(), "favorite", json!(true));
            }
            if shown.hidden {
                snapshot.put(Entity::Game, id.clone(), "hidden", json!(true));
            }
            let map = record_map(&shown)?;
            for field in GameField::ALL {
                let key = field.key();
                let value = map.get(key).cloned().unwrap_or(Value::Null);
                if field.automatic() {
                    if !overrides.contains_key(key) {
                        continue;
                    }
                    if is_list(field) {
                        snapshot.put(Entity::Game, id.clone(), key, json!(true));
                        for item in value.as_array().into_iter().flatten() {
                            if let Some(item) = item.as_str() {
                                snapshot.put(
                                    Entity::Game,
                                    id.clone(),
                                    format!("{key}/{item}"),
                                    json!(true),
                                );
                            }
                        }
                    } else {
                        snapshot.put(Entity::Game, id.clone(), key, json!({ "v": value }));
                    }
                } else if !(value.is_null() || value.as_str() == Some("")) {
                    // Rating and notes have no automatic layer.
                    snapshot.put(Entity::Game, id.clone(), key, json!({ "v": value }));
                }
            }
        }
        for game in sqlx::query_scalar::<_, String>("SELECT game_id FROM kept_apart")
            .fetch_all(&mut *conn)
            .await?
        {
            snapshot.put(Entity::KeptApart, game, EXISTS, json!(true));
        }
        for (game, igdb) in sqlx::query_as::<_, (String, Option<i64>)>(
            "SELECT game_id,igdb_id FROM igdb_matches WHERE source='manual'",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            let value = igdb.map_or(json!("none"), |id| json!(id));
            snapshot.put(Entity::IgdbMatch, game, "igdb", value);
        }
        for (game, kind) in
            sqlx::query_as::<_, (String, String)>("SELECT game_id,kind FROM artwork_choices")
                .fetch_all(&mut *conn)
                .await?
        {
            snapshot.put(
                Entity::ArtworkChoice,
                join_key(&[&game, &kind]),
                EXISTS,
                json!(true),
            );
        }

        // Collections and their members.
        for (id, name, kind, query, description, pinned, account) in sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                String,
                bool,
                Option<String>,
            ),
        >(
            "SELECT id,name,kind,query_json,description,pinned,account_id FROM collections",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            let query = query
                .and_then(|text| serde_json::from_str::<Map<String, Value>>(&text).ok())
                .map(|query| {
                    Value::Object(
                        query
                            .into_iter()
                            .filter(|(key, _)| QUERY_FIELDS.contains(&key.as_str()))
                            .collect(),
                    )
                });
            snapshot.put(Entity::Collection, id.clone(), EXISTS, json!(true));
            snapshot.put(Entity::Collection, id.clone(), "name", json!(name));
            snapshot.put(
                Entity::Collection,
                id.clone(),
                "description",
                json!(description),
            );
            snapshot.put(Entity::Collection, id.clone(), "pinned", json!(pinned));
            snapshot.put(
                Entity::Collection,
                id.clone(),
                "rule",
                json!({ "kind": kind, "query": query }),
            );
            if let Some(account) = account {
                snapshot.put(Entity::Collection, id, "account", json!(account));
            }
        }
        for (collection, game, position) in sqlx::query_as::<_, (String, String, i64)>(
            "SELECT collection_id,game_id,position FROM collection_members",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            let key = join_key(&[&collection, &game]);
            snapshot.put(Entity::Member, key.clone(), EXISTS, json!(true));
            snapshot.put(Entity::Member, key, "position", json!(position));
        }

        // Finished play sessions.
        for (id, game, started, ended, duration) in
            sqlx::query_as::<_, (String, String, i64, i64, i64)>(
                "SELECT id,game_id,started_at_ms,ended_at_ms,duration_ms FROM play_sessions \
             WHERE ended_at_ms IS NOT NULL",
            )
            .fetch_all(&mut *conn)
            .await?
        {
            snapshot.put(
                Entity::Session,
                id,
                EXISTS,
                json!({ "game": game, "started": started, "ended": ended, "duration": duration }),
            );
        }

        // Accounts, with their PIN hashes; never the throttle.
        for (id, name, avatar, role, pin, permissions, max_age, unrated, created) in sqlx::query_as::<
            _,
            (String, String, String, String, Option<String>, String, Option<i64>, bool, i64),
        >(
            "SELECT id,name,avatar,role,pin_hash,permissions_json,max_age,unrated_needs_pin,created_at_ms \
             FROM accounts",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            let permissions = serde_json::from_str::<Map<String, Value>>(&permissions)
                .unwrap_or_default()
                .into_iter()
                .filter(|(key, _)| key == "settings" || key == "install")
                .collect::<Map<_, _>>();
            snapshot.put(Entity::Account, id.clone(), EXISTS, json!(true));
            snapshot.put(Entity::Account, id.clone(), "name", json!(name));
            snapshot.put(Entity::Account, id.clone(), "avatar", json!(avatar));
            snapshot.put(Entity::Account, id.clone(), "role", json!(role));
            if let Some(pin) = pin {
                snapshot.put(Entity::Account, id.clone(), "pinHash", json!(pin));
            }
            snapshot.put(Entity::Account, id.clone(), "permissions", Value::Object(permissions));
            if let Some(max_age) = max_age {
                snapshot.put(Entity::Account, id.clone(), "maxAge", json!(max_age));
            }
            snapshot.put(Entity::Account, id.clone(), "unratedNeedsPin", json!(unrated));
            snapshot.put(Entity::Account, id, "createdAt", json!(created));
        }
        for (account, game) in sqlx::query_as::<_, (String, String)>(
            "SELECT account_id,game_id FROM account_game_approvals",
        )
        .fetch_all(&mut *conn)
        .await?
        {
            snapshot.put(
                Entity::Approval,
                join_key(&[&account, &game]),
                EXISTS,
                json!(true),
            );
        }

        // Save versions made here (#239).
        crate::saves::versions::sync_snapshot(&mut conn, &mut snapshot.values).await?;
        Ok(snapshot)
    }

    /// Marks a change applied without writing anything here.
    pub async fn sync_mark_applied(&self, change: &Change, target: Option<&str>) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        mark_applied(&mut conn, change, target).await
    }

    /// Applies `change` to the row `target` (its key in this device's ids),
    /// with `value` (its value in this device's ids), if it's newer than the
    /// field's winner here. One transaction.
    pub async fn sync_apply(
        &self,
        change: &Change,
        target: &str,
        value: Option<Value>,
    ) -> Result<Applied> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let entity = change.entity;
        let current = field(&mut tx, entity, target, &change.field).await?;
        if current.is_some_and(|current| current.stamp >= change.stamp()) {
            mark_applied(&mut tx, change, Some(target)).await?;
            tx.commit().await?;
            return Ok(Applied::Superseded);
        }
        if entity.has_rows() && change.field != EXISTS {
            match field(&mut tx, entity, target, EXISTS).await? {
                None => return Ok(Applied::Wait),
                Some(row) if row.value.is_none() => {
                    mark_applied(&mut tx, change, Some(target)).await?;
                    tx.commit().await?;
                    return Ok(Applied::Superseded);
                }
                Some(_) => {}
            }
        }
        let written = write(&mut tx, entity, target, &change.field, value.as_ref()).await?;
        if !written {
            return Ok(Applied::Wait);
        }
        set_field(
            &mut tx,
            (&entity, target, &change.field),
            &FieldState {
                value,
                stamp: change.stamp(),
            },
        )
        .await?;
        mark_applied(&mut tx, change, Some(target)).await?;
        tx.commit().await?;
        Ok(Applied::Written)
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
        let mut tx = self.pool.begin().await?;
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

/// Writes one field of one row here; `false` when something it needs is
/// missing.
async fn write(
    conn: &mut SqliteConnection,
    entity: Entity,
    target: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let parts = key_parts(target);
    match entity {
        Entity::Game => game_field(conn, target, field, value).await,
        Entity::KeptApart => {
            if !row_exists(conn, "games", target).await? {
                return Ok(false);
            }
            match value {
                Some(_) => sqlx::query("INSERT OR IGNORE INTO kept_apart(game_id) VALUES(?)"),
                None => sqlx::query("DELETE FROM kept_apart WHERE game_id=?"),
            }
            .bind(target)
            .execute(&mut *conn)
            .await?;
            Ok(true)
        }
        Entity::IgdbMatch => {
            if !row_exists(conn, "games", target).await? {
                return Ok(false);
            }
            match value {
                Some(value) => {
                    sqlx::query(
                        "INSERT INTO igdb_matches(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json) \
                         VALUES(?,?,'manual',1,?,'','{}') ON CONFLICT(game_id) DO UPDATE SET \
                         igdb_id=excluded.igdb_id,source='manual',pending=1,checked_at_ms=excluded.checked_at_ms",
                    )
                    .bind(target)
                    .bind(value.as_i64())
                    .bind(crate::sync::clock::now_ms())
                    .execute(&mut *conn)
                    .await?;
                }
                None => {
                    sqlx::query("DELETE FROM igdb_matches WHERE game_id=? AND source='manual'")
                        .bind(target)
                        .execute(&mut *conn)
                        .await?;
                }
            }
            Ok(true)
        }
        Entity::ArtworkChoice => {
            let [game, kind] = parts[..] else {
                return Ok(true);
            };
            if !row_exists(conn, "games", game).await? {
                return Ok(false);
            }
            match value {
                Some(_) => {
                    sqlx::query("INSERT OR IGNORE INTO artwork_choices(game_id,kind) VALUES(?,?)")
                }
                None => sqlx::query("DELETE FROM artwork_choices WHERE game_id=? AND kind=?"),
            }
            .bind(game)
            .bind(kind)
            .execute(&mut *conn)
            .await?;
            Ok(true)
        }
        Entity::Collection => collection(conn, target, field, value).await,
        Entity::Member => {
            let [collection, game] = parts[..] else {
                return Ok(true);
            };
            if !row_exists(conn, "collections", collection).await?
                || !row_exists(conn, "games", game).await?
            {
                return Ok(false);
            }
            match (field, value) {
                (EXISTS, Some(_)) => {
                    sqlx::query(
                        "INSERT OR IGNORE INTO collection_members(collection_id,game_id,position) VALUES(?,?,0)",
                    )
                    .bind(collection)
                    .bind(game)
                    .execute(&mut *conn)
                    .await?;
                }
                (EXISTS, None) => {
                    sqlx::query(
                        "DELETE FROM collection_members WHERE collection_id=? AND game_id=?",
                    )
                    .bind(collection)
                    .bind(game)
                    .execute(&mut *conn)
                    .await?;
                }
                ("position", Some(position)) => {
                    sqlx::query(
                        "UPDATE collection_members SET position=? WHERE collection_id=? AND game_id=?",
                    )
                    .bind(position.as_i64().unwrap_or(0).max(0))
                    .bind(collection)
                    .bind(game)
                    .execute(&mut *conn)
                    .await?;
                }
                _ => {}
            }
            Ok(true)
        }
        Entity::Session => session(conn, target, value).await,
        Entity::SaveVersion => crate::saves::versions::sync_apply(conn, target, value).await,
        // A setting: the engine applies it through `SettingsService`.
        Entity::RommServer => Ok(true),
        Entity::Account => account(conn, target, field, value).await,
        Entity::Approval => {
            let [account, game] = parts[..] else {
                return Ok(true);
            };
            if value.is_some()
                && (!row_exists(conn, "accounts", account).await?
                    || !row_exists(conn, "games", game).await?)
            {
                return Ok(false);
            }
            match value {
                Some(_) => sqlx::query(
                    "INSERT OR IGNORE INTO account_game_approvals(account_id,game_id) VALUES(?,?)",
                ),
                None => sqlx::query(
                    "DELETE FROM account_game_approvals WHERE account_id=? AND game_id=?",
                ),
            }
            .bind(account)
            .bind(game)
            .execute(&mut *conn)
            .await?;
            Ok(true)
        }
    }
}

async fn row_exists(conn: &mut SqliteConnection, table: &str, id: &str) -> Result<bool> {
    let sql = format!("SELECT count(*) FROM {table} WHERE id=?");
    Ok(sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(conn)
        .await?
        > 0)
}

/// A game's synced field, through its user-value layer.
async fn game_field(
    conn: &mut SqliteConnection,
    game: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    if field == "identity" {
        return Ok(true);
    }
    let Some((shown, mut overrides)) = load_layers(conn, game).await? else {
        return Ok(false);
    };
    let mut map = record_map(&shown)?;
    let base = record_map(&automatic(&shown, &overrides)?)?;
    let user = |value: Option<&Value>| value.and_then(|value| value.get("v")).cloned();
    match field {
        "favorite" | "hidden" => {
            map.insert(field.to_owned(), json!(value.is_some()));
        }
        _ => {
            let (name, item) = match field.split_once('/') {
                Some((name, item)) => (name, Some(item)),
                None => (field, None),
            };
            let Some(game_field) = GameField::ALL.into_iter().find(|f| f.key() == name) else {
                return Ok(true);
            };
            if !game_field.automatic() {
                // Rating and notes: the value, or cleared.
                let cleared = if name == GameField::Notes.key() {
                    json!("")
                } else {
                    Value::Null
                };
                map.insert(name.to_owned(), user(value).unwrap_or(cleared));
            } else if is_list(game_field) {
                let take_over = |map: &mut Map<String, Value>, overrides: &mut Overrides| {
                    if !overrides.contains_key(name) {
                        overrides.insert(
                            name.to_owned(),
                            base.get(name).cloned().unwrap_or(json!([])),
                        );
                        map.insert(name.to_owned(), json!([]));
                    }
                };
                match (item, value) {
                    // The list becomes the user's (its items follow).
                    (None, Some(_)) => take_over(&mut map, &mut overrides),
                    // Back to the automatic list.
                    (None, None) => {
                        if let Some(automatic) = overrides.remove(name) {
                            map.insert(name.to_owned(), automatic);
                        }
                    }
                    (Some(item), Some(_)) => {
                        take_over(&mut map, &mut overrides);
                        let list = map.entry(name.to_owned()).or_insert(json!([]));
                        if let Some(list) = list.as_array_mut()
                            && !list.iter().any(|v| v.as_str() == Some(item))
                        {
                            list.push(json!(item));
                            list.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                        }
                    }
                    (Some(item), None) => {
                        if overrides.contains_key(name)
                            && let Some(list) = map.get_mut(name).and_then(Value::as_array_mut)
                        {
                            list.retain(|v| v.as_str() != Some(item));
                        }
                    }
                }
            } else {
                match user(value) {
                    Some(user) => {
                        if !overrides.contains_key(name) {
                            overrides.insert(
                                name.to_owned(),
                                base.get(name).cloned().unwrap_or(Value::Null),
                            );
                        }
                        map.insert(name.to_owned(), user);
                    }
                    None => {
                        if let Some(automatic) = overrides.remove(name) {
                            map.insert(name.to_owned(), automatic);
                        }
                    }
                }
            }
        }
    }
    let next: Game = serde_json::from_value(Value::Object(map))?;
    if next.validate().is_err() {
        // A value this device can't hold (an empty title, say): keep ours.
        return Ok(true);
    }
    write_game(conn, &next, Some(&overrides)).await?;
    Ok(true)
}

async fn collection(
    conn: &mut SqliteConnection,
    id: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    match (field, value) {
        (EXISTS, Some(_)) => {
            sqlx::query(
                "INSERT OR IGNORE INTO collections(id,name,kind,query_json,updated_at_ms) VALUES(?,'','manual',NULL,?)",
            )
            .bind(id)
            .bind(crate::sync::clock::now_ms())
            .execute(&mut *conn)
            .await?;
        }
        (EXISTS, None) => {
            sqlx::query("DELETE FROM collections WHERE id=?")
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        ("name", value) | ("description", value) => {
            let column = if field == "name" {
                "name"
            } else {
                "description"
            };
            let text = value.and_then(Value::as_str).unwrap_or_default();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE collections SET {column}=?,updated_at_ms=? WHERE id=?"
            )))
            .bind(text)
            .bind(crate::sync::clock::now_ms())
            .bind(id)
            .execute(&mut *conn)
            .await?;
        }
        ("pinned", value) => {
            sqlx::query("UPDATE collections SET pinned=? WHERE id=?")
                .bind(value.and_then(Value::as_bool).unwrap_or(false))
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        ("rule", Some(rule)) => {
            let smart = rule.get("kind").and_then(Value::as_str) == Some("smart");
            let query = rule.get("query").filter(|query| query.is_object());
            let (kind, query) = match (smart, query) {
                (true, Some(query)) => ("smart", Some(query.to_string())),
                _ => ("manual", None),
            };
            sqlx::query("UPDATE collections SET kind=?,query_json=?,updated_at_ms=? WHERE id=?")
                .bind(kind)
                .bind(query)
                .bind(crate::sync::clock::now_ms())
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        ("account", Some(account)) => {
            let account = account.as_str().unwrap_or_default();
            if !row_exists(conn, "accounts", account).await? {
                return Ok(false);
            }
            sqlx::query("UPDATE collections SET account_id=? WHERE id=?")
                .bind(account)
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        _ => {}
    }
    Ok(true)
}

/// A finished play session from another device. Sessions are a union: one
/// is never removed by sync.
async fn session(conn: &mut SqliteConnection, id: &str, value: Option<&Value>) -> Result<bool> {
    let Some(value) = value else {
        return Ok(true);
    };
    let game = value
        .get("game")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !row_exists(conn, "games", game).await? {
        return Ok(false);
    }
    // The game's own entry when it has one, else one that holds sessions.
    let entry: Option<String> = sqlx::query_scalar(
        "SELECT id FROM game_entries WHERE game_id=? ORDER BY source_id=?, id LIMIT 1",
    )
    .bind(game)
    .bind(SESSIONS_SOURCE)
    .fetch_optional(&mut *conn)
    .await?;
    let entry = match entry {
        Some(entry) => entry,
        None => {
            let entry = GameEntry::new(game, SourceId::from(SESSIONS_SOURCE.to_owned()), game);
            save_entry(conn, &entry).await?;
            entry.id
        }
    };
    let number = |key: &str| value.get(key).and_then(Value::as_i64).unwrap_or(0);
    let ended = number("ended");
    sqlx::query(
        "INSERT OR IGNORE INTO play_sessions(id,game_id,entry_id,started_at_ms,ended_at_ms,duration_ms,heartbeat_at_ms) \
         VALUES(?,?,?,?,?,?,?)",
    )
    .bind(id)
    .bind(game)
    .bind(&entry)
    .bind(number("started"))
    .bind(ended)
    .bind(number("duration").max(0))
    .bind(ended)
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

async fn account(
    conn: &mut SqliteConnection,
    id: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let now = crate::sync::clock::now_ms();
    match (field, value) {
        (EXISTS, Some(_)) => {
            sqlx::query(
                "INSERT OR IGNORE INTO accounts(id,name,avatar,role,permissions_json,created_at_ms,updated_at_ms) \
                 VALUES(?,'','avatar:default','standard','{}',?,?)",
            )
            .bind(id)
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
        }
        (EXISTS, None) => {
            // Never the last admin: a library always keeps one.
            let admins: i64 =
                sqlx::query_scalar("SELECT count(*) FROM accounts WHERE role='admin' AND id<>?")
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            let role: Option<String> = sqlx::query_scalar("SELECT role FROM accounts WHERE id=?")
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?;
            if role.as_deref() != Some("admin") || admins > 0 {
                sqlx::query("DELETE FROM accounts WHERE id=?")
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
        (field, value) => {
            let (column, bound): (&str, Value) = match field {
                "name" => ("name", value.cloned().unwrap_or(json!(""))),
                "avatar" => ("avatar", value.cloned().unwrap_or(json!("avatar:default"))),
                "role" => {
                    let role = value.and_then(Value::as_str).unwrap_or("standard");
                    let role = if role == "admin" { "admin" } else { "standard" };
                    ("role", json!(role))
                }
                "pinHash" => ("pin_hash", value.cloned().unwrap_or(Value::Null)),
                "permissions" => (
                    "permissions_json",
                    json!(value.cloned().unwrap_or(json!({})).to_string()),
                ),
                "maxAge" => ("max_age", value.cloned().unwrap_or(Value::Null)),
                "unratedNeedsPin" => (
                    "unrated_needs_pin",
                    json!(value.and_then(Value::as_bool).unwrap_or(false)),
                ),
                "createdAt" => ("created_at_ms", value.cloned().unwrap_or(json!(now))),
                _ => return Ok(true),
            };
            let query = format!("UPDATE accounts SET {column}=?,updated_at_ms=? WHERE id=?");
            let query = sqlx::query(sqlx::AssertSqlSafe(query));
            let query = match bound {
                Value::Null => query.bind(None::<String>),
                Value::Bool(flag) => query.bind(flag),
                Value::Number(number) => query.bind(number.as_i64()),
                Value::String(text) => query.bind(text),
                other => query.bind(other.to_string()),
            };
            query.bind(now).bind(id).execute(&mut *conn).await?;
        }
    }
    Ok(true)
}
