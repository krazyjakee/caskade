//! The sync change log's storage (#228): migration 0032.
//!
//! `sync_changes` holds every change this device made or received, until
//! compaction drops what every paired device has seen and something newer
//! replaced. `sync_fields` holds the winning value of each field here, in
//! this device's ids, with the stamp of the change that set it.
//! `sync_seen` is the version vector; `sync_peers` what each paired device
//! said it has. See `crate::sync`.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;
use sqlx::SqliteConnection;

use super::LibraryStore;
use crate::Result;
use crate::sync::change::{Change, Entity};
use crate::sync::clock::{Hlc, Stamp};

/// A version vector: the highest clock known from each device.
pub type Seen = BTreeMap<String, Hlc>;

/// A field's winning value here.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldState {
    pub value: Option<Value>,
    pub stamp: Stamp,
}

/// `(entity, key here, field)`.
pub type FieldKey = (Entity, String, String);

type ChangeRow = (String, i64, i64, String, String, String, Option<String>);

fn change(row: ChangeRow) -> Option<Change> {
    Some(Change {
        origin: row.0,
        clock: Hlc::new(row.1, row.2),
        entity: Entity::parse(&row.3)?,
        key: row.4,
        field: row.5,
        value: row.6.and_then(|text| serde_json::from_str(&text).ok()),
    })
}

fn text(value: &Option<Value>) -> Option<String> {
    value.as_ref().map(Value::to_string)
}

/// Raises `origin`'s entry in the version vector to `clock`.
pub(super) async fn see(conn: &mut SqliteConnection, origin: &str, clock: Hlc) -> Result<()> {
    sqlx::query(
        "INSERT INTO sync_seen(origin,wall,counter) VALUES(?,?,?) ON CONFLICT(origin) DO UPDATE \
         SET wall=excluded.wall,counter=excluded.counter \
         WHERE (excluded.wall,excluded.counter) > (sync_seen.wall,sync_seen.counter)",
    )
    .bind(origin)
    .bind(clock.wall)
    .bind(clock.counter)
    .execute(conn)
    .await?;
    Ok(())
}

/// Sets a field's winning value.
pub(super) async fn set_field(
    conn: &mut SqliteConnection,
    (entity, key, field): (&Entity, &str, &str),
    state: &FieldState,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sync_fields(entity,key,field,value_json,origin,wall,counter) VALUES(?,?,?,?,?,?,?) \
         ON CONFLICT(entity,key,field) DO UPDATE SET value_json=excluded.value_json,\
         origin=excluded.origin,wall=excluded.wall,counter=excluded.counter",
    )
    .bind(entity.as_str())
    .bind(key)
    .bind(field)
    .bind(text(&state.value))
    .bind(&state.stamp.origin)
    .bind(state.stamp.clock.wall)
    .bind(state.stamp.clock.counter)
    .execute(conn)
    .await?;
    Ok(())
}

/// A field's winning value, read inside a transaction.
pub(super) async fn field(
    conn: &mut SqliteConnection,
    entity: Entity,
    key: &str,
    field: &str,
) -> Result<Option<FieldState>> {
    let row = sqlx::query_as::<_, (Option<String>, String, i64, i64)>(
        "SELECT value_json,origin,wall,counter FROM sync_fields WHERE entity=? AND key=? AND field=?",
    )
    .bind(entity.as_str())
    .bind(key)
    .bind(field)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|(value, origin, wall, counter)| FieldState {
        value: value.and_then(|text| serde_json::from_str(&text).ok()),
        stamp: Stamp {
            clock: Hlc::new(wall, counter),
            origin,
        },
    }))
}

/// Marks a change applied (or superseded) at `target`.
pub(super) async fn mark_applied(
    conn: &mut SqliteConnection,
    change: &Change,
    target: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE sync_changes SET applied=1,target=? WHERE origin=? AND wall=? AND counter=?",
    )
    .bind(target)
    .bind(&change.origin)
    .bind(change.clock.wall)
    .bind(change.clock.counter)
    .execute(conn)
    .await?;
    Ok(())
}

impl LibraryStore {
    /// A field's winning value here.
    pub async fn sync_field(
        &self,
        entity: Entity,
        key: &str,
        name: &str,
    ) -> Result<Option<FieldState>> {
        let mut conn = self.pool.acquire().await?;
        field(&mut conn, entity, key, name).await
    }

    /// Records a change applied outside the library's tables (a setting), as
    /// its field's winner.
    pub async fn sync_record_applied(
        &self,
        change: &Change,
        target: &str,
        value: Option<Value>,
    ) -> Result<()> {
        let mut tx = self.begin_write().await?;
        set_field(
            &mut tx,
            (&change.entity, target, &change.field),
            &FieldState {
                value,
                stamp: change.stamp(),
            },
        )
        .await?;
        mark_applied(&mut tx, change, Some(target)).await?;
        tx.commit().await?;
        Ok(())
    }

    /// This device's version vector.
    pub async fn sync_seen(&self) -> Result<Seen> {
        let rows =
            sqlx::query_as::<_, (String, i64, i64)>("SELECT origin,wall,counter FROM sync_seen")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(origin, wall, counter)| (origin, Hlc::new(wall, counter)))
            .collect())
    }

    /// Every field's winning value here.
    pub async fn sync_fields(&self) -> Result<HashMap<FieldKey, FieldState>> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, String, i64, i64)>(
            "SELECT entity,key,field,value_json,origin,wall,counter FROM sync_fields",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(entity, key, field, value, origin, wall, counter)| {
                Some((
                    (Entity::parse(&entity)?, key, field),
                    FieldState {
                        value: value.and_then(|text| serde_json::from_str(&text).ok()),
                        stamp: Stamp {
                            clock: Hlc::new(wall, counter),
                            origin,
                        },
                    },
                ))
            })
            .collect())
    }

    /// Records changes this device made, already the winners here, in one
    /// transaction with the fields and the version vector.
    pub async fn sync_record_local(&self, changes: &[Change]) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin_write().await?;
        for change in changes {
            sqlx::query(
                "INSERT INTO sync_changes(origin,wall,counter,entity,key,field,value_json,target,applied) \
                 VALUES(?,?,?,?,?,?,?,?,1)",
            )
            .bind(&change.origin)
            .bind(change.clock.wall)
            .bind(change.clock.counter)
            .bind(change.entity.as_str())
            .bind(&change.key)
            .bind(&change.field)
            .bind(text(&change.value))
            .bind(&change.key)
            .execute(&mut *tx)
            .await?;
            set_field(
                &mut tx,
                (&change.entity, &change.key, &change.field),
                &FieldState {
                    value: change.value.clone(),
                    stamp: change.stamp(),
                },
            )
            .await?;
            see(&mut tx, &change.origin, change.clock).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Takes in changes from another device, unapplied. Ones already here
    /// are skipped. Returns how many were new.
    pub async fn sync_import(&self, changes: &[Change]) -> Result<usize> {
        let mut tx = self.begin_write().await?;
        let mut added = 0;
        for change in changes {
            added += sqlx::query(
                "INSERT OR IGNORE INTO sync_changes(origin,wall,counter,entity,key,field,value_json) \
                 VALUES(?,?,?,?,?,?,?)",
            )
            .bind(&change.origin)
            .bind(change.clock.wall)
            .bind(change.clock.counter)
            .bind(change.entity.as_str())
            .bind(&change.key)
            .bind(&change.field)
            .bind(text(&change.value))
            .execute(&mut *tx)
            .await?
            .rows_affected() as usize;
            see(&mut tx, &change.origin, change.clock).await?;
        }
        tx.commit().await?;
        Ok(added)
    }

    /// Changes waiting to apply, oldest first.
    pub async fn sync_pending(&self, limit: i64) -> Result<Vec<Change>> {
        let rows = sqlx::query_as::<_, ChangeRow>(
            "SELECT origin,wall,counter,entity,key,field,value_json FROM sync_changes \
             WHERE applied=0 ORDER BY wall,counter,origin LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(change).collect())
    }

    /// Up to `limit` changes a device that has `cursor` hasn't seen, in
    /// order per device, leaving out `skip`'s entities.
    pub async fn sync_changes_after(
        &self,
        cursor: &Seen,
        skip: &HashSet<Entity>,
        limit: usize,
    ) -> Result<Vec<Change>> {
        let mut out = Vec::new();
        for origin in self.sync_seen().await?.keys() {
            if out.len() >= limit {
                break;
            }
            let after = cursor
                .get(origin)
                .copied()
                .unwrap_or(Hlc::new(i64::MIN, i64::MIN));
            let rows = sqlx::query_as::<_, ChangeRow>(
                "SELECT origin,wall,counter,entity,key,field,value_json FROM sync_changes \
                 WHERE origin=? AND (wall,counter) > (?,?) ORDER BY wall,counter LIMIT ?",
            )
            .bind(origin)
            .bind(after.wall)
            .bind(after.counter)
            .bind(i64::try_from(limit - out.len()).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await?;
            out.extend(
                rows.into_iter()
                    .filter_map(change)
                    .filter(|change| !skip.contains(&change.entity)),
            );
        }
        Ok(out)
    }

    /// Up to `limit` changes to `entities`, after `after` in (origin, clock)
    /// order: a category sent again in full.
    pub async fn sync_changes_of(
        &self,
        entities: &[Entity],
        after: Option<(&str, Hlc)>,
        limit: usize,
    ) -> Result<Vec<Change>> {
        let names: Vec<&str> = entities.iter().map(|entity| entity.as_str()).collect();
        let (origin, clock) = after.unwrap_or(("", Hlc::new(i64::MIN, i64::MIN)));
        let rows = sqlx::query_as::<_, ChangeRow>(
            "SELECT origin,wall,counter,entity,key,field,value_json FROM sync_changes \
             WHERE entity IN (SELECT value FROM json_each(?)) AND (origin,wall,counter) > (?,?,?) \
             ORDER BY origin,wall,counter LIMIT ?",
        )
        .bind(serde_json::to_string(&names)?)
        .bind(origin)
        .bind(clock.wall)
        .bind(clock.counter)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(change).collect())
    }

    /// The latest identity a device sent for one of its games.
    pub async fn sync_identity(&self, origin: &str, game: &str) -> Result<Option<Value>> {
        let value = sqlx::query_scalar::<_, Option<String>>(
            "SELECT value_json FROM sync_changes WHERE origin=? AND entity='game' AND key=? \
             AND field='identity' ORDER BY wall DESC,counter DESC LIMIT 1",
        )
        .bind(origin)
        .bind(game)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        Ok(value.and_then(|text| serde_json::from_str(&text).ok()))
    }

    /// What SQLite's integrity and foreign key checks report: empty when the
    /// library is sound.
    pub async fn sync_integrity(&self) -> Result<Vec<String>> {
        let mut problems: Vec<String> = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter(|line| line != "ok")
            .collect();
        let foreign: Vec<(String, Option<i64>, String, i64)> =
            sqlx::query_as("PRAGMA foreign_key_check")
                .fetch_all(&self.pool)
                .await?;
        problems.extend(foreign.into_iter().map(|(table, row, parent, _)| {
            format!("{table} row {row:?} refers to a missing {parent}")
        }));
        Ok(problems)
    }

    /// How many changes the log holds.
    pub async fn sync_log_len(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM sync_changes")
            .fetch_one(&self.pool)
            .await?)
    }

    /// What `device` said it has.
    pub async fn sync_peer_seen(&self, device: &str) -> Result<Seen> {
        let seen =
            sqlx::query_scalar::<_, String>("SELECT seen_json FROM sync_peers WHERE device=?")
                .bind(device)
                .fetch_optional(&self.pool)
                .await?;
        Ok(seen
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default())
    }

    pub async fn set_sync_peer_seen(&self, device: &str, seen: &Seen) -> Result<()> {
        sqlx::query(
            "INSERT INTO sync_peers(device,seen_json) VALUES(?,?) \
             ON CONFLICT(device) DO UPDATE SET seen_json=excluded.seen_json",
        )
        .bind(device)
        .bind(serde_json::to_string(seen)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Categories (by name) to send `device` again in full.
    pub async fn sync_peer_replay(&self, device: &str) -> Result<Vec<String>> {
        let replay =
            sqlx::query_scalar::<_, String>("SELECT replay_json FROM sync_peers WHERE device=?")
                .bind(device)
                .fetch_optional(&self.pool)
                .await?;
        Ok(replay
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default())
    }

    pub async fn set_sync_peer_replay(&self, device: &str, replay: &[String]) -> Result<()> {
        sqlx::query(
            "INSERT INTO sync_peers(device,replay_json) VALUES(?,?) \
             ON CONFLICT(device) DO UPDATE SET replay_json=excluded.replay_json",
        )
        .bind(device)
        .bind(serde_json::to_string(replay)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forgets what a device has seen (it was forgotten).
    pub async fn delete_sync_peer(&self, device: &str) -> Result<()> {
        sqlx::query("DELETE FROM sync_peers WHERE device=?")
            .bind(device)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Drops changes every one of `peers` has seen that are no longer needed:
    /// values something newer replaced, and tombstones, once the replacing or
    /// deleting change has been seen by all. What remains is what some peer
    /// hasn't seen and the live values. Returns how many went.
    pub async fn sync_compact(&self, peers: &[Seen]) -> Result<u64> {
        let seen_by_all = |stamp: &Stamp| {
            peers.iter().all(|seen| {
                seen.get(&stamp.origin)
                    .is_some_and(|clock| *clock >= stamp.clock)
            })
        };
        let winners = self.sync_fields().await?;
        let rows = sqlx::query_as::<_, (String, i64, i64, String, String, Option<String>, String, Option<String>)>(
            "SELECT origin,wall,counter,entity,key,target,field,value_json FROM sync_changes WHERE applied=1",
        )
        .fetch_all(&self.pool)
        .await?;
        // The newest identity each device sent per game stays.
        let mut identities: HashMap<(String, String), Hlc> = HashMap::new();
        for (origin, wall, counter, entity, key, _, field, _) in &rows {
            if entity == "game" && field == "identity" {
                let clock = Hlc::new(*wall, *counter);
                let newest = identities
                    .entry((origin.clone(), key.clone()))
                    .or_insert(clock);
                if clock > *newest {
                    *newest = clock;
                }
            }
        }
        let mut drop = Vec::new();
        for (origin, wall, counter, entity_name, key, target, field, value) in rows {
            let stamp = Stamp {
                clock: Hlc::new(wall, counter),
                origin,
            };
            let Some(entity) = Entity::parse(&entity_name) else {
                continue;
            };
            let gone = if entity == Entity::Game && field == "identity" {
                let newest = identities[&(stamp.origin.clone(), key.clone())];
                newest > stamp.clock
                    && seen_by_all(&Stamp {
                        clock: newest,
                        origin: stamp.origin.clone(),
                    })
            } else {
                let winner = target
                    .as_ref()
                    .and_then(|target| winners.get(&(entity, target.clone(), field.clone())));
                match winner {
                    // Replaced: once the replacement reached everyone.
                    Some(winner) if winner.stamp != stamp => seen_by_all(&winner.stamp),
                    // A live tombstone: once everyone has deleted.
                    Some(_) => value.is_none() && seen_by_all(&stamp),
                    // Superseded without a target (a game since removed here).
                    None => seen_by_all(&stamp),
                }
            };
            if gone {
                drop.push(stamp);
            }
        }
        let mut tx = self.begin_write().await?;
        let mut removed = 0;
        for stamp in &drop {
            removed +=
                sqlx::query("DELETE FROM sync_changes WHERE origin=? AND wall=? AND counter=?")
                    .bind(&stamp.origin)
                    .bind(stamp.clock.wall)
                    .bind(stamp.clock.counter)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
        }
        tx.commit().await?;
        Ok(removed)
    }
}
