//! Rows written to the library, counted as SQLite reports them, so work
//! that follows the library (sync's capture) can skip a pass when nothing
//! changed instead of reading every table again.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use sqlx::{
    Error, SqliteConnection,
    pool::PoolConnectionMetadata,
    sqlite::{SqlitePoolOptions, UpdateHookResult},
};

use super::LibraryStore;

/// Sync's own bookkeeping, which a capture writes: counting it would make
/// every capture call for another.
const UNCOUNTED: &[&str] = &["sync_changes", "sync_fields", "sync_peers", "sync_seen"];

/// How many rows have changed in the library's tables, other than
/// [`UNCOUNTED`] ones, since it opened.
#[derive(Debug, Default)]
pub(crate) struct Writes(AtomicU64);

impl Writes {
    fn count(&self, change: UpdateHookResult<'_>) {
        if change.database == "main" && !UNCOUNTED.contains(&change.table) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `options` counting every pooled connection's writes into `writes`. The
/// update hook sees each inserted, updated and deleted row; the library has
/// no `WITHOUT ROWID` tables or unfiltered `DELETE FROM`s, the writes it
/// would miss.
pub(super) fn counting(options: SqlitePoolOptions, writes: &Arc<Writes>) -> SqlitePoolOptions {
    let writes = writes.clone();
    options.after_connect(
        move |connection: &mut SqliteConnection, _: PoolConnectionMetadata| {
            let writes = writes.clone();
            Box::pin(async move {
                let mut handle = connection.lock_handle().await?;
                handle.set_update_hook(move |change| writes.count(change));
                Ok::<(), Error>(())
            })
        },
    )
}

impl LibraryStore {
    /// Rows changed so far, for telling whether anything was written since
    /// an earlier reading. Writes by other processes don't count.
    pub fn writes(&self) -> u64 {
        self.writes.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::LibraryStore;

    #[tokio::test]
    async fn library_writes_count_and_sync_bookkeeping_does_not() {
        let store = LibraryStore::in_memory().await.unwrap();
        let start = store.writes();
        sqlx::query("INSERT INTO settings (key, value_json) VALUES ('a', '1')")
            .execute(&store.pool)
            .await
            .unwrap();
        let after_insert = store.writes();
        assert!(after_insert > start);
        sqlx::query("SELECT key FROM settings")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        assert_eq!(store.writes(), after_insert, "reads aren't writes");
        sqlx::query("INSERT INTO sync_seen (origin, wall, counter) VALUES ('d', 1, 0)")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            store.writes(),
            after_insert,
            "sync's bookkeeping isn't counted"
        );
    }
}
