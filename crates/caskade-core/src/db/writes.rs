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

/// Tables whose mutations can change `sync_snapshot()` (excluding
/// `play_sessions`, which gets 5-second open-session heartbeats and marks
/// synced writes explicitly on completion/import/delete).
const SYNCED_TABLES: &[&str] = &[
    "games",
    "game_entries",
    "igdb_matches",
    "sync_game_ids",
    "kept_apart",
    "artwork_choices",
    "collections",
    "collection_members",
    "accounts",
    "account_game_approvals",
    "artwork",
    "lifecycle_scripts",
    "game_actions",
    "entry_action_settings",
    "achievement_links",
    "plugins",
    "settings",
    "platform_settings",
    "emulators",
    "save_versions",
];

/// How many rows have changed in the library's tables since it opened:
/// other than [`UNCOUNTED`] ones, synced tables only, and all tables.
#[derive(Debug, Default)]
pub(crate) struct Writes {
    counted: AtomicU64,
    synced: AtomicU64,
    all: AtomicU64,
}

impl Writes {
    fn count(&self, change: UpdateHookResult<'_>) {
        if change.database != "main" {
            return;
        }
        self.all.fetch_add(1, Ordering::Relaxed);
        if !UNCOUNTED.contains(&change.table) {
            self.counted.fetch_add(1, Ordering::Relaxed);
        }
        if SYNCED_TABLES.contains(&change.table) {
            self.synced.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn mark_synced(&self) {
        self.synced.fetch_add(1, Ordering::Relaxed);
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
    /// an earlier reading. Writes by other processes don't count, and
    /// neither does sync's bookkeeping.
    pub fn writes(&self) -> u64 {
        self.writes.counted.load(Ordering::Relaxed)
    }

    /// [`writes`](Self::writes), sync's bookkeeping included.
    pub fn all_writes(&self) -> u64 {
        self.writes.all.load(Ordering::Relaxed)
    }

    /// Writes to tables that feed `sync_snapshot()`, excluding open-session
    /// heartbeats and local-only tables (notifications, caches, popularity, …).
    pub fn synced_writes(&self) -> u64 {
        self.writes.synced.load(Ordering::Relaxed)
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
        assert!(
            store.all_writes() > after_insert,
            "but counts among all writes"
        );
    }
}
