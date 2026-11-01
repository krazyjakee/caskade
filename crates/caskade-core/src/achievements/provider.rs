//! What an achievement service does for Caskade: say which of a release's
//! identities are its games, and read an account's definitions and state.
//! Nothing here writes to a service: providers read only.

use super::{AchievementCapabilities, AchievementDefinition, AchievementState};
use crate::store_accounts::{ProviderError, StoreAccount, StoreCredentials};
use async_trait::async_trait;
use caskade_discovery::ExternalId;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

/// A service's answer for one game and account.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fetched {
    /// The game's achievements, in the service's order.
    pub definitions: Vec<AchievementDefinition>,
    /// The account's state, by achievement id; missing ones are locked.
    pub states: BTreeMap<String, AchievementState>,
    /// Every page came back. An incomplete answer changes only what it
    /// lists and never drops an achievement.
    pub complete: bool,
}

/// Why a fetch brought no achievements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The service doesn't offer this game's achievements (or this kind of
    /// game at all). The text says why.
    Unsupported(String),
    /// The store's answer: offline, expired, rate-limited, private
    /// (`Unavailable`), cancelled.
    Provider(ProviderError),
}

impl From<ProviderError> for FetchError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(why) => f.write_str(why),
            Self::Provider(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FetchError {}

/// An achievement service, for the accounts of the store provider with the
/// same id (`steam`, `gog`, `epic`, `retroachievements`).
#[async_trait]
pub trait AchievementProvider: Send + Sync {
    /// The store provider whose accounts it reads.
    fn id(&self) -> String;
    fn capabilities(&self) -> AchievementCapabilities;
    /// The service's game id for one of a release's store identities, when
    /// it is one of this service's. Releases without one can still be
    /// linked (`AchievementLink`).
    fn product(&self, _id: &ExternalId) -> Option<String> {
        None
    }
    /// The game's definitions and `account`'s state. Read only.
    async fn fetch(
        &self,
        account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        product: &str,
        cancel: &CancellationToken,
    ) -> Result<Fetched, FetchError>;
}

/// What linking a release found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkOutcome {
    /// The service's game, verified by this hash of the release's ROM.
    Linked { product: String, hash: String },
    /// No game could be verified; the text says why, for people.
    Unresolved(String),
}

/// Finds a release's game at a service that its store ids don't give
/// (RetroAchievements, by ROM hash).
#[async_trait]
pub trait ReleaseLinker: Send + Sync {
    /// The store provider whose accounts it uses.
    fn provider(&self) -> String;
    /// Whether it could link `entry` at all (an emulated game).
    fn applies(&self, entry: &crate::model::GameEntry) -> bool;
    async fn link(
        &self,
        store: &crate::db::LibraryStore,
        entry: &crate::model::GameEntry,
        account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        cancel: &CancellationToken,
    ) -> Result<LinkOutcome, FetchError>;
}

/// The store accounts achievements are read for, and their sign-ins.
#[async_trait]
pub trait AccountAccess: Send + Sync {
    /// Every store account, connected or not.
    async fn accounts(&self) -> crate::Result<Vec<StoreAccount>>;
    /// The account's current sign-in, renewed first when it is about to
    /// expire; `Expired` when there is none to use.
    async fn credentials(
        &self,
        account: &StoreAccount,
    ) -> Result<Option<StoreCredentials>, ProviderError>;
    /// The provider's name for people (`Steam`).
    fn provider_name(&self, provider: &str) -> Option<String>;
}
