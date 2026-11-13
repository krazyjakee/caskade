//! Game updates: newer releases of the games Caskade installed, found
//! without accounts or API keys.
//!
//! An install is followed when an update provider knows where it came
//! from: [`Tracker::track`] reads the project, release and file out of the
//! URL the user gave and the file it downloaded. GitHub releases are the
//! first provider ([`GitHub`]); the rest of this module doesn't depend on
//! which one.
//!
//! [`Updates`] checks followed games now and again ([`Updates::watch`]) and
//! before they're played ([`Updates::before_play`]). Updating installs the
//! newer release in the game's place under the same library entry, and
//! carries the user's files across (`InstallService::update_game`).

mod github;
#[cfg(all(test, feature = "net"))]
mod tests;
mod version;

pub use github::GitHub;
pub(crate) use version::{newer, prerelease as looks_prerelease};

use crate::{
    Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
    install::{InstallService, JobSnapshot},
    launch::LaunchService,
    model::InstallLocation,
    now_ms,
    settings::{SettingsService, UpdateSettings},
};
use caskade_installer::{Input, manifest::InstallManifest};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use url::Url;

/// How long Play waits for a check that's due.
const PLAY_TIMEOUT: Duration = Duration::from_secs(5);
/// How often [`Updates::watch`] looks for games due a check.
const TICK: Duration = Duration::from_secs(15 * 60);
/// How long after a failed check the watch and Play leave the install
/// alone, rather than the whole interval of a check that worked.
const RETRY: Duration = Duration::from_secs(30 * 60);

/// What a provider follows for one install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Followed {
    /// The provider's name for the project: `owner/repo` on GitHub.
    pub project: String,
    /// The release installed, when the URLs name it.
    pub version: Option<String>,
    /// The file it was installed from.
    pub asset: Option<String>,
}

/// A release a provider found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// Its page.
    pub url: String,
}

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Somewhere games are released: it recognises installs from it, finds
/// their newest release and says what to install it from.
pub trait UpdateProvider: Send + Sync {
    fn id(&self) -> &str;
    /// What to follow for an install from `urls`, the most specific first
    /// (the file downloaded, then what the user gave). `None` when they
    /// aren't this provider's.
    fn follow(&self, urls: &[Url]) -> Option<Followed>;
    /// The project's releases, for people.
    fn page(&self, project: &str) -> Option<String>;
    /// The newest release; `None` when the project has none.
    fn latest<'a>(
        &'a self,
        project: &'a str,
        prereleases: bool,
    ) -> ProviderFuture<'a, Option<Release>>;
    /// What to install `version` from: the file matching the one installed,
    /// or a URL the installer resolves (and may ask about).
    fn download<'a>(&'a self, row: &'a UpdateRow, version: &'a str) -> ProviderFuture<'a, String>;
}

/// A managed install's update state, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRow {
    pub entry_id: String,
    pub game_id: String,
    /// [`NOT_FOLLOWED`] when no provider knows the install's source.
    pub provider: String,
    pub project: String,
    pub installed: Option<String>,
    pub asset: Option<String>,
    pub prereleases: bool,
    pub latest: Option<String>,
    pub latest_url: Option<String>,
    pub checked_at_ms: i64,
    pub skipped: Option<String>,
    /// Why the last check failed, as stored ([`crate::i18n::Text::store`]):
    /// [`GameUpdate`] translates it.
    pub error: Option<String>,
}

/// The provider of installs no provider follows.
pub const NOT_FOLLOWED: &str = "none";

impl UpdateRow {
    /// Whether the newest release found is newer than the one installed
    /// and wasn't skipped.
    pub fn available(&self) -> bool {
        match (&self.latest, &self.installed) {
            (Some(latest), Some(installed)) => {
                version::newer(latest, installed) && self.skipped.as_ref() != Some(latest)
            }
            _ => false,
        }
    }
}

/// An install's updates, for the game page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameUpdate {
    pub entry_id: String,
    pub game_id: String,
    /// `github`, …
    pub provider: String,
    pub project: String,
    /// The project's releases.
    pub project_url: Option<String>,
    /// `None` until the first check, for installs that didn't say.
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub latest_url: Option<String>,
    /// `latest` is newer than `installed`, and not skipped.
    pub available: bool,
    pub skipped: Option<String>,
    /// Prereleases are offered too.
    pub prereleases: bool,
    /// When a check last reached the provider; 0 before the first.
    pub checked_at_ms: i64,
    /// Why the last check failed.
    pub error: Option<String>,
}

/// Records where installs came from, for the providers it has. Cheap to
/// clone; the install service uses it as installs finish.
#[derive(Clone)]
pub struct Tracker {
    store: LibraryStore,
    providers: Arc<[Arc<dyn UpdateProvider>]>,
}

impl Tracker {
    pub fn new(store: LibraryStore, providers: Vec<Arc<dyn UpdateProvider>>) -> Self {
        Self {
            store,
            providers: providers.into(),
        }
    }

    /// The built-in providers: GitHub releases.
    pub fn builtin(store: LibraryStore) -> Self {
        Self::new(store, vec![Arc::new(GitHub::default())])
    }

    fn provider(&self, id: &str) -> Result<&Arc<dyn UpdateProvider>> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .ok_or_else(|| Error::Invalid(crate::tr!("no update provider {id}", id = id)))
    }

    /// Follows entry `entry_id`, installed per `manifest`, through the first
    /// provider its URLs belong to: `download` (the file the install
    /// downloaded), then the install's input and final URL. The user's
    /// choices (prereleases, a skipped release) stay when the project does.
    pub async fn track(
        &self,
        entry_id: &str,
        manifest: &InstallManifest,
        download: Option<&Url>,
    ) -> Result<Option<UpdateRow>> {
        let source = &manifest.record().source;
        let mut urls: Vec<Url> = download.into_iter().cloned().collect();
        if let Input::Urls(input) = &source.input {
            urls.extend(input.iter().cloned());
        }
        urls.extend(source.final_url.iter().cloned());
        let found = self.providers.iter().find_map(|provider| {
            provider
                .follow(&urls)
                .map(|followed| (provider.id().to_owned(), followed))
        });
        match found {
            Some((provider, followed)) => {
                self.store
                    .follow_updates(entry_id, &provider, &followed)
                    .await?;
                self.store.game_update(entry_id).await
            }
            None => {
                self.store.not_followed(entry_id).await?;
                Ok(None)
            }
        }
    }
}

/// Checks for and installs game updates. Cheap to clone.
#[derive(Clone)]
pub struct Updates {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    tracker: Tracker,
    install: InstallService,
    settings: Option<SettingsService>,
    /// One check of a game at a time, so Play and the watch don't race.
    checking: tokio::sync::Mutex<()>,
    /// Installs whose last check failed, and when to try them again.
    retry_at: Mutex<HashMap<String, Instant>>,
}

impl Updates {
    /// Reads `core.updates` from `settings`; without them the defaults
    /// apply.
    pub fn new(
        store: LibraryStore,
        bus: Bus,
        tracker: Tracker,
        install: InstallService,
        settings: Option<SettingsService>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                tracker,
                install,
                settings,
                checking: tokio::sync::Mutex::new(()),
                retry_at: Mutex::default(),
            }),
        }
    }

    async fn settings(&self) -> UpdateSettings {
        let Some(settings) = &self.inner.settings else {
            return UpdateSettings::default();
        };
        settings.read_or_default("core.updates").await
    }

    fn view(&self, row: &UpdateRow) -> GameUpdate {
        GameUpdate {
            entry_id: row.entry_id.clone(),
            game_id: row.game_id.clone(),
            provider: row.provider.clone(),
            project: row.project.clone(),
            project_url: self
                .inner
                .tracker
                .provider(&row.provider)
                .ok()
                .and_then(|provider| provider.page(&row.project)),
            installed: row.installed.clone(),
            latest: row.latest.clone(),
            latest_url: row.latest_url.clone(),
            available: row.available(),
            skipped: row.skipped.clone(),
            prereleases: row.prereleases,
            checked_at_ms: row.checked_at_ms,
            error: row
                .error
                .as_deref()
                .map(|error| crate::i18n::Text::load(error).render()),
        }
    }

    async fn followed(&self, entry_id: &str) -> Result<UpdateRow> {
        self.inner
            .store
            .game_update(entry_id)
            .await?
            .filter(|row| row.provider != NOT_FOLLOWED)
            .ok_or_else(|| Error::Invalid(crate::tr!("this install isn't followed for updates")))
    }

    /// Follows the managed installs tracking hasn't looked at: those from
    /// before updates were followed, and those a storage scan imported.
    /// Installs whose root is offline wait for the next call.
    pub async fn track_missing(&self) -> Result<usize> {
        let store = &self.inner.store;
        let mut followed = 0;
        for entry in store.untracked_managed_entries().await? {
            let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
                continue;
            };
            let Some(root) = store.get_root(root_id).await? else {
                continue;
            };
            let dir = root.path.join(rel_dir);
            let manifest = match tokio::task::spawn_blocking(move || InstallManifest::read(&dir))
                .await
            {
                Ok(Ok(manifest)) => manifest,
                Ok(Err(error)) => {
                    tracing::debug!(%error, entry_id = %entry.id, "install manifest unreadable");
                    continue;
                }
                Err(error) => return Err(Error::Invalid(error.to_string())),
            };
            if self
                .inner
                .tracker
                .track(&entry.id, &manifest, None)
                .await?
                .is_some()
            {
                followed += 1;
            }
        }
        Ok(followed)
    }

    /// Asks the install's provider for its newest release now. A failure is
    /// kept in the answer (`error`), and the last release found and the
    /// time of the last check that worked stay.
    pub async fn check(&self, entry_id: &str) -> Result<GameUpdate> {
        Ok(self.check_once(entry_id).await?.0)
    }

    /// [`Updates::check`], and the provider's error when it failed.
    async fn check_once(&self, entry_id: &str) -> Result<(GameUpdate, Option<Error>)> {
        let _checking = self.inner.checking.lock().await;
        let row = self.followed(entry_id).await?;
        let provider = self.inner.tracker.provider(&row.provider)?;
        let mut checked = row.clone();
        let mut failure = None;
        match provider.latest(&row.project, row.prereleases).await {
            Ok(release) => {
                self.retry_at().remove(entry_id);
                checked.checked_at_ms = now_ms();
                checked.error = None;
                checked.latest = release.as_ref().map(|release| release.version.clone());
                checked.latest_url = release.map(|release| release.url);
                // An install that didn't say which release it was counts as
                // the newest one when it is first checked.
                if checked.installed.is_none() {
                    checked.installed = checked.latest.clone();
                }
            }
            Err(error) => {
                tracing::debug!(%error, entry_id, project = %row.project, "update check failed");
                self.retry_at()
                    .insert(entry_id.to_owned(), Instant::now() + RETRY);
                // Kept as its message, translated when read (#214).
                checked.error = Some(crate::i18n::Text::of_error(&error).store());
                failure = Some(error);
            }
        }
        self.inner.store.save_update_check(&checked).await?;
        if (&checked.latest, &checked.installed, &checked.error)
            != (&row.latest, &row.installed, &row.error)
        {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: row.game_id.clone(),
            });
        }
        Ok((self.view(&checked), failure))
    }

    fn retry_at(&self) -> std::sync::MutexGuard<'_, HashMap<String, Instant>> {
        crate::lock(&self.inner.retry_at)
    }

    /// Whether `entry_id`'s last check failed too recently to try again.
    fn failed_recently(&self, entry_id: &str) -> bool {
        let now = Instant::now();
        let mut retry_at = self.retry_at();
        retry_at.retain(|_, at| *at > now);
        retry_at.contains_key(entry_id)
    }

    /// Checks every followed install whose last check is older than the
    /// settings' interval, one at a time, except those that failed in the
    /// last half hour. Stops when the provider can't be reached: the rest
    /// wait for the next round. How many were checked.
    pub async fn check_due(&self) -> Result<usize> {
        let settings = self.settings().await;
        if !settings.check {
            return Ok(0);
        }
        let interval = i64::from(settings.interval_hours) * 60 * 60 * 1000;
        let due = now_ms().saturating_sub(interval);
        let rows = self.inner.store.followed_updates().await?;
        let mut checked = 0;
        for row in rows.iter().filter(|row| row.checked_at_ms < due) {
            if self.failed_recently(&row.entry_id) {
                continue;
            }
            let (_, failure) = self.check_once(&row.entry_id).await?;
            checked += 1;
            if failure.as_ref().is_some_and(crate::error::unreachable) {
                break;
            }
        }
        Ok(checked)
    }

    /// The updates of `game_id`'s followed installs.
    pub async fn game(&self, game_id: &str) -> Result<Vec<GameUpdate>> {
        let rows = self.inner.store.game_updates(game_id).await?;
        Ok(rows.iter().map(|row| self.view(row)).collect())
    }

    /// The update to offer before `entry_id` is played, if any. A check
    /// older than the settings' interval (one the background watch hasn't
    /// got to yet) is redone first, for a few seconds at most, unless one
    /// failed in the last half hour; when it can't finish, the last answer
    /// stands. A newer one is the watch's answer, so Play doesn't wait on
    /// GitHub.
    pub async fn before_play(&self, entry_id: &str) -> Result<Option<GameUpdate>> {
        let settings = self.settings().await;
        if !settings.ask_before_play {
            return Ok(None);
        }
        let Some(row) = self
            .inner
            .store
            .game_update(entry_id)
            .await?
            .filter(|row| row.provider != NOT_FOLLOWED)
        else {
            return Ok(None);
        };
        let interval = i64::from(settings.interval_hours) * 60 * 60 * 1000;
        let stale = row.checked_at_ms < now_ms().saturating_sub(interval);
        let update = if settings.check && stale && !self.failed_recently(entry_id) {
            match tokio::time::timeout(PLAY_TIMEOUT, self.check(entry_id)).await {
                Ok(Ok(update)) => update,
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    // Too slow counts as failed, so the next Play doesn't wait.
                    self.retry_at()
                        .insert(entry_id.to_owned(), Instant::now() + RETRY);
                    self.view(&row)
                }
            }
        } else {
            self.view(&row)
        };
        Ok(update.available.then_some(update))
    }

    /// Stops offering release `version` (`None` offers every release
    /// again).
    pub async fn skip(&self, entry_id: &str, version: Option<String>) -> Result<GameUpdate> {
        let mut row = self.followed(entry_id).await?;
        row.skipped = version;
        self.inner.store.save_update_choices(&row).await?;
        self.inner.bus.emit(Event::LibraryGameUpdated {
            game_id: row.game_id.clone(),
        });
        Ok(self.view(&row))
    }

    /// Offers prereleases too, or stops, and checks again.
    pub async fn set_prereleases(&self, entry_id: &str, prereleases: bool) -> Result<GameUpdate> {
        let mut row = self.followed(entry_id).await?;
        row.prereleases = prereleases;
        self.inner.store.save_update_choices(&row).await?;
        self.check(entry_id).await
    }

    /// Installs the newest release over `entry_id`; the game must not be
    /// running. The returned job carries on in the background.
    pub async fn install(&self, entry_id: &str, launch: LaunchService) -> Result<JobSnapshot> {
        let row = self.followed(entry_id).await?;
        let latest = row
            .latest
            .clone()
            .filter(|_| row.latest != row.installed)
            .ok_or_else(|| Error::Invalid(crate::tr!("there is no newer release to install")))?;
        let provider = self.inner.tracker.provider(&row.provider)?;
        let url = provider.download(&row, &latest).await?;
        self.inner
            .install
            .update_game(entry_id.to_owned(), url, latest, launch)
            .await
    }

    /// Finishes updates an exit interrupted, follows new installs, and
    /// checks games when they're due, until the bus closes.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        use tokio::sync::broadcast::error::RecvError;
        let this = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            this.inner.install.recover_updates().await;
            let mut timer = tokio::time::interval(TICK);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = timer.tick() => {}
                    event = events.recv() => match event {
                        // A scan may have imported installs from another
                        // machine or an older Caskade.
                        Ok(Event::LibraryScanCompleted { .. }) | Err(RecvError::Lagged(_)) => {}
                        Ok(_) => continue,
                        Err(RecvError::Closed) => break,
                    },
                }
                if let Err(error) = this.track_missing().await {
                    tracing::warn!(%error, "following installs for updates failed");
                }
                match this.check_due().await {
                    Ok(0) => {}
                    Ok(checked) => tracing::info!(checked, "checked games for updates"),
                    Err(error) => tracing::warn!(%error, "update checks failed"),
                }
            }
        })
    }
}
