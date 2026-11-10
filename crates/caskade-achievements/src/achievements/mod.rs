//! Achievements (#185): each store account's achievements for the games it
//! plays, read from the services and cached. See `README.md` beside this
//! file.
//!
//! A set is one store account's achievements for one service game
//! (`achievement_sets`: account and product id). A game's sets are those
//! of the accounts the viewer can see whose service recognises one of the
//! game's releases: by the release's store ids (`ExternalId`), or by a
//! link (`achievement_links`: a verified ROM hash, or the user's choice).
//! Nothing is ever joined by title, and a game that merges or separates
//! takes exactly its releases' sets along.

mod db;
pub use db::{AchievementStore, Applied as AchievementsApplied};
mod model;
pub use caskade_core::bus::NewUnlocks;
pub use model::*;
mod provider;
/// For implementing [`AchievementProvider`] outside the core.
pub use async_trait::async_trait;
pub use provider::{
    AccountAccess, AchievementProvider, FetchError, Fetched, LinkOutcome, ReleaseLinker,
};
#[cfg(feature = "net")]
pub mod epic;
#[cfg(feature = "net")]
pub mod gog;
#[cfg(feature = "net")]
mod net;
#[cfg(feature = "net")]
pub mod retroachievements;
#[cfg(feature = "net")]
pub mod steam;
pub use tokio_util::sync::CancellationToken;
#[cfg(test)]
mod tests;

use caskade_core::{
    Result,
    bus::{Bus, Event},
    db::LibraryStore,
    error::ErrorReport,
    model::GameEntry,
    store_accounts::{ProviderError, StoreAccount},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

pub const NAMESPACE: &str = "core.achievements";
/// How long a service's "wait" lasts when it doesn't say.
const DEFAULT_WAIT: Duration = Duration::from_secs(15 * 60);
/// Background refreshes: games played in the last fortnight, sets older
/// than this, at most this many per round.
const BACKGROUND_AGE_MS: i64 = 6 * 60 * 60 * 1000;
const BACKGROUND_WINDOW_MS: i64 = 14 * 24 * 60 * 60 * 1000;
const BACKGROUND_SETS: usize = 10;
const BACKGROUND_TICK: Duration = Duration::from_secs(60 * 60);
/// A refresh after a game exits waits this long, for the service to catch up.
const AFTER_EXIT: Duration = Duration::from_secs(15);
/// An unresolved link is tried again after this long.
const RELINK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Settings → Achievements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AchievementSettings {
    #[schemars(title = "Show achievements")]
    /// Read achievements from connected accounts.
    pub enabled: bool,
    #[schemars(title = "Refresh in the background")]
    /// After a game exits, and now and then for games played lately.
    pub background: bool,
    #[schemars(title = "Notify new unlocks")]
    /// A notification for each achievement unlocked since the last refresh.
    pub notify_unlocks: bool,
}

impl Default for AchievementSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            background: true,
            notify_unlocks: true,
        }
    }
}

/// Reads and caches achievements.
#[derive(Clone)]
pub struct Achievements {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    accounts: Arc<dyn AccountAccess>,
    providers: RwLock<BTreeMap<String, Arc<dyn AchievementProvider>>>,
    linkers: RwLock<Vec<Arc<dyn ReleaseLinker>>>,
    /// One fetch per set at a time.
    flights: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Refreshes asked for by a game's page, by game: what Stop cancels.
    asked: Mutex<HashMap<String, CancellationToken>>,
    settings: std::sync::OnceLock<caskade_core::settings::SettingsService>,
}

/// What fetching one set did.
enum Fetch {
    Done,
    /// Not asked: the service's wait, or no sign-in.
    Skipped,
    Failed(FetchError),
}

/// An account refresh stops at these: the next sets would fail the same.
fn stops(error: &ProviderError) -> bool {
    matches!(
        error,
        ProviderError::Offline(_)
            | ProviderError::Expired(_)
            | ProviderError::Rejected(_)
            | ProviderError::RateLimited { .. }
            | ProviderError::Cancelled
    )
}

/// [`Achievements::account_refresher`]: does nothing once the service is
/// gone, so the store accounts don't keep it alive.
struct Refresher(std::sync::Weak<Inner>);

#[async_trait::async_trait]
impl caskade_core::store_accounts::AccountRefresher for Refresher {
    async fn refresh_account(
        &self,
        account: &StoreAccount,
        cancel: &CancellationToken,
    ) -> std::result::Result<(), ProviderError> {
        match self.0.upgrade() {
            Some(inner) => {
                Achievements { inner }
                    .refresh_account(account, cancel)
                    .await
            }
            None => Ok(()),
        }
    }
}

/// A set a game has: the account, the product and the releases.
#[derive(Debug, Clone)]
struct Candidate {
    account: StoreAccount,
    product: String,
    entries: Vec<String>,
}

impl Achievements {
    pub fn new(store: LibraryStore, bus: Bus, accounts: Arc<dyn AccountAccess>) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                accounts,
                providers: RwLock::default(),
                linkers: RwLock::default(),
                flights: Mutex::default(),
                asked: Mutex::default(),
                settings: std::sync::OnceLock::new(),
            }),
        }
    }

    pub fn register_provider(&self, provider: Arc<dyn AchievementProvider>) {
        caskade_core::write(&self.inner.providers).insert(provider.id(), provider);
    }

    /// Links releases to `linker`'s service before their sets are fetched.
    pub fn register_linker(&self, linker: Arc<dyn ReleaseLinker>) {
        caskade_core::write(&self.inner.linkers).push(linker);
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn AchievementProvider>> {
        caskade_core::read(&self.inner.providers).get(id).cloned()
    }

    /// Reads `core.achievements` from `settings`; registers it first.
    pub fn set_settings(&self, settings: caskade_core::settings::SettingsService) {
        if let Err(error) = settings.register(
            caskade_core::settings::SectionDefinition::typed::<AchievementSettings>(NAMESPACE, 1)
                .expect("the achievement settings schema is valid"),
        ) {
            tracing::error!(%error, "achievement settings unavailable");
        }
        let _ = self.inner.settings.set(settings);
    }

    pub async fn settings(&self) -> AchievementSettings {
        let Some(settings) = self.inner.settings.get() else {
            return AchievementSettings::default();
        };
        settings.read_or_default(NAMESPACE).await
    }

    /// The sets `game_id` has for `viewer` (a local account; `None`: every
    /// account's), with their achievements as last fetched.
    pub async fn sets(&self, game_id: &str, viewer: Option<&str>) -> Result<Vec<AchievementSet>> {
        let mut sets = vec![];
        for candidate in self.candidates(game_id, viewer).await? {
            sets.push(self.view(&candidate).await?);
        }
        Ok(sets)
    }

    /// Counts for each game, as cards and lists show them.
    pub async fn summary(&self, game_id: &str, viewer: Option<&str>) -> Result<AchievementSummary> {
        let sets = self.sets(game_id, viewer).await?;
        Ok(AchievementSummary {
            game_id: game_id.into(),
            sets: sets.len() as u32,
            unlocked: sets.iter().map(|set| set.unlocked).sum(),
            total: sets.iter().map(|set| set.total).sum(),
        })
    }

    /// Fetches every set of `game_id` that `viewer` sees, then answers them.
    /// One that can't be fetched keeps what was known and says why.
    pub async fn refresh_game(
        &self,
        game_id: &str,
        viewer: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<AchievementSet>> {
        self.link_releases(game_id, viewer, cancel).await?;
        let mut sets = vec![];
        for candidate in self.candidates(game_id, viewer).await? {
            if cancel.is_cancelled() {
                break;
            }
            self.refresh(game_id, &candidate, cancel).await?;
            sets.push(self.view(&candidate).await?);
        }
        Ok(sets)
    }

    /// [`Self::refresh_game`] that [`Self::cancel_refresh`] can stop.
    pub async fn refresh_game_now(
        &self,
        game_id: &str,
        viewer: Option<&str>,
    ) -> Result<Vec<AchievementSet>> {
        let cancel = CancellationToken::new();
        caskade_core::lock(&self.inner.asked).insert(game_id.into(), cancel.clone());
        let result = self.refresh_game(game_id, viewer, &cancel).await;
        caskade_core::lock(&self.inner.asked).remove(game_id);
        result
    }

    /// Stops `game_id`'s refresh under way; false when there is none.
    pub fn cancel_refresh(&self, game_id: &str) -> bool {
        match caskade_core::lock(&self.inner.asked).get(game_id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Links the library's releases to `account`'s service, then fetches
    /// the account's set of each game that has one: Refresh for an account
    /// without a library (RetroAchievements). Stops at the service's first
    /// wait, or once it can't be reached or refuses the sign-in; fails when
    /// a set failed and none came back.
    pub async fn refresh_account(
        &self,
        account: &StoreAccount,
        cancel: &CancellationToken,
    ) -> std::result::Result<(), ProviderError> {
        let other = |error: caskade_core::Error| ProviderError::Other(error.to_string());
        self.inner.accounts.credentials(account).await?;
        let mut fetched = false;
        let mut failed = None;
        for game_id in self.inner.store.games_with_entries().await.map_err(other)? {
            let linking = self
                .link_releases(&game_id, None, cancel)
                .await
                .map_err(other)?;
            if let Some(FetchError::Provider(error)) = linking
                && stops(&error)
            {
                return Err(error);
            }
            for candidate in self.candidates(&game_id, None).await.map_err(other)? {
                if candidate.account.id != account.id {
                    continue;
                }
                match self
                    .refresh(&game_id, &candidate, cancel)
                    .await
                    .map_err(other)?
                {
                    Fetch::Done => fetched = true,
                    Fetch::Skipped => {}
                    Fetch::Failed(FetchError::Provider(error)) if stops(&error) => {
                        return Err(error);
                    }
                    Fetch::Failed(error) => failed = Some(error),
                }
            }
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
        }
        match failed {
            Some(FetchError::Provider(error)) if !fetched => Err(error),
            _ => Ok(()),
        }
    }

    /// What [`StoreAccounts`](caskade_core::store_accounts::StoreAccounts) runs to
    /// refresh an account without a library; it holds the service weakly.
    pub fn account_refresher(&self) -> Arc<dyn caskade_core::store_accounts::AccountRefresher> {
        Arc::new(Refresher(Arc::downgrade(&self.inner)))
    }

    /// Links `game_id`'s releases that a linker applies to and that have no
    /// link yet, or an unresolved one older than a week, with the first
    /// connected account of its service `viewer` sees. A link the user set
    /// is never replaced. A failure (offline, signed out) links nothing now
    /// and is answered.
    async fn link_releases(
        &self,
        game_id: &str,
        viewer: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Option<FetchError>> {
        let linkers = caskade_core::read(&self.inner.linkers).clone();
        if linkers.is_empty() {
            return Ok(None);
        }
        let store = &self.inner.store;
        let entries = store.entries_for_game(game_id).await?;
        let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
        let links = store.achievement_links(&ids).await?;
        let accounts = self.inner.accounts.accounts().await?;
        let now = caskade_core::now_ms();
        for linker in linkers {
            let provider = linker.provider();
            let Some(account) = accounts.iter().find(|account| {
                account.provider == provider
                    && account.enabled
                    && viewer.is_none_or(|viewer| {
                        account
                            .owner_id
                            .as_deref()
                            .is_none_or(|owner| owner == viewer)
                    })
            }) else {
                continue;
            };
            for entry in entries.iter().filter(|entry| linker.applies(entry)) {
                let current = links
                    .iter()
                    .find(|link| link.entry_id == entry.id && link.provider == provider);
                let due = match current {
                    None => true,
                    Some(link) => {
                        matches!(link.method, LinkMethod::Unresolved { .. })
                            && link.linked_at_ms + RELINK_MS <= now
                    }
                };
                if !due || cancel.is_cancelled() {
                    continue;
                }
                let credentials = match self.inner.accounts.credentials(account).await {
                    Ok(credentials) => credentials,
                    Err(error) => return Ok(Some(error.into())),
                };
                let outcome = match linker
                    .link(store, entry, account, credentials.as_ref(), cancel)
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        tracing::info!(%error, "release not linked now");
                        return Ok(Some(error));
                    }
                };
                let (product_id, method) = match outcome {
                    LinkOutcome::Linked { product, hash } => (product, LinkMethod::Hash { hash }),
                    LinkOutcome::Unresolved(reason) => {
                        (String::new(), LinkMethod::Unresolved { reason })
                    }
                };
                store
                    .save_achievement_link(&AchievementLink {
                        entry_id: entry.id.clone(),
                        provider: provider.clone(),
                        product_id,
                        method,
                        linked_at_ms: now,
                    })
                    .await?;
            }
        }
        Ok(None)
    }

    /// Links of `game_id`'s releases, verified, set by hand or unresolved
    /// with why: what the game page shows and lets the user change.
    pub async fn links(&self, game_id: &str) -> Result<Vec<AchievementLink>> {
        let store = &self.inner.store;
        let ids: Vec<String> = store
            .entries_for_game(game_id)
            .await?
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        store.achievement_links(&ids).await
    }

    /// Which accounts' sets `game_id` has, by its releases' identities.
    async fn candidates(&self, game_id: &str, viewer: Option<&str>) -> Result<Vec<Candidate>> {
        let entries = self.inner.store.entries_for_game(game_id).await?;
        let entry_ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
        let links = self.inner.store.achievement_links(&entry_ids).await?;
        let accounts = self.inner.accounts.accounts().await?;
        let mut found: BTreeMap<(String, String), Candidate> = BTreeMap::new();
        for account in accounts {
            if viewer.is_some_and(|viewer| {
                account
                    .owner_id
                    .as_deref()
                    .is_some_and(|owner| owner != viewer)
            }) {
                continue;
            }
            let Some(provider) = self.provider(&account.provider) else {
                continue;
            };
            for entry in &entries {
                for product in products(provider.as_ref(), entry, &links) {
                    found
                        .entry((account.id.clone(), product.clone()))
                        .or_insert_with(|| Candidate {
                            account: account.clone(),
                            product,
                            entries: vec![],
                        })
                        .entries
                        .push(entry.id.clone());
                }
            }
        }
        Ok(found.into_values().collect())
    }

    async fn view(&self, candidate: &Candidate) -> Result<AchievementSet> {
        let store = &self.inner.store;
        let provider = self.provider(&candidate.account.provider);
        let (id, sync, achievements) = match store
            .achievement_set(&candidate.account.id, &candidate.product)
            .await?
        {
            Some((id, sync)) => {
                let achievements = store.achievements(&id).await?;
                (Some(id), sync, achievements)
            }
            None => (None, SetSync::default(), vec![]),
        };
        let mut sync = sync;
        // A disconnected account's set says so, whatever it last fetched.
        if !candidate.account.enabled {
            sync.status = SetStatus::SignInNeeded;
        }
        let capabilities = provider
            .as_ref()
            .map(|provider| provider.capabilities())
            .unwrap_or_default();
        Ok(AchievementSet {
            id,
            account: SetAccount {
                store_account_id: candidate.account.id.clone(),
                provider: candidate.account.provider.clone(),
                provider_name: self
                    .inner
                    .accounts
                    .provider_name(&candidate.account.provider)
                    .unwrap_or_else(|| candidate.account.provider.clone()),
                display_name: candidate.account.display_name.clone(),
            },
            product_id: candidate.product.clone(),
            entry_ids: candidate.entries.clone(),
            unlocked: achievements.iter().filter(|a| a.state.unlocked).count() as u32,
            total: achievements.len() as u32,
            hardcore_unlocked: capabilities.hardcore.then(|| {
                achievements
                    .iter()
                    .filter(|a| a.state.hardcore.as_ref().is_some_and(|h| h.unlocked))
                    .count() as u32
            }),
            capabilities,
            sync,
            achievements,
        })
    }

    /// Fetches one set, recording what came back.
    async fn refresh(
        &self,
        game_id: &str,
        candidate: &Candidate,
        cancel: &CancellationToken,
    ) -> Result<Fetch> {
        let store = &self.inner.store;
        let set_id = store
            .ensure_achievement_set(&candidate.account.id, &candidate.product)
            .await?;
        let flight = Arc::clone(
            caskade_core::lock(&self.inner.flights)
                .entry(set_id.clone())
                .or_default(),
        );
        let _one = flight.lock().await;
        let (_, mut sync) = store
            .achievement_set(&candidate.account.id, &candidate.product)
            .await?
            .unwrap_or_default();
        let now = caskade_core::now_ms();
        // The service's wait, for this set or for the whole account.
        if sync.retry_at_ms.is_some_and(|at| at > now)
            || candidate
                .account
                .sync
                .retry_at_ms
                .is_some_and(|at| at > now)
        {
            return Ok(Fetch::Skipped);
        }
        let Some(provider) = self.provider(&candidate.account.provider) else {
            return Ok(Fetch::Skipped);
        };
        if !candidate.account.enabled {
            sync.status = SetStatus::SignInNeeded;
            store.save_achievement_sync(&set_id, &sync).await?;
            return Ok(Fetch::Skipped);
        }
        sync.last_attempt_ms = Some(now);
        let fetched = match self.inner.accounts.credentials(&candidate.account).await {
            Ok(credentials) => {
                provider
                    .fetch(
                        &candidate.account,
                        credentials.as_ref(),
                        &candidate.product,
                        cancel,
                    )
                    .await
            }
            Err(error) => Err(FetchError::Provider(error)),
        };
        let mut unlocked = vec![];
        let mut failure = None;
        match fetched {
            Ok(fetched) => {
                let applied = store
                    .apply_achievements(
                        &set_id,
                        &fetched.definitions,
                        &fetched.states,
                        fetched.complete,
                    )
                    .await?;
                let had_baseline = sync.baseline;
                sync.status = if fetched.complete && fetched.definitions.is_empty() {
                    SetStatus::NoAchievements
                } else {
                    SetStatus::Ready
                };
                sync.fetched_at_ms = Some(now);
                sync.failures = 0;
                sync.retry_at_ms = None;
                sync.error = (!fetched.complete).then(|| {
                    ErrorReport::from(caskade_core::tr!(
                        "Not every achievement came back; the rest show as last fetched."
                    ))
                });
                if fetched.complete {
                    sync.baseline = true;
                }
                if had_baseline {
                    unlocked = applied.newly_unlocked;
                }
            }
            Err(error @ FetchError::Provider(ProviderError::Cancelled)) => {
                return Ok(Fetch::Failed(error));
            }
            Err(error) => {
                sync.failures = sync.failures.saturating_add(1);
                match &error {
                    FetchError::Unsupported(_) => sync.status = SetStatus::Unsupported,
                    FetchError::Provider(ProviderError::Unavailable(_)) => {
                        sync.status = SetStatus::Private
                    }
                    FetchError::Provider(
                        ProviderError::Expired(_) | ProviderError::Rejected(_),
                    ) => sync.status = SetStatus::SignInNeeded,
                    FetchError::Provider(ProviderError::RateLimited { retry_after }) => {
                        sync.retry_at_ms = Some(
                            now + retry_after
                                .unwrap_or(DEFAULT_WAIT)
                                .as_millis()
                                .min(i64::MAX as u128) as i64,
                        );
                    }
                    // Offline and the rest: what was known stays, marked stale.
                    _ => {}
                }
                sync.error = Some(ErrorReport::from_error(&error));
                failure = Some(error);
            }
        }
        store.save_achievement_sync(&set_id, &sync).await?;
        self.inner.bus.emit(Event::AchievementsChanged {
            game_id: game_id.into(),
        });
        if !unlocked.is_empty() && self.settings().await.notify_unlocks {
            let achievements = store.achievements(&set_id).await?;
            let title = store
                .get_game(game_id)
                .await?
                .map(|details| details.game.title)
                .unwrap_or_default();
            self.inner
                .bus
                .emit(Event::AchievementsUnlocked(Box::new(NewUnlocks {
                    game_id: game_id.into(),
                    game_title: title,
                    set_id,
                    provider_name: self
                        .inner
                        .accounts
                        .provider_name(&candidate.account.provider)
                        .unwrap_or_else(|| candidate.account.provider.clone()),
                    account_name: candidate.account.display_name.clone(),
                    names: achievements
                        .iter()
                        .filter(|a| unlocked.contains(&a.definition.id))
                        .map(|a| a.definition.name.clone())
                        .collect(),
                })));
        }
        Ok(failure.map_or(Fetch::Done, Fetch::Failed))
    }

    /// Refreshes in the background while `core.achievements` allows: a
    /// game's sets shortly after it exits, and each hour the sets of games
    /// played in the last fortnight not fetched for six hours, a few at a
    /// time. A service's wait is always kept.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(BACKGROUND_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Ok(Event::GameExited(session)) => {
                            let service = service.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(AFTER_EXIT).await;
                                service.background_game(&session.game_id).await;
                            });
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => return,
                    },
                    _ = tick.tick() => service.background_round().await,
                }
            }
        })
    }

    async fn background_game(&self, game_id: &str) {
        let settings = self.settings().await;
        if !settings.enabled || !settings.background {
            return;
        }
        if let Err(error) = self
            .refresh_game(game_id, None, &CancellationToken::new())
            .await
        {
            tracing::warn!(%error, "background achievement refresh failed");
        }
    }

    async fn background_round(&self) {
        let settings = self.settings().await;
        if !settings.enabled || !settings.background {
            return;
        }
        let now = caskade_core::now_ms();
        let games = match self
            .inner
            .store
            .games_played_since(now - BACKGROUND_WINDOW_MS, 50)
            .await
        {
            Ok(games) => games,
            Err(error) => {
                tracing::warn!(%error, "background achievement refresh failed");
                return;
            }
        };
        let mut budget = BACKGROUND_SETS;
        for game in games {
            let Ok(candidates) = self.candidates(&game, None).await else {
                continue;
            };
            for candidate in candidates {
                if budget == 0 {
                    return;
                }
                let due = match self
                    .inner
                    .store
                    .achievement_set(&candidate.account.id, &candidate.product)
                    .await
                {
                    Ok(Some((_, sync))) => {
                        sync.last_attempt_ms
                            .is_none_or(|at| at + BACKGROUND_AGE_MS <= now)
                            && sync.retry_at_ms.is_none_or(|at| at <= now)
                    }
                    Ok(None) => true,
                    Err(_) => false,
                };
                if due && candidate.account.enabled {
                    budget -= 1;
                    let _ = self
                        .refresh(&game, &candidate, &CancellationToken::new())
                        .await;
                }
            }
        }
    }
}

/// The service game ids a provider knows `entry` by: its store ids, and the
/// links set for it.
fn products(
    provider: &dyn AchievementProvider,
    entry: &GameEntry,
    links: &[AchievementLink],
) -> Vec<String> {
    let provider_id = provider.id();
    let mut products: Vec<String> = entry
        .ids
        .iter()
        .filter_map(|id| provider.product(id))
        .collect();
    products.extend(
        links
            .iter()
            .filter(|link| {
                link.entry_id == entry.id
                    && link.provider == provider_id
                    && !matches!(link.method, LinkMethod::Unresolved { .. })
            })
            .map(|link| link.product_id.clone()),
    );
    products.sort();
    products.dedup();
    products
}

/// The app's store accounts: their sign-ins come from the keychain,
/// renewed when about to expire (#165).
#[async_trait::async_trait]
impl AccountAccess for caskade_core::store_accounts::StoreAccounts {
    async fn accounts(&self) -> Result<Vec<StoreAccount>> {
        Ok(caskade_core::store_accounts::StoreAccounts::accounts(self)
            .await?
            .into_iter()
            .map(|summary| summary.account)
            .collect())
    }
    async fn credentials(
        &self,
        account: &StoreAccount,
    ) -> std::result::Result<Option<caskade_core::store_accounts::StoreCredentials>, ProviderError>
    {
        caskade_core::store_accounts::StoreAccounts::credentials(self, &account.id).await
    }
    fn provider_name(&self, provider: &str) -> Option<String> {
        self.providers()
            .into_iter()
            .find(|capabilities| capabilities.id == provider)
            .map(|capabilities| capabilities.name)
    }
}
