//! Disposable cache databases (IGDB answers, the Hasheous index): Caskade
//! refills them from their source, so a damaged one is started again
//! instead of failing every lookup until someone deletes it by hand (#160).
//! The library database is never opened through here: it holds the user's
//! data.

use crate::{
    Result,
    db::Opening,
    error::{ErrorCode, ErrorReport},
};
use sqlx::{
    AssertSqlSafe, ConnectOptions, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

/// Opens the cache at `path` with `open`. When SQLite says the file is
/// damaged (not a database, corrupt), sets it and its WAL files aside as
/// `<name>.damaged` and opens a fresh one, once. Other failures (no space,
/// no access) are returned unchanged: starting again wouldn't help.
pub(crate) async fn open<T, F, Fut>(path: PathBuf, what: &str, open: F) -> Result<T>
where
    F: Fn(PathBuf) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    match open(path.clone()).await {
        Err(error) if ErrorReport::from_error(&error).code == ErrorCode::Corrupt => {
            tracing::warn!(cache = what, path = %path.display(), %error, "cache is damaged; starting it again");
            set_aside(&path)?;
            open(path).await
        }
        opened => opened,
    }
}

/// A pool on the cache at `path` (created with its folder), at schema
/// `version`. A file at another version runs `change(old version)`, which
/// must be constant SQL, in one transaction. The pool is closed on failure,
/// so `open` can set a damaged file aside, on Windows too.
pub(crate) async fn pool(
    path: &Path,
    version: i64,
    change: impl Fn(i64) -> &'static str,
) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .disable_statement_logging();
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_lazy_with(options);
    let ready = async {
        let current: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await?;
        if current != version {
            let change = change(current);
            sqlx::raw_sql(AssertSqlSafe(format!(
                "BEGIN;{change}PRAGMA user_version = {version};COMMIT;"
            )))
            .execute(&pool)
            .await?;
        }
        Ok(())
    };
    // Opened as patiently as the library: a slow disk is waited out.
    Opening::PATIENT
        .open(&pool, std::future::ready(()), ready)
        .await?;
    Ok(pool)
}

/// Moves the damaged file to `<name>.damaged` (replacing an older one, for
/// diagnostics) and removes its WAL and shared-memory files.
fn set_aside(path: &Path) -> std::io::Result<()> {
    let with = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    for suffix in ["-wal", "-shm"] {
        match std::fs::remove_file(with(suffix)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    std::fs::rename(path, with(".damaged"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    async fn probe(path: PathBuf) -> Result<i64> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await?;
        let version = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await;
        pool.close().await;
        Ok(version?)
    }

    #[tokio::test]
    async fn a_damaged_cache_is_started_again_and_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("games.sqlite");
        std::fs::write(&path, vec![0x5a; 8192]).unwrap();
        assert_eq!(open(path.clone(), "test", probe).await.unwrap(), 0);
        assert!(dir.path().join("games.sqlite.damaged").exists());
        // The fresh file works the next time too.
        assert_eq!(open(path, "test", probe).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn other_failures_are_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let error = open(dir.path().join("x.sqlite"), "test", |_| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async {
                Err::<(), _>(Error::Io(std::io::Error::from(
                    std::io::ErrorKind::PermissionDenied,
                )))
            }
        })
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Io(_)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
