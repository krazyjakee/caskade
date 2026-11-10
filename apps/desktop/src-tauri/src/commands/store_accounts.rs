//! Store accounts (#164, #165): providers, connecting and signing in,
//! connected accounts and what they own. `StoreAccountsChanged` follows
//! every change; refreshes run as Activity work. Sign-ins stay in the core
//! and the OS keychain: no command answers with a token.
//!
//! Forwarded to `caskade_app::commands::store_accounts`.
use crate::error::CommandError;
use caskade_app::commands::store_accounts as app;
use caskade_core::{
    accounts::AccountService,
    store_accounts::{
        AuthFlow, AuthInput, Ownership, ProviderCapabilities, RefreshOutcome, RemovalPreview,
        StoreAccount, StoreAccountSummary, StoreAccounts,
    },
};
use tauri::{AppHandle, Manager, Runtime, State};

pub use app::is_callback;

/// The store providers available this session.
#[tauri::command]
#[specta::specta]
pub fn store_providers(accounts: State<'_, StoreAccounts>) -> Vec<ProviderCapabilities> {
    app::store_providers(&accounts)
}

#[tauri::command]
#[specta::specta]
pub async fn store_accounts_list(
    accounts: State<'_, StoreAccounts>,
) -> Result<Vec<StoreAccountSummary>, CommandError> {
    app::store_accounts_list(&accounts).await
}

/// Refreshes what an account owns now.
#[tauri::command]
#[specta::specta]
pub async fn store_account_refresh(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<RefreshOutcome, CommandError> {
    app::store_account_refresh(&accounts, &id).await
}

/// Stops refreshing an account; its games stay.
#[tauri::command]
#[specta::specta]
pub async fn store_account_disconnect(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<(), CommandError> {
    app::store_account_disconnect(&accounts, &id).await
}

/// What removing an account would take out of the library.
#[tauri::command]
#[specta::specta]
pub async fn store_account_removal_preview(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<RemovalPreview, CommandError> {
    app::store_account_removal_preview(&accounts, &id).await
}

/// Removes an account and the games only it brought.
#[tauri::command]
#[specta::specta]
pub async fn store_account_remove(
    accounts: State<'_, StoreAccounts>,
    id: String,
) -> Result<RemovalPreview, CommandError> {
    app::store_account_remove(&accounts, &id).await
}

/// Cancels an account's refresh under way.
#[tauri::command]
#[specta::specta]
pub fn store_account_cancel_refresh(accounts: State<'_, StoreAccounts>, id: String) {
    app::store_account_cancel_refresh(&accounts, &id);
}

/// Sign-ins under way.
#[tauri::command]
#[specta::specta]
pub fn store_auth_flows(accounts: State<'_, StoreAccounts>) -> Vec<AuthFlow> {
    app::store_auth_flows(&accounts)
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
    app::store_connect_begin(&accounts, &provider, reconnect).await
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
    let signed_in = app.try_state::<AccountService>();
    app::store_connect_complete(&accounts, signed_in.as_deref(), &flow_id, &input).await
}

/// Abandons a sign-in.
#[tauri::command]
#[specta::specta]
pub fn store_connect_cancel(accounts: State<'_, StoreAccounts>, flow_id: String) {
    app::store_connect_cancel(&accounts, &flow_id);
}

/// Finishes the sign-in a callback link names. The link carries a code:
/// it is never logged.
pub async fn callback<R: Runtime>(app: &AppHandle<R>, link: tauri::Url) {
    let Some(accounts) = app.try_state::<StoreAccounts>() else {
        return;
    };
    let signed_in = app.try_state::<AccountService>();
    app::callback(&accounts, signed_in.as_deref(), &link).await;
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
    app::game_playtime(&store, &game_id).await
}

/// A game's owned releases and whether each can be installed here (#170).
#[tauri::command]
#[specta::specta]
pub async fn game_owned_releases<R: Runtime>(
    app: AppHandle<R>,
    accounts: State<'_, StoreAccounts>,
    game_id: String,
) -> Result<Vec<caskade_core::store_accounts::OwnedRelease>, CommandError> {
    let discovery = app.try_state::<crate::discovery::DiscoveryController>();
    app::game_owned_releases(&accounts, discovery.as_deref(), &game_id).await
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
    let discovery = app.try_state::<crate::discovery::DiscoveryController>();
    app::owned_install(&accounts, discovery.as_deref(), &entry_id).await
}

/// Stops waiting for a launcher's install (cancelled or failed there).
#[tauri::command]
#[specta::specta]
pub fn owned_install_cancel(accounts: State<'_, StoreAccounts>, entry_id: String) {
    app::owned_install_cancel(&accounts, &entry_id);
}

/// The store accounts that own a game.
#[tauri::command]
#[specta::specta]
pub async fn game_ownership(
    accounts: State<'_, StoreAccounts>,
    game_id: String,
) -> Result<Vec<Ownership>, CommandError> {
    app::game_ownership(&accounts, &game_id).await
}

#[cfg(test)]
#[path = "store_accounts_tests.rs"]
mod tests;
