//! Opening a library that a newer Caskade wrote. Every migration from
//! [`FIRST_CLASSIFIED`] on says on its first line whether a Caskade without
//! it can still read and write the library:
//!
//! - `-- older Caskade: compatible`: new tables, columns that may be empty
//!   or have a default, looser checks, data fixes.
//! - `-- older Caskade: incompatible`: anything an older Caskade's queries
//!   would trip on (a dropped or renamed table or column, a new column that
//!   must be filled in, a stricter check, a changed meaning).
//!
//! The library keeps the newest incompatible migration it has applied in
//! `PRAGMA user_version`. A Caskade that has that migration opens the
//! library even when it lacks newer ones; one that doesn't says the library
//! is newer, and the app offers to update.

use super::MIGRATOR;
use crate::Result;
use sqlx::{AssertSqlSafe, SqlitePool, migrate::MigrateError, migrate::Migration};

/// Migrations before this one predate the rule; every Caskade that reads
/// the marker has them all.
pub(super) const FIRST_CLASSIFIED: i64 = 38;
pub(super) const INCOMPATIBLE: &str = "-- older Caskade: incompatible";

fn ups() -> impl Iterator<Item = &'static Migration> {
    MIGRATOR
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
}

/// The first line of a migration, which classifies it.
pub(super) fn classification(migration: &Migration) -> &str {
    migration.sql.as_str().lines().next().unwrap_or("").trim()
}

/// The newest incompatible migration this Caskade has: an older Caskade
/// needs it to open a library this one migrated.
pub(super) fn needed() -> i64 {
    ups()
        .filter(|m| m.version >= FIRST_CLASSIFIED && classification(m) == INCOMPATIBLE)
        .map(|m| m.version)
        .max()
        .unwrap_or(0)
}

async fn marker(pool: &SqlitePool) -> Result<i64> {
    Ok(sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?)
}

/// Brings the library up to this Caskade's schema. A library with
/// migrations this Caskade doesn't have opens as it is when none of them is
/// incompatible; otherwise it fails with sqlx's `VersionMissing`, which
/// reports say a newer Caskade wrote.
pub(super) async fn migrate(pool: &SqlitePool) -> Result<()> {
    match MIGRATOR.run(pool).await {
        Ok(()) => {
            let needed = needed();
            if marker(pool).await? != needed {
                sqlx::query(AssertSqlSafe(format!("PRAGMA user_version = {needed}")))
                    .execute(pool)
                    .await?;
            }
            Ok(())
        }
        Err(MigrateError::VersionMissing(version)) => {
            let marked = marker(pool).await?;
            if marked != 0 && !ups().any(|m| m.version == marked) {
                tracing::error!(
                    version,
                    needed = marked,
                    "a newer Caskade changed the library in a way this one can't read"
                );
                return Err(MigrateError::VersionMissing(version).into());
            }
            tracing::warn!(
                version,
                "a newer Caskade migrated the library; its changes are compatible, so it opens"
            );
            let mut lenient =
                sqlx::migrate::Migrator::with_migrations(MIGRATOR.iter().cloned().collect());
            lenient.set_ignore_missing(true);
            // The marker stays the newer Caskade's.
            Ok(lenient.run(pool).await?)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "compat_tests.rs"]
mod tests;
