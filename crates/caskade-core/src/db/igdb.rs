//! Library games' IGDB matches (`igdb_matches`), their age ratings
//! (`games.age_ratings_json` and `games.min_age`, #204) and the library
//! games of catalog ids (#163). The service lives in `crate::metadata`.

use super::{LibraryStore, decode};
use crate::{
    Result,
    igdb::{IgdbAgeRating, age},
    metadata::{IgdbMatch, MatchSource},
    model::{ExternalId, validation as v},
};
use serde::Serialize;
use specta::Type;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

/// A library game's age ratings and the minimum age they come to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameAgeRatings {
    pub ratings: Vec<IgdbAgeRating>,
    /// The highest of the ratings' minimum ages ([`age::min_age`]); `None`
    /// when unrated.
    pub min_age: Option<u8>,
}

/// A library game an IGDB game is in the library as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IgdbOwned {
    #[serde(with = "crate::igdb::decimal")]
    #[specta(type = String)]
    pub igdb_id: u64,
    pub game_id: String,
    /// One of its entries is installed.
    pub installed: bool,
}

/// Merges `remove`'s ratings into `keep`'s, for a merge of the two: a
/// merged game takes the highest minimum age over its releases.
pub(super) async fn merge_age_ratings(
    connection: &mut SqliteConnection,
    keep: &str,
    remove: &str,
) -> Result<()> {
    let ratings = |id: &str| {
        sqlx::query_scalar::<Sqlite, String>("SELECT age_ratings_json FROM games WHERE id=?")
            .bind(id.to_owned())
    };
    let kept: Vec<IgdbAgeRating> = decode(ratings(keep).fetch_one(&mut *connection).await?)?;
    let donor: Vec<IgdbAgeRating> = decode(ratings(remove).fetch_one(&mut *connection).await?)?;
    let mut merged = kept.clone();
    merged.extend(donor);
    let merged = age::clean(&merged);
    if merged != kept {
        write_age_ratings(connection, keep, &merged).await?;
    }
    Ok(())
}

async fn write_age_ratings(
    connection: &mut SqliteConnection,
    game_id: &str,
    ratings: &[IgdbAgeRating],
) -> Result<()> {
    sqlx::query("UPDATE games SET age_ratings_json=?, min_age=? WHERE id=?")
        .bind(v::json(&ratings)?)
        .bind(age::min_age(ratings))
        .bind(game_id)
        .execute(connection)
        .await?;
    Ok(())
}

type Row = (String, Option<i64>, String, bool, i64, String, String);

fn read(row: Row) -> Result<IgdbMatch> {
    let (game_id, igdb_id, source, pending, checked_at_ms, dumps, applied) = row;
    Ok(IgdbMatch {
        game_id,
        igdb_id: igdb_id.map(i64::cast_unsigned),
        source: MatchSource::parse(&source)?,
        pending,
        checked_at_ms,
        dumps,
        applied: decode(applied)?,
    })
}

impl LibraryStore {
    pub async fn igdb_match(&self, game_id: &str) -> Result<Option<IgdbMatch>> {
        sqlx::query_as::<_, Row>(
            "SELECT game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json \
             FROM igdb_matches WHERE game_id=?",
        )
        .bind(game_id)
        .fetch_optional(&self.pool)
        .await?
        .map(read)
        .transpose()
    }

    pub async fn save_igdb_match(&self, row: &IgdbMatch) -> Result<()> {
        self.write_igdb_match(row, "").await.map(drop)
    }

    /// Saves an automatic match, unless the game has one chosen by hand.
    /// Whether it was saved.
    pub async fn save_auto_igdb_match(&self, row: &IgdbMatch) -> Result<bool> {
        self.write_igdb_match(row, " WHERE igdb_matches.source<>'manual'")
            .await
    }

    async fn write_igdb_match(&self, row: &IgdbMatch, guard: &'static str) -> Result<bool> {
        v::id(&row.game_id)?;
        let igdb_id = row
            .igdb_id
            .map(|id| {
                i64::try_from(id).ok().filter(|id| *id > 0).ok_or_else(|| {
                    crate::Error::Invalid(crate::tr!("{id} is not an IGDB id", id = id))
                })
            })
            .transpose()?;
        // Constants only.
        let sql = format!(
            "INSERT INTO igdb_matches(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json) \
             VALUES(?,?,?,?,?,?,?) ON CONFLICT(game_id) DO UPDATE SET igdb_id=excluded.igdb_id,\
             source=excluded.source,pending=excluded.pending,checked_at_ms=excluded.checked_at_ms,\
             dumps=excluded.dumps,applied_json=excluded.applied_json{guard}"
        );
        let saved = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(&row.game_id)
            .bind(igdb_id)
            .bind(row.source.as_str())
            .bind(row.pending)
            .bind(row.checked_at_ms)
            .bind(&row.dumps)
            .bind(v::json(&row.applied)?)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(saved > 0)
    }

    /// The game's age ratings; `None` when there is no such game.
    pub async fn game_age_ratings(&self, game_id: &str) -> Result<Option<GameAgeRatings>> {
        let Some((ratings, min_age)) = sqlx::query_as::<_, (String, Option<u8>)>(
            "SELECT age_ratings_json, min_age FROM games WHERE id=?",
        )
        .bind(game_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        Ok(Some(GameAgeRatings {
            ratings: decode(ratings)?,
            min_age,
        }))
    }

    /// Replaces the game's age ratings (cleaned) and its minimum age with
    /// theirs.
    pub async fn set_game_age_ratings(
        &self,
        game_id: &str,
        ratings: &[IgdbAgeRating],
    ) -> Result<()> {
        let mut connection = self.pool.acquire().await?;
        write_age_ratings(&mut connection, game_id, &age::clean(ratings)).await
    }

    /// The library games matched to `igdb_ids`, hidden ones left out: a
    /// catalog game's "in your library".
    pub async fn igdb_owned(&self, igdb_ids: &[u64]) -> Result<Vec<IgdbOwned>> {
        let mut owned = Vec::new();
        for chunk in igdb_ids.chunks(500) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT m.igdb_id, g.id, EXISTS(SELECT 1 FROM game_entries e WHERE e.game_id=g.id \
                 AND e.state_kind IN ('installed','updating','broken')) \
                 FROM igdb_matches m JOIN games g ON g.id=m.game_id \
                 WHERE g.hidden=0 AND m.igdb_id IN (",
            );
            let mut list = query.separated(", ");
            for id in chunk.iter().filter_map(|id| i64::try_from(*id).ok()) {
                list.push_bind(id);
            }
            query.push(") ORDER BY g.sort_title COLLATE NOCASE, g.id");
            let rows: Vec<(i64, String, bool)> =
                query.build_query_as().fetch_all(&self.pool).await?;
            owned.extend(
                rows.into_iter()
                    .map(|(igdb_id, game_id, installed)| IgdbOwned {
                        igdb_id: igdb_id.cast_unsigned(),
                        game_id,
                        installed,
                    }),
            );
        }
        Ok(owned)
    }

    /// The games matching has work for: never looked at, matched but not
    /// applied, or without a match since before `retry_before_ms` or
    /// against other Hasheous `dumps` (`None`: unknown, so no change). A
    /// RomM row without an id is from a version that left RomM games
    /// alone; they are matched now.
    pub async fn igdb_pending(
        &self,
        dumps: Option<&str>,
        retry_before_ms: i64,
    ) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT g.id FROM games g LEFT JOIN igdb_matches m ON m.game_id=g.id \
             WHERE m.game_id IS NULL OR m.pending=1 \
             OR (m.source='none' AND (m.dumps<>coalesce(?1, m.dumps) OR m.checked_at_ms<?2)) \
             OR (m.source='romm' AND m.igdb_id IS NULL) \
             ORDER BY g.sort_title COLLATE NOCASE, g.id",
        )
        .bind(dumps)
        .bind(retry_before_ms)
        .fetch_all(&self.pool)
        .await?)
    }

    /// The store ids other devices know `game_id` by: those the identities
    /// they sent it with name (#75). A game that came from another device
    /// has no entries here until it's installed, so matching asks about
    /// these.
    pub async fn synced_store_ids(&self, game_id: &str) -> Result<Vec<ExternalId>> {
        let identities: Vec<String> = sqlx::query_scalar(
            "SELECT value_json FROM sync_changes \
             WHERE entity='game' AND target=? AND field='identity' AND value_json IS NOT NULL \
             ORDER BY wall DESC,counter DESC",
        )
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?;
        let mut ids = Vec::new();
        for identity in identities {
            let Ok(identity) = serde_json::from_str::<serde_json::Value>(&identity) else {
                continue;
            };
            let keys = identity
                .get("stores")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str);
            for id in keys.filter_map(store_id) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        Ok(ids)
    }

    /// The IGDB id the device that sent `game_id` matched it to, from the
    /// newest identity naming one (#75). Its automatic match counts: a ROM
    /// matched by name or by RomM there has no store id to ask about here.
    pub async fn synced_igdb_id(&self, game_id: &str) -> Result<Option<u64>> {
        let igdb: Option<i64> = sqlx::query_scalar(
            "SELECT json_extract(value_json,'$.igdb') FROM sync_changes \
             WHERE entity='game' AND target=? AND field='identity' \
             AND json_valid(value_json) AND json_type(value_json,'$.igdb')='integer' \
             ORDER BY wall DESC,counter DESC LIMIT 1",
        )
        .bind(game_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        Ok(igdb
            .and_then(|id| u64::try_from(id).ok())
            .filter(|id| *id > 0))
    }
}

/// The store id a synced identity names by `key` (`sync_data`'s store
/// keys: the id's JSON, or `epic:<namespace>:<catalog item>`).
fn store_id(key: &str) -> Option<ExternalId> {
    match key.strip_prefix("epic:") {
        Some(epic) => {
            let (namespace, catalog_item) = epic.split_once(':')?;
            Some(ExternalId::Epic {
                namespace: namespace.to_owned(),
                catalog_item: catalog_item.to_owned(),
                app_name: String::new(),
            })
        }
        None => serde_json::from_str(key).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Game;

    /// A game another device sent, with the identity it sent it with.
    async fn synced_game(store: &LibraryStore, stores: serde_json::Value) -> String {
        let game = store.upsert_game(&Game::new("Portal 2")).await.unwrap();
        let identity = serde_json::json!({"title": "Portal 2", "stores": stores});
        sqlx::query(
            "INSERT INTO sync_changes(origin,wall,counter,entity,key,field,value_json,target,applied) \
             VALUES('desk',1,0,'game','g1','identity',?,?,1)",
        )
        .bind(identity.to_string())
        .bind(&game.id)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sync_game_ids(origin,origin_id,game_id,method) VALUES('desk','g1',?,'created')",
        )
        .bind(&game.id)
        .execute(&store.pool)
        .await
        .unwrap();
        game.id
    }

    #[tokio::test]
    async fn synced_identities_name_the_games_store_ids() {
        let store = LibraryStore::in_memory().await.unwrap();
        let steam = serde_json::to_string(&ExternalId::Steam { app_id: 620 }).unwrap();
        let game = synced_game(
            &store,
            serde_json::json!([steam, "epic:crab:item", "epic:broken", "not json"]),
        )
        .await;
        assert_eq!(
            store.synced_store_ids(&game).await.unwrap(),
            [
                ExternalId::Steam { app_id: 620 },
                ExternalId::Epic {
                    namespace: "crab".into(),
                    catalog_item: "item".into(),
                    app_name: String::new(),
                },
            ]
        );
        let local = store.upsert_game(&Game::new("Local")).await.unwrap();
        assert!(store.synced_store_ids(&local.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn synced_games_found_without_a_match_are_due_again_after_the_upgrade() {
        let store = LibraryStore::in_memory().await.unwrap();
        let synced = synced_game(&store, serde_json::json!([])).await;
        let local = store.upsert_game(&Game::new("Local")).await.unwrap();
        for game in [&synced, &local.id] {
            let row = IgdbMatch {
                game_id: game.clone(),
                igdb_id: None,
                source: MatchSource::None,
                pending: false,
                checked_at_ms: crate::now_ms(),
                dumps: String::new(),
                applied: Default::default(),
            };
            store.save_igdb_match(&row).await.unwrap();
        }
        let week_ago = crate::now_ms() - 7 * 24 * 60 * 60 * 1000;
        assert!(store.igdb_pending(None, week_ago).await.unwrap().is_empty());
        sqlx::raw_sql(include_str!(
            "../../migrations/0036_synced_games_rematch.up.sql"
        ))
        .execute(&store.pool)
        .await
        .unwrap();
        assert_eq!(store.igdb_pending(None, week_ago).await.unwrap(), [synced]);
    }
}
