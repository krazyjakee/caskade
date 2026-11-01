//! Store accounts (#164, #165): providers, connecting and signing in,
//! connected accounts and what they own. `StoreAccountsChanged` follows
//! every change; refreshes run as Activity work. Sign-ins stay in the core
//! and the OS keychain: no command answers with a token.

use crate::error::{CommandError, report};
use caskade_core::store_accounts::{
    AuthFlow, AuthInput, Ownership, ProviderCapabilities, RefreshOutcome, RemovalPreview,
    StoreAccount, StoreAccountSummary, StoreAccounts,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

/// Store accounts or what they own changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct StoreAccountsChanged;

/// Manages the service, reconciles owned games after every library scan,
/// and forwards its changes to the webview.
pub fn setup<R: Runtime>(app: &AppHandle<R>, accounts: StoreAccounts) {
    let mut changes = accounts.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            if let Err(error) = StoreAccountsChanged.emit(&handle) {
                tracing::debug!(%error, "store accounts change not sent");
            }
        }
    });
    crate::background::supervise("owned games", {
        let accounts = accounts.clone();
        move || accounts.watch()
    });
    app.manage(accounts);
}

/// The store providers available this session.
#[tauri::command]
#[specta::specta]
pub fn store_providers(accounts: State<'_, StoreAccounts>) -> Vec<ProviderCapabilities> {
    accounts.providers()
}

#[tauri::command]
#[specta::specta]
pub async fn store_accounts_list(
    accounts: State<'_, StoreAccounts>,
) -> Result<Vec<StoreAccountSummary>, CommandError> {
    accounts.accounts().await.map_err(report)
}

/// Refreshes what an account owns now.
#[tauri::command]
#[specta::specta]
pub async fn store_account_refresh(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<RefreshOutcome, CommandError> {
    accounts
        .refresh(&id, &caskade_core::backup::CancellationToken::new())
        .await
        .map_err(report)
}

/// Stops refreshing an account; its games stay.
#[tauri::command]
#[specta::specta]
pub async fn store_account_disconnect(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<(), CommandError> {
    accounts.disconnect(&id).await.map(|_| ()).map_err(report)
}

/// What removing an account would take out of the library.
#[tauri::command]
#[specta::specta]
pub async fn store_account_removal_preview(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<RemovalPreview, CommandError> {
    accounts.removal_preview(&id).await.map_err(report)
}

/// Removes an account and the games only it brought.
#[tauri::command]
#[specta::specta]
pub async fn store_account_remove(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<RemovalPreview, CommandError> {
    accounts.remove(&id).await.map_err(report)
}

/// Cancels an account's refresh under way.
#[tauri::command]
#[specta::specta]
pub fn store_account_cancel_refresh(accounts: State<'_, StoreAccounts>, id: String) {
    accounts.cancel_refresh(&id);
}

/// Sign-ins under way.
#[tauri::command]
#[specta::specta]
pub fn store_auth_flows(accounts: State<'_, StoreAccounts>) -> Vec<AuthFlow> {
    accounts.flows()
}

/// Starts signing in to `provider`, or signing `reconnect` in again: the
/// page to open or the code to enter. Expires after ten minutes.
#[tauri::command]
#[specta::specta]
pub async fn store_connect_begin(
    accounts: State<'_, StoreAccounts>,
    provider: String,
    reconnect: Option<String>,
) -> Result<AuthFlow, CommandError> {
    accounts
        .begin_connect(&provider, reconnect)
        .await
        .map_err(report)
}

/// Finishes a sign-in with a pasted key or code, a device poll, or a
/// launcher's sign-in; then refreshes the account's library in the
/// background. The account belongs to whoever is signed in to Caskade.
#[tauri::command]
#[specta::specta]
pub async fn store_connect_complete<R: Runtime>(
    app: AppHandle<R>,
    accounts: State<'_, StoreAccounts>,
    flow_id: String,
    input: AuthInput,
) -> Result<StoreAccount, CommandError> {
    complete(&app, &accounts, &flow_id, &input).await
}

/// Abandons a sign-in.
#[tauri::command]
#[specta::specta]
pub fn store_connect_cancel(accounts: State<'_, StoreAccounts>, flow_id: String) {
    accounts.cancel_connect(&flow_id);
}

async fn complete<R: Runtime>(
    app: &AppHandle<R>,
    accounts: &StoreAccounts,
    flow_id: &str,
    input: &AuthInput,
) -> Result<StoreAccount, CommandError> {
    let owner = app
        .try_state::<caskade_core::accounts::AccountService>()
        .and_then(|service| service.signed_in().ok())
        .map(|account| account.id);
    let account = accounts
        .complete_connect(flow_id, input, owner)
        .await
        .map_err(report)?;
    let (accounts, id) = (accounts.clone(), account.id.clone());
    tauri::async_runtime::spawn(async move {
        if let Err(error) = accounts
            .refresh(&id, &caskade_core::backup::CancellationToken::new())
            .await
        {
            tracing::warn!(%error, "the new store account's library wasn't refreshed");
        }
    });
    Ok(account)
}

/// A store's browser sign-in calling back: `caskade://store-auth?state=…`.
pub fn is_callback(link: &tauri::Url) -> bool {
    format!("{}{}", link.host_str().unwrap_or_default(), link.path()).trim_matches('/')
        == "store-auth"
}

/// Finishes the sign-in a callback link names. The link carries a code:
/// it is never logged.
pub async fn callback<R: Runtime>(app: &AppHandle<R>, link: tauri::Url) {
    let Some(accounts) = app.try_state::<StoreAccounts>() else {
        return;
    };
    let params: std::collections::BTreeMap<String, String> =
        link.query_pairs().into_owned().collect();
    let Some(flow_id) = params.get("state").cloned() else {
        tracing::warn!("a store sign-in callback without a state was ignored");
        return;
    };
    match complete(app, &accounts, &flow_id, &AuthInput::Callback { params }).await {
        Ok(account) => tracing::info!(account = %account.id, "store account signed in"),
        Err(error) => tracing::warn!(%error, "store sign-in callback refused"),
    }
    crate::main_window::raise(app);
}

/// Where a game's play time comes from (#169): tracked sessions, and what
/// each store or launcher reported.
#[tauri::command]
#[specta::specta]
pub async fn game_playtime(
    store: State<'_, caskade_core::db::LibraryStore>,
    game_id: String,
) -> Result<caskade_core::db::PlaytimeBreakdown, CommandError> {
    store.playtime_breakdown(&game_id).await.map_err(report)
}

/// The launchers discovery found on this computer, by discovery source.
async fn launchers<R: Runtime>(app: &AppHandle<R>) -> std::collections::BTreeSet<String> {
    let Some(controller) = app.try_state::<crate::discovery::DiscoveryController>() else {
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
#[tauri::command]
#[specta::specta]
pub async fn game_owned_releases<R: Runtime>(
    app: AppHandle<R>,
    accounts: State<'_, StoreAccounts>,
    game_id: String,
) -> Result<Vec<caskade_core::store_accounts::OwnedRelease>, CommandError> {
    let launchers = launchers(&app).await;
    accounts
        .owned_releases(&game_id, &launchers)
        .await
        .map_err(report)
}

/// Hands an owned release to its launcher to install. It shows as being
/// installed until discovery finds the copy.
#[tauri::command]
#[specta::specta]
pub async fn owned_install<R: Runtime>(
    app: AppHandle<R>,
    accounts: State<'_, StoreAccounts>,
    entry_id: String,
) -> Result<caskade_core::store_accounts::OwnedRelease, CommandError> {
    let launchers = launchers(&app).await;
    accounts
        .install(&entry_id, &launchers)
        .await
        .map_err(report)
}

/// Stops waiting for a launcher's install (cancelled or failed there).
#[tauri::command]
#[specta::specta]
pub fn owned_install_cancel(accounts: State<'_, StoreAccounts>, entry_id: String) {
    accounts.cancel_install(&entry_id);
}

/// The store accounts that own a game.
#[tauri::command]
#[specta::specta]
pub async fn game_ownership(
    accounts: State<'_, StoreAccounts>,
    game_id: String,
) -> Result<Vec<Ownership>, CommandError> {
    accounts.ownership(&game_id).await.map_err(report)
}

#[cfg(test)]
#[path = "store_accounts_tests.rs"]
mod tests;
