//! The cache's SQLite file (`games.sqlite`): one row per id the server has
//! answered for, with the game's JSON as the server sent it, or none when
//! IGDB has no such game. A schema change drops it, except the change from
//! version 1, which only adds `shape`.
//!
//! `shape` is the [`SHAPE`] of the game a row was fetched as. Rows of an
//! older shape (version 1's, from before age ratings, #204) still answer,
//! but are outdated: the next lookup that needs one fetches it again, once
//! (`super::Igdb::lookup`). A fetch that fails keeps the row, so going
//! offline loses nothing. Not-found rows are never outdated.
//!
//! IGDB ids are signed 64-bit; [`super::Igdb`] never sends or caches a
//! larger one.

use super::IgdbGame;
use crate::{Result, now_ms};
use serde_json::Value;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::{collections::HashMap, path::PathBuf};

const SCHEMA_VERSION: i64 = 2;
const SCHEMA: &str = "
DROP TABLE IF EXISTS games;
CREATE TABLE games(id INTEGER PRIMARY KEY, game TEXT, fetched INTEGER NOT NULL,
    shape INTEGER NOT NULL DEFAULT 1);
";
/// Version 1's rows are kept, as shape 1.
const FROM_1: &str = "ALTER TABLE games ADD COLUMN shape INTEGER NOT NULL DEFAULT 1;";
/// The shape of the games this build fetches: bump it when `IgdbGame`
/// gains fields that cached games should be fetched again for. 2: age
/// ratings, release dates, store links, websites and similar games. 3:
/// videos.
pub(super) const SHAPE: i64 = 3;
/// Ids per `SELECT … IN (…)` and rows per `INSERT` (4 columns), well
/// under SQLite's 32766 parameters.
const ROWS: usize = 500;

pub(super) struct Store {
    pub(super) pool: SqlitePool,
}

impl Store {
    pub(super) async fn open(path: PathBuf) -> Result<Self> {
        let pool = crate::cache_db::pool(&path, SCHEMA_VERSION, |version| {
            if version == 1 { FROM_1 } else { SCHEMA }
        })
        .await?;
        Ok(Self { pool })
    }

    /// The answers cached for `ids`: a game, or `None` when IGDB has none.
    /// Ids without an answer are left out, and so is a row that no longer
    /// reads as a game, so it is fetched again.
    pub(super) async fn get(&self, ids: &[u64]) -> Result<HashMap<u64, Option<IgdbGame>>> {
        let mut known = HashMap::with_capacity(ids.len());
        for chunk in ids.chunks(ROWS) {
            let mut query = QueryBuilder::<Sqlite>::new("SELECT id, game FROM games WHERE id IN (");
            let mut list = query.separated(", ");
            for id in chunk {
                list.push_bind(id.cast_signed());
            }
            query.push(")");
            let rows: Vec<(i64, Option<String>)> =
                query.build_query_as().fetch_all(&self.pool).await?;
            for (id, game) in rows {
                let id = id.cast_unsigned();
                match game.map(|text| serde_json::from_str::<IgdbGame>(&text)) {
                    None => {
                        known.insert(id, None);
                    }
                    Some(Ok(game)) => {
                        known.insert(id, Some(game));
                    }
                    Some(Err(error)) => {
                        tracing::warn!(id, %error, "dropping unreadable cached IGDB game");
                    }
                }
            }
        }
        Ok(known)
    }

    /// Caches `games` (each with the JSON it came as) and `not_found`,
    /// replacing earlier answers for the same ids.
    pub(super) async fn put(&self, games: &[(IgdbGame, Value)], not_found: &[u64]) -> Result<()> {
        let now = now_ms();
        let rows: Vec<(i64, Option<String>)> = games
            .iter()
            .map(|(game, raw)| (game.id.cast_signed(), Some(raw.to_string())))
            .chain(not_found.iter().map(|id| (id.cast_signed(), None)))
            .collect();
        let mut tx = self.pool.begin().await?;
        for chunk in rows.chunks(ROWS) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT OR REPLACE INTO games(id, game, fetched, shape) ",
            );
            query.push_values(chunk, |mut row, (id, game)| {
                row.push_bind(*id)
                    .push_bind(game.as_deref())
                    .push_bind(now)
                    .push_bind(SHAPE);
            });
            query.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Those of `ids` whose answer was fetched at `since_ms` or later.
    pub(super) async fn fetched_since(&self, ids: &[u64], since_ms: i64) -> Result<Vec<u64>> {
        let mut fresh = Vec::new();
        for chunk in ids.chunks(ROWS) {
            let mut query = QueryBuilder::<Sqlite>::new("SELECT id FROM games WHERE fetched >= ");
            query.push_bind(since_ms).push(" AND id IN (");
            let mut list = query.separated(", ");
            for id in chunk {
                list.push_bind(id.cast_signed());
            }
            query.push(")");
            let rows: Vec<i64> = query.build_query_scalar().fetch_all(&self.pool).await?;
            fresh.extend(rows.into_iter().map(i64::cast_unsigned));
        }
        Ok(fresh)
    }

    /// Those of `ids` cached as a game of an older [`SHAPE`].
    pub(super) async fn outdated(&self, ids: &[u64]) -> Result<Vec<u64>> {
        let mut outdated = Vec::new();
        for chunk in ids.chunks(ROWS) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT id FROM games WHERE game IS NOT NULL AND shape < ",
            );
            query.push_bind(SHAPE).push(" AND id IN (");
            let mut list = query.separated(", ");
            for id in chunk {
                list.push_bind(id.cast_signed());
            }
            query.push(")");
            let rows: Vec<i64> = query.build_query_scalar().fetch_all(&self.pool).await?;
            outdated.extend(rows.into_iter().map(i64::cast_unsigned));
        }
        Ok(outdated)
    }

    /// Writes rows as an older build would have, for tests of outdated
    /// answers.
    #[cfg(all(test, feature = "net"))]
    pub(super) async fn put_shape(&self, id: u64, game: &Value, shape: i64) -> Result<()> {
        sqlx::query("INSERT OR REPLACE INTO games(id, game, fetched, shape) VALUES(?, ?, 0, ?)")
            .bind(id.cast_signed())
            .bind(game.to_string())
            .bind(shape)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// How many ids have an answer.
    pub(super) async fn count(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games")
            .fetch_one(&self.pool)
            .await?;
        Ok(count.cast_unsigned())
    }
}
