//! SQLite library store: migrations, repositories, FTS5 search (#105).
//! Async SQLite library repository. One bundled SQLite is shared with
//! discovery; no process-global database, runtime, or logging configuration.
#[cfg(test)]
mod account_tests;
mod accounts;
mod achievements;
pub use achievements::Applied as AchievementsApplied;
mod artwork_choice;
mod backup;
pub(crate) use backup::ARTWORK_HASHES;
mod actions;
#[cfg(test)]
mod actions_tests;
mod collation;
mod emulation;
mod game_edit;
mod games;
mod icons;
mod igdb;
pub use igdb::{GameAgeRatings, IgdbOwned};
mod plugins;
pub use plugins::{PLUGIN_KEY_MAX, PLUGIN_KV_TOTAL_MAX, PLUGIN_VALUE_MAX, PluginRow};
mod imports;
mod managed;
mod managed_tools;
mod streaming;
pub use streaming::{StreamApp, StreamSync, stream_source};
mod manual;
pub use imports::{ImportedGroup, SourceRemoval};
mod notifications;
pub use notifications::Ingested;
mod query;
mod records;
mod rom_scans;
mod romm;
mod same_game;
mod scripts;
mod shares;
pub use shares::{RecipientRow, ShareRow};
#[cfg(test)]
mod scripts_tests;
pub use rom_scans::ScanState;
#[cfg(test)]
mod artwork_choice_tests;
#[cfg(test)]
mod game_actions_tests;
#[cfg(test)]
mod game_edit_tests;
#[cfg(test)]
mod hidden_tests;
#[cfg(test)]
mod letter_tests;
#[cfg(test)]
mod migration_checksum_tests;
mod paired_devices;
#[cfg(test)]
mod platform_tests;
mod playtime;
mod sync_accounts;
mod sync_artwork;
mod sync_carried;
mod sync_copies;
mod sync_data;
mod sync_log;
#[cfg(test)]
mod write_lock_tests;
mod writes;
pub use sync_accounts::is_marker as is_stand_in_marker;
pub use sync_artwork::SyncedArtwork;
pub use sync_copies::{AnnouncedCopy, CopyLocation, PEER_PREFIX, PeerCopyRecord, this_os};
pub use sync_data::{Applied, SESSIONS_SOURCE, Snapshot, normal_title};
pub use sync_log::{FieldKey, FieldState, Seen};
mod popularity;
pub use paired_devices::PairedDeviceRecord;
pub use playtime::{PlaytimeBreakdown, PlaytimeImport, RemoteTotal, ReportedPlaytime};
mod sessions;
mod store_accounts;
#[cfg(test)]
mod tests;
mod updates;

use crate::{AppPaths, Result};
use sqlx::{
    ConnectOptions, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, sync::Arc, time::Duration};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// How long opening a database file may take: its first connection, which
/// creates or opens the file and switches it to WAL, and the migrations.
/// Queries afterwards wait sqlx's default for a connection, so a stuck one
/// still fails fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opening {
    pub wait: Duration,
}

impl Opening {
    /// As long as a query waits for a connection.
    pub const QUICK: Self = Self {
        wait: Duration::from_secs(30),
    };
    /// Caskade's own databases: the library at startup and after a restore,
    /// and the caches (#37). A slow disk, where one write can stall for
    /// minutes, is waited out rather than refusing to start (#36).
    pub const PATIENT: Self = Self {
        wait: Duration::from_secs(300),
    };

    /// Opens `pool` within this bound: after `before` (a slow disk, in
    /// tests), its first connection, then `ready` (migrations, a schema).
    /// A pool waits the same for every connection, a query's included, so
    /// the first, which creates or opens the file, is tried again for as
    /// long as this allows; back in the pool, it's the one `ready`, then the
    /// first query, take at once. The pool is closed if opening fails.
    pub(crate) async fn open(
        self,
        pool: &SqlitePool,
        before: impl std::future::Future<Output = ()>,
        ready: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        let opening = async {
            before.await;
            loop {
                match pool.acquire().await {
                    Err(sqlx::Error::PoolTimedOut) => continue,
                    acquired => drop(acquired?),
                }
                break;
            }
            ready.await
        };
        let opened = tokio::time::timeout(self.wait, opening)
            .await
            .unwrap_or_else(|_| Err(sqlx::Error::PoolTimedOut.into()));
        if opened.is_err() {
            pool.close().await;
        }
        opened
    }
}

#[cfg(test)]
mod opening_tests;
#[derive(Debug, Clone)]
pub struct LibraryStore {
    pub(super) pool: SqlitePool,
    /// Rows written, for skipping work when nothing changed.
    writes: Arc<writes::Writes>,
    /// Titles' order in the language (#218), as the `caskade_title`
    /// collation sorts them.
    titles: Arc<collation::TitleOrder>,
}
impl LibraryStore {
    pub async fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, Opening::QUICK).await
    }
    pub async fn open_with(path: &Path, opening: Opening) -> Result<Self> {
        Self::open_after(path, opening, std::future::ready(())).await
    }
    /// Opens `path` once `before` is done, all within `opening`: `before`
    /// stands in for a slow disk in tests.
    async fn open_after(
        path: &Path,
        opening: Opening,
        before: impl std::future::Future<Output = ()>,
    ) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await?;
        }
        let titles = Arc::new(collation::TitleOrder::new(None));
        let order = titles.clone();
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .disable_statement_logging()
            .collation(collation::TITLE, move |a, b| order.compare(a, b));
        let writes = Arc::default();
        let pool = writes::counting(SqlitePoolOptions::new(), &writes)
            .max_connections(4)
            .connect_lazy_with(options);
        let migrate = async { Ok(MIGRATOR.run(&pool).await?) };
        opening.open(&pool, before, migrate).await?;
        Ok(Self {
            pool,
            writes,
            titles,
        })
    }
    /// A transaction that writes: it takes the write lock at `BEGIN`. A
    /// deferred one that reads first can't wait for the lock when it comes
    /// to write while another connection is writing (SQLite never waits
    /// while it holds a read snapshot), so it failed at once with "database
    /// is locked" whenever an import or a sync wrote at the same time.
    /// Waiting at `BEGIN`, the busy timeout applies. Transactions that only
    /// read keep `pool.begin()`.
    pub(super) async fn begin_write(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }
    /// The library, waited for as `Opening::PATIENT` allows.
    pub async fn open_default(paths: &AppPaths) -> Result<Self> {
        Self::open_with(&paths.database(), Opening::PATIENT).await
    }
    pub async fn in_memory() -> Result<Self> {
        Self::in_memory_sorted(None).await
    }
    /// An in-memory store whose titles sort in `language` rather than the
    /// core's, for tests that run side by side.
    #[cfg(test)]
    pub(crate) async fn in_memory_in(language: &str) -> Result<Self> {
        Self::in_memory_sorted(Some(language)).await
    }
    async fn in_memory_sorted(language: Option<&str>) -> Result<Self> {
        Self::connect(
            SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true)
                .disable_statement_logging(),
            1,
            language,
        )
        .await
    }
    async fn connect(
        options: SqliteConnectOptions,
        max: u32,
        language: Option<&str>,
    ) -> Result<Self> {
        let titles = Arc::new(collation::TitleOrder::new(language));
        let order = titles.clone();
        let options = options.collation(collation::TITLE, move |a, b| order.compare(a, b));
        let writes = Arc::default();
        let pool = writes::counting(SqlitePoolOptions::new(), &writes)
            .max_connections(max)
            .connect_with(options)
            .await?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self {
            pool,
            writes,
            titles,
        })
    }
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

fn decode<T: serde::de::DeserializeOwned>(text: String) -> Result<T> {
    Ok(serde_json::from_str(&text)?)
}
/// `entity` is gone, in the user's words; the id stays in the log.
pub(crate) fn missing(entity: &str, id: &str) -> crate::Error {
    tracing::debug!(entity, id, "not found");
    crate::Error::Invalid(match entity {
        "account" => crate::tr!("This account no longer exists."),
        "collection" => crate::tr!("This collection no longer exists."),
        "entry" => crate::tr!("This copy of the game is no longer in the library."),
        "game" => crate::tr!("This game is no longer in the library."),
        "session" => crate::tr!("This play session no longer exists."),
        "storage root" => crate::tr!("This storage location no longer exists."),
        _ => crate::tr!("This {entity} no longer exists.", entity = entity),
    })
}
