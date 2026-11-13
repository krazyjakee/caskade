//! Synced data as the change log sees it (#228–#230): the snapshot of every
//! synced field here (`sync_snapshot`), and the writes that apply a change.
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

use serde_json::{Map, Value, json};
use sqlx::SqliteConnection;

use super::LibraryStore;
use super::game_edit::{Overrides, automatic, load_layers};
use super::games::{save_entry, write_game};
use super::sync_log::{FieldState, field, mark_applied, set_field};
use crate::Result;
use crate::model::{Game, GameEntry, GameField, SourceId};
use crate::sync::change::{Change, EXISTS, Entity, key_parts};

/// The source of entries holding other devices' play sessions for a game
/// that has no entry here (#229): uninstalled and unlaunchable.
pub(super) const SESSIONS_SOURCE: &str = "sync:sessions";

/// The game fields whose values are lists.
const LIST_FIELDS: [GameField; 4] = [
    GameField::Aliases,
    GameField::Genres,
    GameField::Tags,
    GameField::Platforms,
];

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

pub(super) fn is_list(field: GameField) -> bool {
    LIST_FIELDS.contains(&field)
}

pub(super) fn record_map(game: &Game) -> Result<Map<String, Value>> {
    match serde_json::to_value(game)? {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

impl LibraryStore {
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
                    .bind(crate::now_ms())
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
        Entity::Session => session(conn, target, field, value).await,
        Entity::SaveVersion => crate::saves::versions::sync_apply(conn, target, value).await,
        // Settings: the engine applies them through `SettingsService`.
        Entity::RommServer | Entity::Appearance | Entity::AccountSettings => Ok(true),
        Entity::Copy => super::sync_copies::write(conn, target, value).await,
        // Applied by the engine once its file is here (`sync_apply_artwork`).
        Entity::Artwork => Ok(false),
        Entity::GameScript
        | Entity::Action
        | Entity::ActionSettings
        | Entity::AchievementLink
        | Entity::Addon
        | Entity::PreferredEmulator => {
            super::sync_carried::write(conn, entity, target, field, value).await
        }
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
            .bind(crate::now_ms())
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
            .bind(crate::now_ms())
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
                .bind(crate::now_ms())
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

/// A finished play session from another device, or the device it was
/// played on (`device`). Sessions are a union: one is never removed by sync.
async fn session(
    conn: &mut SqliteConnection,
    id: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<bool> {
    let Some(value) = value else {
        return Ok(true);
    };
    if field == "device" {
        sqlx::query("UPDATE play_sessions SET device_id=? WHERE id=?")
            .bind(value.as_str())
            .bind(id)
            .execute(&mut *conn)
            .await?;
        return Ok(true);
    }
    if field != EXISTS {
        return Ok(true);
    }
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
    let now = crate::now_ms();
    match (field, value) {
        // Another device's untouched default admin: never a second account.
        (EXISTS, Some(value)) if super::sync_accounts::is_marker(value) => {
            super::sync_accounts::retire(conn, id).await?;
        }
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
