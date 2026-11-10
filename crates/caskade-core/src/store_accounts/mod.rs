//! Store accounts and the games they own (#164), separate from what is
//! installed. See `README.md` beside this file.
//!
//! An [`AccountProvider`] lists what an account owns. A refresh records
//! each product as an entitlement of that account, and shows it in the
//! library as a game with a release that isn't installed (an entry from
//! source `store:<account id>`, with no launch). When discovery finds an
//! installed copy with the same store id, before or after the refresh,
//! the two are one game ([`StoreAccounts::reconcile`]).

mod model;
pub use model::*;
#[cfg(feature = "net")]
pub mod epic;
#[cfg(feature = "net")]
pub mod gog;
#[cfg(feature = "net")]
pub mod http;
mod schedule;
#[cfg(feature = "net")]
pub mod steam;
pub use schedule::{NAMESPACE, StoreAccountSettings, register_settings};
mod install;
pub use install::{
    InstallTarget, Opener, OwnedInstallState, OwnedRelease, UnavailableReason, system_opener,
    this_system,
};
mod secrets;
pub use secrets::{KEYCHAIN_SERVICE, StoreKeychain};
#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod tests;

use crate::{
    Error, Result,
    bus::{Bus, Event},
    db::LibraryStore,
    error::ErrorReport,
};
/// For implementing [`AccountProvider`] outside the core.
pub use async_trait::async_trait;
use caskade_discovery::SourceId;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Library entries of store accounts have source ids starting with this.
pub const SOURCE_PREFIX: &str = "store:";

/// The library source of `account_id`'s entries.
pub fn source_id(account_id: &str) -> SourceId {
    SourceId::from(format!("{SOURCE_PREFIX}{account_id}"))
}

/// Lists what a store account owns. Built-in stores and plugins provide
/// them; tests use fakes.
#[async_trait]
pub trait AccountProvider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;
    /// Starts a sign-in. `flow_id` is the `state` a browser callback must
    /// carry back.
    async fn begin_auth(&self, flow_id: &str) -> std::result::Result<AuthChallenge, ProviderError>;
    /// Finishes a sign-in: who signed in, and their sign-in.
    /// [`ProviderError::Pending`] while a device sign-in isn't done.
    async fn complete_auth(
        &self,
        flow_id: &str,
        input: &AuthInput,
    ) -> std::result::Result<SignedIn, ProviderError>;
    /// A fresh sign-in from an expired one's refresh token.
    async fn refresh_credentials(
        &self,
        _credentials: &StoreCredentials,
    ) -> std::result::Result<StoreCredentials, ProviderError> {
        Err(ProviderError::Expired(crate::tr!("it can't be renewed")))
    }
    /// What installs `product` here: its launcher and the URI that opens
    /// the install (#170). `None`: the store can't install from Caskade.
    fn install_target(&self, _product: &OwnedProduct) -> Option<InstallTarget> {
        None
    }
    /// Revokes a sign-in at the store, where the store supports it.
    async fn revoke(
        &self,
        _credentials: &StoreCredentials,
    ) -> std::result::Result<(), ProviderError> {
        Ok(())
    }
    /// Every product `account` owns. An error, or a snapshot that isn't
    /// complete (a page that failed, a timeout), leaves what the library
    /// knows as it was. `credentials` is the account's sign-in, if any.
    async fn library(
        &self,
        account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        cancel: &CancellationToken,
    ) -> std::result::Result<LibrarySnapshot, ProviderError>;
}

/// Refreshes an account whose provider has no library (achievements only,
/// #189): the achievements service, in the app.
#[async_trait]
pub trait AccountRefresher: Send + Sync {
    async fn refresh_account(
        &self,
        account: &StoreAccount,
        cancel: &CancellationToken,
    ) -> std::result::Result<(), ProviderError>;
}

/// Why a provider couldn't list a library. The text is for people.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    /// The store can't be reached.
    Offline(String),
    /// The sign-in expired or was revoked: connect again.
    Expired(String),
    /// Too many requests: wait, `retry_after` when the store said.
    RateLimited {
        retry_after: Option<Duration>,
    },
    Cancelled,
    /// A device sign-in the user hasn't finished yet.
    Pending,
    /// The store refused the sign-in (a wrong key or code).
    Rejected(String),
    /// The store won't show the library: a private profile, a region or
    /// account restriction. The text says what to change.
    Unavailable(String),
    Other(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Offline(why) => crate::tr!("the store can't be reached: {why}", why = why),
            Self::Expired(why) => crate::tr!(
                "the sign-in has expired; connect the account again ({why})",
                why = why
            ),
            Self::RateLimited { .. } => {
                crate::tr!("the store asked Caskade to wait before asking again")
            }
            Self::Cancelled => crate::tr!("the refresh was cancelled"),
            Self::Pending => crate::tr!("the sign-in isn't finished yet"),
            Self::Rejected(why) => crate::tr!("the store refused the sign-in: {why}", why = why),
            Self::Unavailable(why) => why.clone(),
            Self::Other(why) => why.clone(),
        })
    }
}

impl std::error::Error for ProviderError {}

/// Why a refresh failed: the provider's answer, or the library's.
#[derive(Debug, Clone)]
pub enum RefreshError {
    Provider(ProviderError),
    Store(Arc<Error>),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(error) => error.fmt(f),
            Self::Store(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RefreshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Provider(error) => Some(error),
            Self::Store(error) => Some(&**error),
        }
    }
}

impl From<Error> for RefreshError {
    fn from(error: Error) -> Self {
        Self::Store(Arc::new(error))
    }
}

/// Why a sign-in didn't finish. The text is for people.
#[derive(Debug)]
pub enum ConnectError {
    /// The flow is unknown, cancelled or older than ten minutes.
    Expired,
    /// A callback that doesn't belong to the flow.
    Mismatch,
    /// Reconnecting one account, the user signed in as another.
    OtherAccount {
        signed_in: String,
        expected: String,
    },
    Provider(ProviderError),
    Store(Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Expired => crate::tr!("This sign-in has expired or was cancelled. Start again."),
            Self::Mismatch => {
                crate::tr!(
                    "The sign-in answer didn't match the sign-in Caskade started. Start again."
                )
            }
            Self::OtherAccount {
                signed_in,
                expected,
            } => crate::tr!(
                "You signed in as {signed_in}, not {expected}. Nothing changed.",
                signed_in = signed_in,
                expected = expected
            ),
            Self::Provider(error) => error.to_string(),
            Self::Store(error) => error.to_string(),
        })
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Provider(error) => Some(error),
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<Error> for ConnectError {
    fn from(error: Error) -> Self {
        Self::Store(error)
    }
}

/// Store accounts, their providers and their libraries. Clones share
/// everything.
#[derive(Clone)]
pub struct StoreAccounts {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    bus: Bus,
    providers: RwLock<BTreeMap<String, Arc<dyn AccountProvider>>>,
    /// Per account: the lock its refreshes take, and how many finished.
    flights: Mutex<HashMap<String, Flight>>,
    changes: tokio::sync::watch::Sender<u64>,
    /// Merges of games, one at a time.
    reconciling: tokio::sync::Mutex<()>,
    credentials: secrets::Credentials,
    /// Sign-ins under way, by flow id.
    flows: Mutex<HashMap<String, AuthFlow>>,
    settings: std::sync::OnceLock<crate::settings::SettingsService>,
    /// What Refresh runs for accounts without a library.
    refresher: std::sync::OnceLock<Arc<dyn AccountRefresher>>,
    /// Releases handed to their launcher, by entry, since when.
    installing: Mutex<HashMap<String, i64>>,
    opener: RwLock<Opener>,
}

/// How long a sign-in may take.
const FLOW_TTL_MS: i64 = 10 * 60 * 1000;

/// A finished refresh's outcome.
type Refreshed = std::result::Result<RefreshOutcome, RefreshError>;

#[derive(Clone, Default)]
struct Flight {
    lock: Arc<tokio::sync::Mutex<()>>,
    /// Cancels the refresh under way.
    running: Arc<Mutex<Option<CancellationToken>>>,
    finished: Arc<Mutex<(u64, Option<Refreshed>)>>,
}

impl StoreAccounts {
    /// Sign-ins in memory only: tests, and sessions without a keychain.
    pub fn new(store: LibraryStore, bus: Bus) -> Self {
        Self::with_secrets(
            store,
            bus,
            Arc::new(crate::romm::secrets::MemorySecrets::default()),
        )
    }

    /// Sign-ins in `secrets` ([`StoreKeychain`] in the app); in memory for
    /// the session when it fails.
    pub fn with_secrets(
        store: LibraryStore,
        bus: Bus,
        secrets: Arc<dyn crate::romm::secrets::SecretStore>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                bus,
                providers: RwLock::default(),
                flights: Mutex::default(),
                changes: tokio::sync::watch::channel(0).0,
                reconciling: tokio::sync::Mutex::new(()),
                credentials: secrets::Credentials::new(secrets),
                flows: Mutex::default(),
                settings: std::sync::OnceLock::new(),
                refresher: std::sync::OnceLock::new(),
                installing: Mutex::default(),
                opener: RwLock::new(system_opener()),
            }),
        }
    }

    pub fn register_provider(&self, provider: Arc<dyn AccountProvider>) {
        let id = provider.capabilities().id;
        crate::write(&self.inner.providers).insert(id, provider);
        self.changed();
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn AccountProvider>> {
        crate::read(&self.inner.providers).get(id).cloned()
    }

    /// Sets what Refresh runs for accounts whose provider has no library.
    pub fn set_refresher(&self, refresher: Arc<dyn AccountRefresher>) {
        let _ = self.inner.refresher.set(refresher);
    }

    /// The providers available this session.
    pub fn providers(&self) -> Vec<ProviderCapabilities> {
        crate::read(&self.inner.providers)
            .values()
            .map(|provider| provider.capabilities())
            .collect()
    }

    /// Bumps on every change to an account or what it owns.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.changes.subscribe()
    }

    fn changed(&self) {
        self.inner.changes.send_modify(|revision| *revision += 1);
    }

    pub async fn accounts(&self) -> Result<Vec<StoreAccountSummary>> {
        let mut out = Vec::new();
        for account in self.inner.store.store_accounts().await? {
            let (owned, lost, installed) = self.inner.store.entitlement_counts(&account.id).await?;
            let capabilities = self.provider(&account.provider).map(|p| p.capabilities());
            let state = self.state(&account, capabilities.as_ref()).await;
            out.push(StoreAccountSummary {
                session_only: self.inner.credentials.session_only(&account.id),
                capabilities,
                account,
                owned,
                lost,
                installed,
                state,
            });
        }
        Ok(out)
    }

    async fn state(
        &self,
        account: &StoreAccount,
        capabilities: Option<&ProviderCapabilities>,
    ) -> StoreConnectionState {
        use crate::error::ErrorCode;
        let Some(capabilities) = capabilities else {
            return StoreConnectionState::Unsupported;
        };
        if crate::lock(&self.inner.flows)
            .values()
            .any(|flow| flow.reconnect.as_deref() == Some(account.id.as_str()))
        {
            return StoreConnectionState::Authenticating;
        }
        if !account.enabled {
            return StoreConnectionState::NotConnected;
        }
        if self.flight(&account.id).lock.try_lock().is_err() {
            return StoreConnectionState::Syncing;
        }
        let code = account.sync.error.as_ref().map(|error| error.code);
        if code == Some(ErrorCode::Auth)
            || (capabilities.auth != AuthKind::Launcher
                && self.inner.credentials.get(&account.id).await.is_none())
        {
            StoreConnectionState::Expired
        } else if code == Some(ErrorCode::Offline) {
            StoreConnectionState::Offline
        } else {
            StoreConnectionState::Connected
        }
    }

    pub async fn account(&self, id: &str) -> Result<StoreAccount> {
        self.inner
            .store
            .store_account(id)
            .await?
            .ok_or_else(|| crate::db::missing("store account", id))
    }

    /// Adds the account `external_id` of `provider`, or turns it back on:
    /// the same account at a provider is always the same record, so what it
    /// owned is never duplicated, and never mixed with another's.
    pub async fn connect(
        &self,
        provider: &str,
        external_id: &str,
        display_name: &str,
        owner_id: Option<String>,
    ) -> Result<StoreAccount> {
        if self.provider(provider).is_none() {
            return Err(Error::Invalid(crate::tr!(
                "Caskade can't connect {provider} accounts",
                provider = provider
            )));
        }
        let account = match self
            .inner
            .store
            .store_account_by_external(provider, external_id)
            .await?
        {
            Some(mut account) => {
                account.enabled = true;
                account.display_name = display_name.to_owned();
                account
            }
            None => StoreAccount {
                id: crate::model::new_id(),
                provider: provider.to_owned(),
                external_id: external_id.to_owned(),
                display_name: display_name.to_owned(),
                owner_id,
                enabled: true,
                sync: SyncState::default(),
                created_at_ms: crate::now_ms(),
            },
        };
        self.inner.store.save_store_account(&account).await?;
        self.changed();
        Ok(account)
    }

    /// Stops refreshing the account. Its games and what it owned stay in
    /// the library; [`Self::remove`] takes them out.
    pub async fn disconnect(&self, id: &str) -> Result<StoreAccount> {
        let mut account = self.account(id).await?;
        self.cancel_refresh(id);
        self.forget_sign_in(&account).await?;
        account.enabled = false;
        self.inner.store.save_store_account(&account).await?;
        self.changed();
        Ok(account)
    }

    /// Revokes the sign-in at the store where it can, then deletes it here.
    async fn forget_sign_in(&self, account: &StoreAccount) -> Result<()> {
        if let Some(credentials) = self.inner.credentials.get(&account.id).await
            && let Some(provider) = self.provider(&account.provider)
            && let Err(error) = provider.revoke(&credentials).await
        {
            tracing::warn!(account = %account.id, %error, "the store sign-in wasn't revoked");
        }
        self.inner.credentials.delete(&account.id).await
    }

    fn flight(&self, id: &str) -> Flight {
        crate::lock(&self.inner.flights)
            .entry(id.to_owned())
            .or_default()
            .clone()
    }

    /// Refreshes what `id` owns. One refresh per account runs at a time; a
    /// second asked for meanwhile gets the first one's outcome.
    pub async fn refresh(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> std::result::Result<RefreshOutcome, RefreshError> {
        let flight = self.flight(id);
        let started = crate::lock(&flight.finished).0;
        let _running = flight.lock.lock().await;
        {
            let finished = crate::lock(&flight.finished);
            if finished.0 != started
                && let Some(result) = &finished.1
            {
                return result.clone();
            }
        }
        let token = cancel.child_token();
        *crate::lock(&flight.running) = Some(token.clone());
        let result = self.refresh_now(id, &token).await;
        *crate::lock(&flight.running) = None;
        // A cancelled refresh answers only its caller: one waiting runs.
        if !matches!(
            result,
            Err(RefreshError::Provider(ProviderError::Cancelled))
        ) {
            let mut finished = crate::lock(&flight.finished);
            finished.0 += 1;
            finished.1 = Some(result.clone());
        }
        result
    }

    async fn refresh_now(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> std::result::Result<RefreshOutcome, RefreshError> {
        let mut account = self.account(id).await?;
        if !account.enabled {
            return Err(Error::Invalid(crate::tr!(
                "{account} is disconnected; connect it to refresh its library",
                account = account.display_name
            ))
            .into());
        }
        let provider = self.provider(&account.provider).ok_or_else(|| {
            RefreshError::from(Error::Invalid(crate::tr!(
                "Caskade can't refresh {provider} accounts this session",
                provider = account.provider
            )))
        })?;
        // A provider without libraries (achievements only, #189) has
        // nothing to list: the refresher brings its achievements instead.
        let refresher = if provider.capabilities().library {
            None
        } else {
            match self.inner.refresher.get() {
                Some(refresher) => Some(Arc::clone(refresher)),
                None => return Ok(RefreshOutcome::default()),
            }
        };
        let now = crate::now_ms();
        // The store said when to ask again: not before, whoever asks.
        if let Some(retry_at) = account.sync.retry_at_ms.filter(|at| *at > now) {
            return Err(RefreshError::Provider(ProviderError::RateLimited {
                retry_after: Some(Duration::from_millis(
                    u64::try_from(retry_at - now).unwrap_or(0),
                )),
            }));
        }
        let _activity = caskade_jobs::global().activity(crate::tr!(
            "Refreshing {account}",
            account = account.display_name
        ));
        account.sync.last_attempt_ms = Some(now);
        if let Some(refresher) = refresher {
            let refreshed = tokio::select! {
                refreshed = refresher.refresh_account(&account, cancel) => refreshed,
                () = cancel.cancelled() => Err(ProviderError::Cancelled),
            };
            if let Err(error) = refreshed {
                return self.refresh_failed(account, now, error).await;
            }
            account.sync.failures = 0;
            account.sync.retry_at_ms = None;
            account.sync.last_ok_ms = Some(now);
            account.sync.error = None;
            self.inner.store.save_store_account(&account).await?;
            self.changed();
            tracing::info!(account = %account.id, "store account's achievements refreshed");
            return Ok(RefreshOutcome::default());
        }
        let listed = tokio::select! {
            listed = self.list(&provider, &account, cancel) => listed,
            () = cancel.cancelled() => Err(ProviderError::Cancelled),
        };
        let snapshot = match listed {
            Ok(snapshot) => snapshot,
            Err(error) => return self.refresh_failed(account, now, error).await,
        };
        account.sync.failures = 0;
        account.sync.retry_at_ms = None;
        let applied = self
            .inner
            .store
            .apply_snapshot(&account, &snapshot, now)
            .await?;
        if snapshot.complete {
            account.sync.last_ok_ms = Some(now);
            account.sync.error = None;
        } else {
            account.sync.error = Some(ErrorReport::new(
                crate::error::ErrorCode::Server,
                crate::tr!(
                    "the store sent part of the library; nothing was marked as no longer owned"
                ),
            ));
        }
        self.inner.store.save_store_account(&account).await?;
        for game_id in &applied.added {
            self.inner.bus.emit(Event::LibraryGameAdded {
                game_id: game_id.clone(),
            });
        }
        for game_id in &applied.updated {
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: game_id.clone(),
            });
        }
        self.record_playtime(&account, &snapshot, now).await?;
        let mut outcome = applied.outcome;
        outcome.merged = self.reconcile().await?;
        self.changed();
        tracing::info!(account = %account.id, ?outcome, "store library refreshed");
        Ok(outcome)
    }

    /// Records why a refresh failed, except a cancelled one, and answers it.
    async fn refresh_failed(
        &self,
        mut account: StoreAccount,
        now: i64,
        error: ProviderError,
    ) -> std::result::Result<RefreshOutcome, RefreshError> {
        if error != ProviderError::Cancelled {
            account.sync.error = Some(ErrorReport::from_error(&error));
            account.sync.failures = account.sync.failures.saturating_add(1);
            if let ProviderError::RateLimited { retry_after } = &error {
                let wait = retry_after.unwrap_or(Duration::from_secs(15 * 60));
                account.sync.retry_at_ms =
                    Some(now + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX / 2));
            }
            self.inner.store.save_store_account(&account).await?;
            self.changed();
        }
        tracing::warn!(account = %account.id, %error, "store account not refreshed");
        Err(RefreshError::Provider(error))
    }

    /// The account's sign-in for a call to its store, renewed first (and
    /// kept) when it has expired or is about to and can be. `Ok(None)` for
    /// a provider that reads a launcher's own sign-in; `Expired` when there
    /// is none. Stays in the core: never send it to the webview, store or
    /// log it. Callers wait out `sync.retry_at_ms` too.
    pub async fn credentials(
        &self,
        account_id: &str,
    ) -> std::result::Result<Option<StoreCredentials>, ProviderError> {
        let account = self
            .account(account_id)
            .await
            .map_err(|error| ProviderError::Other(error.to_string()))?;
        let provider = self.provider(&account.provider).ok_or_else(|| {
            ProviderError::Other(crate::tr!(
                "Caskade can't reach {provider} accounts this session",
                provider = account.provider
            ))
        })?;
        self.sign_in(&provider, &account).await
    }

    async fn sign_in(
        &self,
        provider: &Arc<dyn AccountProvider>,
        account: &StoreAccount,
    ) -> std::result::Result<Option<StoreCredentials>, ProviderError> {
        let Some(current) = self.inner.credentials.get(&account.id).await else {
            return if provider.capabilities().auth == AuthKind::Launcher {
                Ok(None)
            } else {
                Err(ProviderError::Expired(crate::tr!("not signed in")))
            };
        };
        if !current
            .expires_at_ms
            .is_some_and(|at| at <= crate::now_ms() + 60_000)
        {
            return Ok(Some(current));
        }
        let renewed = provider.refresh_credentials(&current).await?;
        if let Err(error) = self.inner.credentials.set(&account.id, &renewed).await {
            tracing::warn!(%error, "the renewed sign-in wasn't kept");
        }
        Ok(Some(renewed))
    }

    /// The account's library with its sign-in ([`Self::sign_in`]).
    async fn list(
        &self,
        provider: &Arc<dyn AccountProvider>,
        account: &StoreAccount,
        cancel: &CancellationToken,
    ) -> std::result::Result<LibrarySnapshot, ProviderError> {
        let credentials = self.sign_in(provider, account).await?;
        provider
            .library(account, credentials.as_ref(), cancel)
            .await
    }

    /// Cancels the refresh of `id` under way, if any.
    pub fn cancel_refresh(&self, id: &str) {
        if let Some(token) = crate::lock(&self.flight(id).running).as_ref() {
            token.cancel();
        }
    }

    /// Starts signing in to `provider`, or (`reconnect`) signing an
    /// account in again. The flow expires after ten minutes.
    pub async fn begin_connect(
        &self,
        provider: &str,
        reconnect: Option<String>,
    ) -> Result<AuthFlow> {
        let source = self.provider(provider).ok_or_else(|| {
            Error::Invalid(crate::tr!(
                "Caskade can't connect {provider} accounts",
                provider = provider
            ))
        })?;
        if let Some(id) = &reconnect {
            let account = self.account(id).await?;
            if account.provider != provider {
                return Err(Error::Invalid(crate::tr!(
                    "that account belongs to another store"
                )));
            }
        }
        let flow_id = crate::model::new_id();
        let challenge = source
            .begin_auth(&flow_id)
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let now = crate::now_ms();
        let flow = AuthFlow {
            flow_id: flow_id.clone(),
            provider: provider.to_owned(),
            kind: source.capabilities().auth,
            challenge,
            expires_at_ms: now + FLOW_TTL_MS,
            reconnect,
        };
        let mut flows = crate::lock(&self.inner.flows);
        flows.retain(|_, flow| flow.expires_at_ms > now);
        flows.insert(flow_id, flow.clone());
        drop(flows);
        self.changed();
        Ok(flow)
    }

    /// Finishes the sign-in `flow_id` with `input`. A browser callback must
    /// carry the flow's id as its `state`. Signing in as another account
    /// than the one being reconnected changes nothing.
    pub async fn complete_connect(
        &self,
        flow_id: &str,
        input: &AuthInput,
        owner_id: Option<String>,
    ) -> std::result::Result<StoreAccount, ConnectError> {
        let flow = {
            let mut flows = crate::lock(&self.inner.flows);
            match flows.get(flow_id) {
                Some(flow) if flow.expires_at_ms > crate::now_ms() => flow.clone(),
                Some(_) => {
                    flows.remove(flow_id);
                    return Err(ConnectError::Expired);
                }
                None => return Err(ConnectError::Expired),
            }
        };
        if let AuthInput::Callback { params } = input
            && params.get("state").map(String::as_str) != Some(flow_id)
        {
            self.cancel_connect(flow_id);
            return Err(ConnectError::Mismatch);
        }
        let provider = self.provider(&flow.provider).ok_or(ConnectError::Expired)?;
        let signed_in = provider
            .complete_auth(flow_id, input)
            .await
            .map_err(ConnectError::Provider)?;
        if let Some(id) = &flow.reconnect {
            let account = self.account(id).await?;
            if account.external_id != signed_in.external_id {
                self.cancel_connect(flow_id);
                return Err(ConnectError::OtherAccount {
                    signed_in: signed_in.display_name,
                    expected: account.display_name,
                });
            }
        }
        crate::lock(&self.inner.flows).remove(flow_id);
        let mut account = self
            .connect(
                &flow.provider,
                &signed_in.external_id,
                &signed_in.display_name,
                owner_id,
            )
            .await?;
        match &signed_in.credentials {
            Some(credentials) => self.inner.credentials.set(&account.id, credentials).await?,
            None => self.inner.credentials.delete(&account.id).await?,
        }
        // A fresh sign-in clears the last one's trouble.
        account.sync.error = None;
        account.sync.failures = 0;
        self.inner.store.save_store_account(&account).await?;
        self.changed();
        Ok(account)
    }

    /// Ages every sign-in under way past its ten minutes.
    #[cfg(test)]
    pub(crate) fn expire_flows(&self) {
        for flow in crate::lock(&self.inner.flows).values_mut() {
            flow.expires_at_ms = 0;
        }
    }

    /// Abandons the sign-in `flow_id`.
    pub fn cancel_connect(&self, flow_id: &str) {
        if crate::lock(&self.inner.flows).remove(flow_id).is_some() {
            self.changed();
        }
    }

    /// Sign-ins under way.
    pub fn flows(&self) -> Vec<AuthFlow> {
        let now = crate::now_ms();
        crate::lock(&self.inner.flows)
            .values()
            .filter(|flow| flow.expires_at_ms > now)
            .cloned()
            .collect()
    }

    /// Records the play time a snapshot reports for the account's
    /// releases, as `core.storeAccounts.playtimeImport` says (#169).
    async fn record_playtime(
        &self,
        account: &StoreAccount,
        snapshot: &LibrarySnapshot,
        now: i64,
    ) -> Result<()> {
        let mode = self.settings().await.playtime_import;
        let changed = if mode == crate::db::PlaytimeImport::Never {
            self.inner.store.forget_remote_playtime(&account.id).await?
        } else {
            let entries: HashMap<String, String> = self
                .inner
                .store
                .entitlements(&account.id)
                .await?
                .into_iter()
                .filter_map(|e| Some((e.product.product_id, e.entry_id?)))
                .collect();
            let totals: Vec<crate::db::RemoteTotal> = snapshot
                .products
                .iter()
                .filter(|p| p.playtime_minutes.is_some() || p.last_played_ms.is_some())
                .filter_map(|p| {
                    Some(crate::db::RemoteTotal {
                        entry_id: entries.get(p.product_id.trim())?.clone(),
                        total_ms: p.playtime_minutes.map(|m| m.saturating_mul(60_000)),
                        last_played_ms: p.last_played_ms,
                    })
                })
                .collect();
            self.inner
                .store
                .record_remote_playtime(&account.provider, &account.id, &totals, now, mode)
                .await?
        };
        for game_id in changed {
            self.inner.bus.emit(Event::LibraryGameUpdated { game_id });
        }
        Ok(())
    }

    /// Makes games that share a store id with an account's entry one game:
    /// an owned game and an installed copy discovery imported separately.
    /// Every release is kept; hidden games and games kept apart stay as
    /// they are. Returns how many games merged away.
    pub async fn reconcile(&self) -> Result<u32> {
        let _merging = self.inner.reconciling.lock().await;
        let mut merged = 0;
        for group in self.inner.store.store_id_groups().await? {
            let Some((keep, others)) = group.split_first() else {
                continue;
            };
            for other in others {
                match self.inner.store.merge_games(keep, other).await {
                    Ok(_) => {
                        merged += 1;
                        self.inner.bus.emit(Event::LibraryGameRemoved {
                            game_id: other.clone(),
                        });
                    }
                    // A game playing now merges on a later pass.
                    Err(error) => tracing::debug!(%error, keep, other, "games not merged yet"),
                }
            }
            self.inner.bus.emit(Event::LibraryGameUpdated {
                game_id: keep.clone(),
            });
        }
        if merged > 0 {
            self.changed();
        }
        Ok(merged)
    }

    /// Reconciles after every library scan, and makes background refreshes
    /// (`core.storeAccounts`), until the process ends.
    pub fn watch(&self) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        let mut events = self.inner.bus.stream();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            let scans = async {
                loop {
                    match events.recv().await {
                        Ok(Event::LibraryScanCompleted { .. }) | Err(RecvError::Lagged(_)) => {
                            if let Err(error) = this.reconcile().await {
                                tracing::warn!(%error, "owned games not reconciled");
                            }
                        }
                        Ok(_) => {}
                        Err(RecvError::Closed) => break,
                    }
                }
            };
            tokio::join!(scans, this.clone().schedule());
        })
    }

    /// What removing `id` would take out of the library.
    pub async fn removal_preview(&self, id: &str) -> Result<RemovalPreview> {
        self.account(id).await?;
        self.inner.store.removal_preview(id).await
    }

    /// Removes the account, what it owned, and the games only it brought;
    /// games installed, added another way or owned by another account stay.
    pub async fn remove(&self, id: &str) -> Result<RemovalPreview> {
        let preview = self.removal_preview(id).await?;
        self.cancel_refresh(id);
        let account = self.account(id).await?;
        self.forget_sign_in(&account).await?;
        let _running = self.flight(id).lock.lock_owned().await;
        let (removed, kept) = self.inner.store.remove_store_account(id).await?;
        for game_id in removed {
            self.inner.bus.emit(Event::LibraryGameRemoved { game_id });
        }
        for game_id in kept {
            self.inner.bus.emit(Event::LibraryGameUpdated { game_id });
        }
        crate::lock(&self.inner.flights).remove(id);
        self.changed();
        Ok(preview)
    }

    /// The store accounts that own `game_id`.
    pub async fn ownership(&self, game_id: &str) -> Result<Vec<Ownership>> {
        self.inner.store.ownership(game_id).await
    }
}
