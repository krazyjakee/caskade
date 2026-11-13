//! The snapshot of synced data the change log diffs (#228–#230): every
//! synced field here, keyed by this device's ids.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value, json};
use sqlx::SqliteConnection;

use super::LibraryStore;
use super::game_edit::{Overrides, automatic};
use super::sync_data::{is_list, record_map};
use super::sync_identity::store_key;
use super::sync_log::FieldKey;
use crate::Result;
use crate::model::{Game, GameField};
use crate::sync::change::{EXISTS, Entity, join_key};

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

impl LibraryStore {
    /// Every synced field here, keyed by this device's ids.
    pub async fn sync_snapshot(&self) -> Result<Snapshot> {
        let mut snapshot = Snapshot::default();
        let mut conn = self.pool.acquire().await?;
        games(&mut conn, &mut snapshot).await?;
        collections(&mut conn, &mut snapshot).await?;
        sessions(&mut conn, &mut snapshot).await?;
        accounts(&mut conn, &mut snapshot).await?;
        // The user's own artwork, and what releases carry (#273).
        super::sync_artwork::snapshot(&mut conn, &mut snapshot.values).await?;
        super::sync_carried::snapshot(&mut conn, &mut snapshot.values).await?;
        super::sync_carried::snapshot_device(&mut conn, &mut snapshot.values).await?;
        // This device's installed copies (#232).
        super::sync_copies::snapshot(&mut conn, &mut snapshot.values).await?;

        // Save versions made here (#239).
        crate::saves::versions::sync_snapshot(&mut conn, &mut snapshot.values).await?;
        Ok(snapshot)
    }
}

/// Games: identity, favourite, hidden and the user's values; kept-apart
/// games, hand-set IGDB matches and artwork choices.
async fn games(conn: &mut SqliteConnection, snapshot: &mut Snapshot) -> Result<()> {
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
    Ok(())
}

/// Collections and their members.
async fn collections(conn: &mut SqliteConnection, snapshot: &mut Snapshot) -> Result<()> {
    for (id, name, kind, query, description, pinned, account) in
        sqlx::query_as::<
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
        >("SELECT id,name,kind,query_json,description,pinned,account_id FROM collections")
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
    Ok(())
}

/// Finished play sessions, and the device each was played on (#74) as a
/// field of its own: a build that doesn't know it leaves it waiting rather
/// than capturing the record back without it.
async fn sessions(conn: &mut SqliteConnection, snapshot: &mut Snapshot) -> Result<()> {
    for (id, game, started, ended, duration, device) in
        sqlx::query_as::<_, (String, String, i64, i64, i64, Option<String>)>(
            "SELECT id,game_id,started_at_ms,ended_at_ms,duration_ms,device_id FROM play_sessions \
         WHERE ended_at_ms IS NOT NULL",
        )
        .fetch_all(&mut *conn)
        .await?
    {
        if let Some(device) = device {
            snapshot.put(Entity::Session, id.clone(), "device", json!(device));
        }
        snapshot.put(
            Entity::Session,
            id,
            EXISTS,
            json!({ "game": game, "started": started, "ended": ended, "duration": duration }),
        );
    }
    Ok(())
}

/// Accounts, with their PIN hashes; never the throttle. An untouched
/// default admin travels only as a stand-in's marker (#273). And which
/// games each may play.
async fn accounts(conn: &mut SqliteConnection, snapshot: &mut Snapshot) -> Result<()> {
    let stand_ins: HashSet<String> = super::sync_accounts::untouched(conn)
        .await?
        .into_iter()
        .collect();
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
        if stand_ins.contains(&id) {
            snapshot.put(Entity::Account, id, EXISTS, super::sync_accounts::marker());
            continue;
        }
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
    Ok(())
}
