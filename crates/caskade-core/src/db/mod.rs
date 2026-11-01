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
mod manual;
pub use imports::ImportedGroup;
mod notifications;
pub use notifications::Ingested;
mod query;
mod records;
mod rom_scans;
mod romm;
mod same_game;
mod scripts;
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
mod platform_tests;
mod playtime;
mod popularity;
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
#[derive(Debug, Clone)]
pub struct LibraryStore {
    pub(super) pool: SqlitePool,
    /// Titles' order in the language (#218), as the `caskade_title`
    /// collation sorts them.
    titles: Arc<collation::TitleOrder>,
}
impl LibraryStore {
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .disable_statement_logging();
        Self::connect(options, 4, None).await
    }
    pub async fn open_default(paths: &AppPaths) -> Result<Self> {
        Self::open(&paths.database()).await
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
        let pool = SqlitePoolOptions::new()
            .max_connections(max)
            .connect_with(options)
            .await?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool, titles })
    }
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

fn decode<T: serde::de::DeserializeOwned>(text: String) -> Result<T> {
    Ok(serde_json::from_str(&text)?)
}
pub(crate) fn missing(entity: &str, id: &str) -> crate::Error {
    crate::Error::Invalid(match entity {
        "account" => crate::tr!("account {id} was not found", id = id),
        "collection" => crate::tr!("collection {id} was not found", id = id),
        "entry" => crate::tr!("entry {id} was not found", id = id),
        "game" => crate::tr!("game {id} was not found", id = id),
        "session" => crate::tr!("session {id} was not found", id = id),
        "storage root" => crate::tr!("storage root {id} was not found", id = id),
        _ => crate::tr!("{entity} {id} was not found", entity = entity, id = id),
    })
}
