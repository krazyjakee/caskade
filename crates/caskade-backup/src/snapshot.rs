//! The database snapshot: a `VACUUM INTO` copy of the live library with
//! [`super::policy`]'s exclusions deleted, vacuumed again so nothing deleted
//! survives in free pages, and checked.

use super::{BackupError, manifest::TableRows, policy};
use caskade_core::db::LibraryStore;
use sqlx::{
    AssertSqlSafe, ConnectOptions, Connection,
    sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode},
};
use std::path::Path;

pub(super) struct Snapshot {
    pub schema_version: i64,
    pub tables: Vec<TableRows>,
    /// Content hashes of the artwork the library uses, and whether each
    /// came from IGDB only.
    pub artwork: Vec<(String, bool)>,
}

/// Writes the snapshot to `file`, which must not exist.
pub(super) async fn take(store: &LibraryStore, file: &Path) -> Result<Snapshot, BackupError> {
    store.vacuum_into(file).await?;
    let mut connection = open(file).await?;
    let result = scrub(&mut connection).await;
    let _ = connection.close().await;
    result
}

/// A plain connection: no migrations, no WAL beside the file.
pub(super) async fn open(file: &Path) -> Result<SqliteConnection, BackupError> {
    Ok(SqliteConnectOptions::new()
        .filename(file)
        .journal_mode(SqliteJournalMode::Delete)
        .foreign_keys(false)
        .disable_statement_logging()
        .connect()
        .await
        .map_err(caskade_core::Error::from)?)
}

async fn scrub(connection: &mut SqliteConnection) -> Result<Snapshot, BackupError> {
    let existing = tables(connection).await?;
    for (table, _) in policy::EXCLUDED_TABLES {
        if existing.iter().any(|name| name == table) {
            sqlx::query(AssertSqlSafe(format!("DELETE FROM \"{table}\"")))
                .execute(&mut *connection)
                .await
                .map_err(caskade_core::Error::from)?;
        }
    }
    let (filter, binds) = policy::settings_filter();
    let sql = format!("DELETE FROM settings WHERE {filter}");
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for bind in binds {
        query = query.bind(bind);
    }
    query
        .execute(&mut *connection)
        .await
        .map_err(caskade_core::Error::from)?;
    sqlx::query("VACUUM")
        .execute(&mut *connection)
        .await
        .map_err(caskade_core::Error::from)?;
    check(connection).await?;
    read(connection).await
}

/// `PRAGMA integrity_check` must answer `ok`.
pub(super) async fn check(connection: &mut SqliteConnection) -> Result<(), BackupError> {
    let answers: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut *connection)
        .await
        .map_err(caskade_core::Error::from)?;
    if answers != ["ok"] {
        return Err(BackupError::Damaged(caskade_core::tr!(
            "the library database failed its integrity check: {why}",
            why = answers.join("; ")
        )));
    }
    Ok(())
}

/// The schema version, row counts and artwork of a snapshot.
pub(super) async fn read(connection: &mut SqliteConnection) -> Result<Snapshot, BackupError> {
    let schema_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version),0) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut *connection)
            .await
            .map_err(caskade_core::Error::from)?;
    let mut rows = Vec::new();
    for name in tables(connection).await? {
        let count: i64 =
            sqlx::query_scalar(AssertSqlSafe(format!("SELECT COUNT(*) FROM \"{name}\"")))
                .fetch_one(&mut *connection)
                .await
                .map_err(caskade_core::Error::from)?;
        rows.push(TableRows {
            name,
            rows: u64::try_from(count).unwrap_or(0),
        });
    }
    let artwork = sqlx::query_as(caskade_core::db::ARTWORK_HASHES)
        .fetch_all(&mut *connection)
        .await
        .map_err(caskade_core::Error::from)?;
    Ok(Snapshot {
        schema_version,
        tables: rows,
        artwork,
    })
}

/// The database's own tables, by name: SQLite's internal ones, sqlx's
/// migration record and FTS shadow tables left out.
async fn tables(connection: &mut SqliteConnection) -> Result<Vec<String>, BackupError> {
    Ok(sqlx::query_scalar(
        "SELECT name FROM pragma_table_list WHERE schema='main' \
         AND type IN ('table','virtual') \
         AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         AND name NOT LIKE '\\_sqlx%' ESCAPE '\\' ORDER BY name",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(caskade_core::Error::from)?)
}
