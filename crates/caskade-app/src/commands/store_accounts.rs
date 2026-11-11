//! Store accounts (#164, #165): providers, connecting and signing in,
//! connected accounts and what they own. `StoreAccountsChanged` follows
//! every change; refreshes run as Activity work. Sign-ins stay in the core
//! and the OS keychain: no command answers with a token.
use caskade_core::{
    accounts::AccountService,
    db::{LibraryStore, PlaytimeBreakdown},
    store_accounts::{
        AuthFlow, AuthInput, OwnedRelease, Ownership, ProviderCapabilities, RefreshOutcome,
        RemovalPreview, StoreAccount, StoreAccountSummary, StoreAccounts,
    },
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::discovery::DiscoveryController;
use crate::error::{CommandError, report};
use crate::events::{Event, Events};

/// Store accounts or what they own changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StoreAccountsChanged;

/// Reconciles owned games after every library scan, and tells the UI of
/// the service's changes.
pub fn setup(events: Events, accounts: &StoreAccounts) {
    let mut changes = accounts.subscribe();
    crate::runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            events.send(Event::StoreAccountsChanged(StoreAccountsChanged));
        }
    });
    crate::background::supervise("owned games", {
        let accounts = accounts.clone();
        move || accounts.watch()
    });
}

/// The store providers available this session.
pub fn store_providers(accounts: &StoreAccounts) -> Vec<ProviderCapabilities> {
    accounts.providers()
}

/// The connected accounts.
pub async fn store_accounts_list(
    accounts: &StoreAccounts,
) -> Result<Vec<StoreAccountSummary>, CommandError> {
    accounts.accounts().await.map_err(report)
}

/// Refreshes what an account owns now.
pub async fn store_account_refresh(
    accounts: &StoreAccounts,
    id: &str,
) -> Result<RefreshOutcome, CommandError> {
    crate::permissions::enforce("store_account_refresh", serde_json::Value::default)?;
    accounts
        .refresh(id, &caskade_backup::CancellationToken::new())
        .await
        .map_err(report)
}

/// Stops refreshing an account; its games stay.
pub async fn store_account_disconnect(
    accounts: &StoreAccounts,
    id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("store_account_disconnect", serde_json::Value::default)?;
    accounts.disconnect(id).await.map(|_| ()).map_err(report)
}

/// What removing an account would take out of the library.
pub async fn store_account_removal_preview(
    accounts: &StoreAccounts,
    id: &str,
) -> Result<RemovalPreview, CommandError> {
    accounts.removal_preview(id).await.map_err(report)
}

/// Removes an account and the games only it brought.
pub async fn store_account_remove(
    accounts: &StoreAccounts,
    id: &str,
) -> Result<RemovalPreview, CommandError> {
    crate::permissions::enforce("store_account_remove", serde_json::Value::default)?;
    accounts.remove(id).await.map_err(report)
}

/// Cancels an account's refresh under way.
pub fn store_account_cancel_refresh(accounts: &StoreAccounts, id: &str) {
    if crate::permissions::enforce("store_account_cancel_refresh", serde_json::Value::default)
        .is_err()
    {
        return;
    }
    accounts.cancel_refresh(id);
}

/// Sign-ins under way.
pub fn store_auth_flows(accounts: &StoreAccounts) -> Vec<AuthFlow> {
    accounts.flows()
}

/// Starts signing in to `provider`, or signing `reconnect` in again: the
/// page to open or the code to enter. Expires after ten minutes.
pub async fn store_connect_begin(
    accounts: &StoreAccounts,
    provider: &str,
    reconnect: Option<String>,
) -> Result<AuthFlow, CommandError> {
    crate::permissions::enforce("store_connect_begin", serde_json::Value::default)?;
    accounts
        .begin_connect(provider, reconnect)
        .await
        .map_err(report)
}

/// Finishes a sign-in with a pasted key or code, a device poll, a
/// launcher's sign-in or a browser callback; then refreshes the account's
/// library in the background. The account belongs to whoever is signed in
/// to Caskade (`signed_in`, when accounts are up).
pub async fn store_connect_complete(
    accounts: &StoreAccounts,
    signed_in: Option<&AccountService>,
    flow_id: &str,
    input: &AuthInput,
) -> Result<StoreAccount, CommandError> {
    crate::permissions::enforce("store_connect_complete", serde_json::Value::default)?;
    let owner = signed_in
        .and_then(|service| service.signed_in().ok())
        .map(|account| account.id);
    let account = accounts
        .complete_connect(flow_id, input, owner)
        .await
        .map_err(report)?;
    let (accounts, id) = (accounts.clone(), account.id.clone());
    crate::runtime::spawn(async move {
        if let Err(error) = accounts
            .refresh(&id, &caskade_backup::CancellationToken::new())
            .await
        {
            tracing::warn!(%error, "the new store account's library wasn't refreshed");
        }
    });
    Ok(account)
}

/// Abandons a sign-in.
pub fn store_connect_cancel(accounts: &StoreAccounts, flow_id: &str) {
    if crate::permissions::enforce("store_connect_cancel", serde_json::Value::default).is_err() {
        return;
    }
    accounts.cancel_connect(flow_id);
}

/// A store's browser sign-in calling back: `caskade://store-auth?state=…`.
pub fn is_callback(link: &url::Url) -> bool {
    format!("{}{}", link.host_str().unwrap_or_default(), link.path()).trim_matches('/')
        == "store-auth"
}

/// Finishes the sign-in a callback link names. The link carries a code:
/// it is never logged.
pub async fn callback(
    accounts: &StoreAccounts,
    signed_in: Option<&AccountService>,
    link: &url::Url,
) {
    let params: std::collections::BTreeMap<String, String> =
        link.query_pairs().into_owned().collect();
    let Some(flow_id) = params.get("state").cloned() else {
        tracing::warn!("a store sign-in callback without a state was ignored");
        return;
    };
    match store_connect_complete(
        accounts,
        signed_in,
        &flow_id,
        &AuthInput::Callback { params },
    )
    .await
    {
        Ok(account) => tracing::info!(account = %account.id, "store account signed in"),
        Err(error) => tracing::warn!(%error, "store sign-in callback refused"),
    }
}

/// Where a game's play time comes from (#169): tracked sessions, and what
/// each store or launcher reported.
pub async fn game_playtime(
    store: &LibraryStore,
    game_id: &str,
) -> Result<PlaytimeBreakdown, CommandError> {
    store.playtime_breakdown(game_id).await.map_err(report)
}

/// The launchers discovery found on this computer, by discovery source;
/// none before discovery is up.
async fn launchers(discovery: Option<&DiscoveryController>) -> std::collections::BTreeSet<String> {
    let Some(controller) = discovery else {
        return Default::default();
    };
    let mut found = controller.snapshot().await.launchers;
    if found.is_empty() {
        found = controller
            .launchers()
            .await
            .map(|snapshot| snapshot.launchers)
            .unwrap_or_default();
    }
    found
        .into_iter()
        .filter(|launcher| launcher.found)
        .map(|launcher| launcher.source.to_string())
        .collect()
}

/// A game's owned releases and whether each can be installed here (#170).
pub async fn game_owned_releases(
    accounts: &StoreAccounts,
    discovery: Option<&DiscoveryController>,
    game_id: &str,
) -> Result<Vec<OwnedRelease>, CommandError> {
    let launchers = launchers(discovery).await;
    accounts
        .owned_releases(game_id, &launchers)
        .await
        .map_err(report)
}

/// Hands an owned release to its launcher to install. It shows as being
/// installed until discovery finds the copy.
pub async fn owned_install(
    accounts: &StoreAccounts,
    discovery: Option<&DiscoveryController>,
    entry_id: &str,
) -> Result<OwnedRelease, CommandError> {
    crate::permissions::enforce("owned_install", serde_json::Value::default)?;
    let launchers = launchers(discovery).await;
    accounts.install(entry_id, &launchers).await.map_err(report)
}

/// Stops waiting for a launcher's install (cancelled or failed there).
pub fn owned_install_cancel(accounts: &StoreAccounts, entry_id: &str) {
    if crate::permissions::enforce("owned_install_cancel", serde_json::Value::default).is_err() {
        return;
    }
    accounts.cancel_install(entry_id);
}

/// The store accounts that own a game.
pub async fn game_ownership(
    accounts: &StoreAccounts,
    game_id: &str,
) -> Result<Vec<Ownership>, CommandError> {
    accounts.ownership(game_id).await.map_err(report)
}
