//! Ludusavi's save manifest, downloaded once and queried offline through SQLite.
//! Each complete YAML/index pair has its own directory. A small SQLite catalog
//! publishes the pair atomically; neither failed refreshes nor offline starts
//! discard the last working generation. No manifest is bundled with Caskade.

#[cfg(feature = "net")]
mod download;
mod index;
mod manifest;

pub use manifest::{Constraint, Entry, Game, Ids, PrimaryId, StoreId, apply_secondary};

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell};

pub const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY: Duration = Duration::from_secs(60);
const MAX_RETRY: Duration = Duration::from_secs(15 * 60);

/// What a refresh did. A first offline start has no entries, but is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    Offline,
    Fresh,
    Unchanged,
    Updated { games: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    generation: String,
    url: String,
    etag: Option<String>,
    checked: u64,
}

#[derive(Clone)]
pub struct Ludusavi {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    url: String,
    catalog: OnceCell<SqlitePool>,
    active: Mutex<Option<(String, SqlitePool)>>,
    refresh: Mutex<()>,
}

impl Ludusavi {
    /// `dir` is `<cache>/ludusavi/`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_url(dir, MANIFEST_URL)
    }

    /// Use the configured `core.saves.manifestUrl`, or a local test server.
    pub fn with_url(dir: impl Into<PathBuf>, url: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                dir: dir.into(),
                url: url.into(),
                catalog: OnceCell::new(),
                active: Mutex::new(None),
                refresh: Mutex::new(()),
            }),
        }
    }

    async fn catalog(&self) -> Result<&SqlitePool> {
        self.inner.catalog.get_or_try_init(|| async {
            crate::cache_db::pool(&self.inner.dir.join("catalog.sqlite"), 1, |_| {
                "CREATE TABLE IF NOT EXISTS current(id INTEGER PRIMARY KEY CHECK(id=1), state TEXT NOT NULL);"
            }).await
        }).await
    }

    async fn cached(&self) -> Result<Option<Cached>> {
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM current WHERE id=1")
            .fetch_optional(self.catalog().await?)
            .await?;
        let cached: Option<Cached> = state
            .map(|state| serde_json::from_str(&state))
            .transpose()?;
        if let Some(cached) = &cached
            && uuid::Uuid::parse_str(&cached.generation).is_err()
        {
            // A locally damaged catalog must never supply an arbitrary path.
            return Err(invalid("invalid Ludusavi cache generation"));
        }
        Ok(cached)
    }

    async fn publish(&self, cached: &Cached) -> Result<()> {
        sqlx::query("INSERT INTO current(id,state) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET state=excluded.state")
            .bind(serde_json::to_string(cached)?)
            .execute(self.catalog().await?)
            .await?;
        Ok(())
    }

    async fn index(&self) -> Result<Option<SqlitePool>> {
        let Some(cached) = self.cached().await? else {
            return Ok(None);
        };
        let mut active = self.inner.active.lock().await;
        if let Some((generation, pool)) = &*active
            && generation == &cached.generation
        {
            return Ok(Some(pool.clone()));
        }
        let pool =
            index::open(&self.inner.dir.join(&cached.generation).join("index.sqlite")).await?;
        *active = Some((cached.generation, pool.clone()));
        Ok(Some(pool))
    }

    /// Exact title or alias after Unicode normalization, case and punctuation
    /// folding. Reads the index only, including after a restart.
    pub async fn by_title(&self, title: &str) -> Result<Vec<Game>> {
        match self.index().await? {
            Some(pool) => index::by_title(&pool, title).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn by_id(&self, id: StoreId) -> Result<Vec<Game>> {
        match self.index().await? {
            Some(pool) => index::by_id(&pool, id).await,
            None => Ok(Vec::new()),
        }
    }

    /// Load a game's optional `.ludusavi.yaml` during save discovery. Its
    /// additions and overrides are scoped to that folder, without changing
    /// the shared manifest. Retain the returned entries for later lookups.
    /// Relative file paths use Ludusavi's `<base>` placeholder; resolving
    /// placeholders to OS paths belongs to the save-location resolver.
    pub async fn load_secondary(&self, game_dir: &Path) -> Result<Vec<Game>> {
        use tokio::io::AsyncReadExt;
        let file = match tokio::fs::File::open(game_dir.join(".ludusavi.yaml")).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        const LIMIT: u64 = 4 << 20;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes).await?;
        if bytes.len() as u64 > LIMIT {
            return Err(invalid("secondary Ludusavi manifest exceeds 4 MiB"));
        }
        let games = tokio::task::spawn_blocking(move || manifest::parse(&bytes))
            .await
            .map_err(|error| invalid(error.to_string()))??;
        let mut result = Vec::new();
        for (title, mut game) in games {
            game.files = game
                .files
                .into_iter()
                .map(|(path, entry)| {
                    let relative = path.starts_with("./")
                        || path.starts_with(".\\")
                        || path.starts_with("../")
                        || path.starts_with("..\\");
                    let path = if relative {
                        let path = path.replace('\\', "/");
                        format!("<base>/{}", path.strip_prefix("./").unwrap_or(&path))
                    } else {
                        path
                    };
                    (path, entry)
                })
                .collect();
            if let Some(primary) = self
                .by_title(&title)
                .await?
                .into_iter()
                .find(|primary| primary.title == title)
            {
                game = manifest::merge_secondary(&primary, game);
            }
            result.push(game);
        }
        Ok(result)
    }

    /// Refresh only when online and at least a day after the last successful
    /// check. Errors leave both the old check time and old generation intact.
    pub async fn refresh(&self, online: bool) -> Result<Refresh> {
        if !online {
            return Ok(Refresh::Offline);
        }
        let _refresh = self.inner.refresh.lock().await;
        self.refresh_at(crate::now_ms().max(0) as u64 / 1000).await
    }

    async fn refresh_at(&self, now: u64) -> Result<Refresh> {
        let cached = self.cached().await?;
        let usable = match &cached {
            Some(cached) => {
                tokio::fs::try_exists(self.inner.dir.join(&cached.generation).join("index.sqlite"))
                    .await?
            }
            None => false,
        };
        let same_source = cached
            .as_ref()
            .is_some_and(|cached| cached.url == self.inner.url);
        if usable
            && same_source
            && cached
                .as_ref()
                .is_some_and(|cached| now >= cached.checked && now - cached.checked < DAY.as_secs())
        {
            return Ok(Refresh::Fresh);
        }
        let etag = cached
            .as_ref()
            .filter(|_| usable && same_source)
            .and_then(|cached| cached.etag.as_deref());
        let conditional = etag.is_some();
        match download::fetch(&self.inner.url, etag).await? {
            download::Response::Unchanged => {
                let mut cached = cached
                    .filter(|_| conditional)
                    .ok_or_else(|| invalid("unsolicited Ludusavi 304"))?;
                cached.checked = now;
                self.publish(&cached).await?;
                Ok(Refresh::Unchanged)
            }
            download::Response::Changed { bytes, etag } => {
                let generation = uuid::Uuid::now_v7().to_string();
                let dir = self.inner.dir.join(&generation);
                let prepared = index::build(&dir, bytes).await;
                let games = match prepared {
                    Ok(games) => games,
                    Err(error) => {
                        let _ = tokio::fs::remove_dir_all(&dir).await;
                        return Err(error);
                    }
                };
                self.publish(&Cached {
                    generation: generation.clone(),
                    url: self.inner.url.clone(),
                    etag,
                    checked: now,
                })
                .await?;
                // Retain the preceding complete generation too. Cleanup only
                // follows a successful download, never an offline lookup.
                self.prune(
                    &generation,
                    cached.as_ref().map(|cached| cached.generation.as_str()),
                )
                .await;
                Ok(Refresh::Updated { games })
            }
        }
    }

    async fn prune(&self, current: &str, previous: Option<&str>) {
        let Ok(mut entries) = tokio::fs::read_dir(&self.inner.dir).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name != current && Some(name) != previous && uuid::Uuid::parse_str(name).is_ok() {
                let _ = tokio::fs::remove_dir_all(entry.path()).await;
            }
        }
    }

    /// Run under the app's online supervisor. Offline requests fail harmlessly
    /// and retry after 1, 2, 4, … minutes, capped at 15 minutes. The daily TTL
    /// survives restarts. Abort the returned handle to stop the watcher.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        self.watch_inner(None)
    }

    /// Follows the manifest URL setting while the online supervisor runs it.
    pub fn watch_settings(
        &self,
        settings: crate::settings::SettingsService,
    ) -> tokio::task::JoinHandle<()> {
        self.watch_inner(Some(settings))
    }

    fn watch_inner(
        &self,
        settings: Option<crate::settings::SettingsService>,
    ) -> tokio::task::JoinHandle<()> {
        let mut this = self.clone();
        tokio::spawn(async move {
            let mut retry = RETRY;
            loop {
                if let Some(settings) = &settings
                    && let Ok(snapshot) = settings.get("core.saves").await
                    && let Some(url) = snapshot.value["manifestUrl"].as_str()
                    && url != this.inner.url
                {
                    this = Self::with_url(this.inner.dir.clone(), url);
                }
                let delay = match this.refresh(true).await {
                    Ok(_) => {
                        retry = RETRY;
                        // Polling locally makes a source change or a check
                        // just short of 24 hours wait at most a minute.
                        RETRY
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Ludusavi manifest refresh failed; keeping cache");
                        let delay = retry;
                        retry = (retry * 2).min(MAX_RETRY);
                        delay
                    }
                };
                tokio::time::sleep(delay).await;
            }
        })
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

#[cfg(not(feature = "net"))]
mod download {
    use crate::Result;
    #[expect(dead_code, reason = "network-free builds only query existing indices")]
    pub(super) enum Response {
        Unchanged,
        Changed {
            bytes: Vec<u8>,
            etag: Option<String>,
        },
    }
    pub(super) async fn fetch(_: &str, _: Option<&str>) -> Result<Response> {
        Err(super::invalid("built without network support"))
    }
}
