//! Store accounts, what they own, and what providers can do.

use crate::error::ErrorReport;
use caskade_discovery::ExternalId;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

/// A launcher or store account Caskade reads owned games from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccount {
    pub id: String,
    /// The provider's id (`steam`, `gog`, `epic`, a plugin's).
    pub provider: String,
    /// The account's id at the provider: its ownership namespace.
    pub external_id: String,
    pub display_name: String,
    /// The local account that connected it; `None` for everyone's.
    pub owner_id: Option<String>,
    /// Off after Disconnect: no refreshes, records kept.
    pub enabled: bool,
    pub sync: SyncState,
    pub created_at_ms: i64,
}

/// How the account's library refreshes went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default, rename = "StoreSyncState")]
pub struct SyncState {
    /// The last refresh that brought a complete library.
    pub last_ok_ms: Option<i64>,
    pub last_attempt_ms: Option<i64>,
    /// Why the last refresh failed or was partial; `None` when it worked.
    pub error: Option<ErrorReport>,
    /// Refreshes that failed in a row.
    pub failures: u32,
    /// No refresh before this: the store asked Caskade to wait.
    pub retry_at_ms: Option<i64>,
}

/// One product an account owns, as a provider lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreProduct")]
pub struct OwnedProduct {
    /// The provider's product id, unique in the account.
    pub product_id: String,
    pub title: String,
    /// Store identities discovery also reads (`Steam { app_id }`, …): how
    /// an installed copy is recognised as this product.
    #[serde(default)]
    pub ids: Vec<ExternalId>,
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Play time the store reports, in minutes; `None` when it doesn't say.
    #[serde(default)]
    pub playtime_minutes: Option<u64>,
    /// When the store says it was last played (Unix ms); `None` when unknown.
    #[serde(default)]
    pub last_played_ms: Option<i64>,
    /// The systems the store says it runs on (`windows`, `mac`, `linux`);
    /// empty when it doesn't say.
    #[serde(default)]
    pub systems: Vec<String>,
}

/// A provider's answer to a library refresh.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibrarySnapshot {
    pub products: Vec<OwnedProduct>,
    /// Every page came back: only then does a product's absence mean the
    /// account lost it.
    pub complete: bool,
}

/// What an account owns, as the library keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreEntitlement")]
pub struct Entitlement {
    pub store_account_id: String,
    pub product: OwnedProduct,
    /// The library entry it shows as; `None` once the user removed the game.
    pub entry_id: Option<String>,
    /// The game that entry belongs to.
    pub game_id: Option<String>,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    /// When a complete refresh stopped listing it.
    pub lost_at_ms: Option<i64>,
}

/// How a provider signs in (#165).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreAuthKind")]
pub enum AuthKind {
    /// A login page in the browser that calls back.
    Browser,
    /// A code entered on another device.
    Device,
    /// An API key or token the user pastes.
    ApiKey,
    /// Reads a launcher's own sign-in on this computer.
    Launcher,
}

/// What a provider can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreProviderCapabilities")]
pub struct ProviderCapabilities {
    pub id: String,
    /// Its name for people (`Steam`).
    pub name: String,
    pub auth: AuthKind,
    /// Lists owned games, installed or not.
    pub library: bool,
    /// Reports play time.
    pub playtime: bool,
    /// Can install an owned game through its launcher.
    pub install: bool,
}

/// A store account in Settings: the account, its provider and its library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccountSummary {
    pub account: StoreAccount,
    /// `None` when no provider with its id is available this session.
    pub capabilities: Option<ProviderCapabilities>,
    /// Products owned now.
    pub owned: u32,
    /// Products a complete refresh no longer lists.
    pub lost: u32,
    /// Owned products with a release installed.
    pub installed: u32,
    pub state: StoreConnectionState,
    /// The sign-in is kept in memory only: the keychain couldn't be used,
    /// so it's asked for again after a restart.
    pub session_only: bool,
}

/// Where a store account's connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreConnectionState")]
pub enum StoreConnectionState {
    /// Disconnected: kept, not refreshed.
    NotConnected,
    /// A sign-in is under way.
    Authenticating,
    Connected,
    /// A refresh is under way.
    Syncing,
    /// The store couldn't be reached last time.
    Offline,
    /// The sign-in expired or was revoked: connect again.
    Expired,
    /// No provider for it this session.
    Unsupported,
}

/// A sign-in: tokens or a key. Never in the database, settings, IPC
/// answers, logs or backups; `Debug` doesn't show it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreCredentials {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// When `access_token` stops working (Unix ms), when known.
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
    /// Anything else the provider keeps.
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl std::fmt::Debug for StoreCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreCredentials")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

/// What the user does to sign in, from the provider.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreAuthChallenge")]
pub struct AuthChallenge {
    /// The page to open: a login page, a device page, or where to get a key.
    pub url: Option<String>,
    /// The code to enter on that page (device sign-in).
    pub user_code: Option<String>,
    /// What to fill in, for a provider that asks for more than one key.
    #[serde(default)]
    pub fields: Vec<AuthField>,
    /// What to know first (privacy settings, where a key comes from), for
    /// people.
    #[serde(default)]
    pub instructions: Option<String>,
}

/// One value a sign-in asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreAuthField")]
pub struct AuthField {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    /// A key or password: hidden as typed.
    pub secret: bool,
}

/// A sign-in under way. `flow_id` is also the `state` a browser callback
/// must carry back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreAuthFlow")]
pub struct AuthFlow {
    pub flow_id: String,
    pub provider: String,
    pub kind: AuthKind,
    pub challenge: AuthChallenge,
    pub expires_at_ms: i64,
    /// The account being connected again, if any.
    pub reconnect: Option<String>,
}

/// What finishes a sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase", rename = "StoreAuthInput")]
pub enum AuthInput {
    /// The browser's callback: the query it came back with, `state` among it.
    Callback { params: BTreeMap<String, String> },
    /// A key, a token or a code the user pasted.
    Code { code: String },
    /// The challenge's fields, by id.
    Fields { fields: BTreeMap<String, String> },
    /// Asks whether a device sign-in is done.
    Poll,
    /// A launcher's own sign-in on this computer.
    Launcher,
}

/// Who signed in, and their sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub external_id: String,
    pub display_name: String,
    /// `None` for a provider that reads a launcher's own sign-in.
    pub credentials: Option<StoreCredentials>,
}

/// What a refresh changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreRefreshOutcome")]
pub struct RefreshOutcome {
    /// New products, each now a library game (an existing one when an
    /// installed copy was already there).
    pub added: u32,
    pub updated: u32,
    /// Products a complete refresh no longer lists.
    pub lost: u32,
    /// Products listed again after being lost.
    pub regained: u32,
    /// The refresh was partial: nothing was marked lost.
    pub partial: bool,
    /// Games merged with an installed copy found under another source.
    pub merged: u32,
}

/// What removing an account would do; nothing changes until it's confirmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreRemovalPreview")]
pub struct RemovalPreview {
    /// Games that only this account brought: they go.
    pub removed: Vec<GameRef>,
    /// Games that stay, because they're installed, added another way or
    /// owned by another account; only this account's entries go.
    pub kept: Vec<GameRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "StoreGameRef")]
pub struct GameRef {
    pub game_id: String,
    pub title: String,
}

/// Who owns a library game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "GameOwnership")]
pub struct Ownership {
    pub store_account_id: String,
    pub provider: String,
    pub display_name: String,
    pub product_id: String,
    pub lost_at_ms: Option<i64>,
}
