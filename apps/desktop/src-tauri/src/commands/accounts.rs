//! Local accounts (#202), forwarded to `caskade_app::commands::accounts`.
//! Changes are made as the active account the app holds, never one the
//! webview names.
use caskade_app::commands::accounts as app;
use caskade_core::accounts::{Account, AccountService, AccountUpdate, NewAccount, PinCheck};
use tauri::State;

pub use app::AccountList;

use crate::error::CommandError;

#[tauri::command]
#[specta::specta]
pub async fn accounts_list(
    accounts: State<'_, AccountService>,
) -> Result<AccountList, CommandError> {
    app::accounts_list(&accounts).await
}

/// Makes an account; admins only.
#[tauri::command]
#[specta::specta]
pub async fn account_create(
    accounts: State<'_, AccountService>,
    account: NewAccount,
) -> Result<Account, CommandError> {
    app::account_create(&accounts, account).await
}

/// Changes an account's name and avatar (an admin or the account itself)
/// and its access (an admin).
#[tauri::command]
#[specta::specta]
pub async fn account_update(
    accounts: State<'_, AccountService>,
    id: String,
    update: AccountUpdate,
) -> Result<Account, CommandError> {
    app::account_update(&accounts, &id, update).await
}

/// Deletes an account and its collections; admins only.
#[tauri::command]
#[specta::specta]
pub async fn account_delete(
    accounts: State<'_, AccountService>,
    id: String,
) -> Result<bool, CommandError> {
    app::account_delete(&accounts, &id).await
}

/// Signs an account in; `pin` is needed when it has one.
#[tauri::command]
#[specta::specta]
pub async fn account_sign_in(
    accounts: State<'_, AccountService>,
    id: String,
    pin: Option<String>,
) -> Result<PinCheck, CommandError> {
    app::account_sign_in(&accounts, &id, pin).await
}

/// Async, so the bus emits on the runtime.
#[tauri::command]
#[specta::specta]
pub async fn account_sign_out(accounts: State<'_, AccountService>) -> Result<(), CommandError> {
    app::account_sign_out(&accounts).await
}

/// Sets an account's PIN (4–8 digits), or clears it with `null`.
#[tauri::command]
#[specta::specta]
pub async fn account_set_pin(
    accounts: State<'_, AccountService>,
    id: String,
    pin: Option<String>,
) -> Result<Account, CommandError> {
    app::account_set_pin(&accounts, &id, pin).await
}

/// Whether `pin` is any admin's; never which.
#[tauri::command]
#[specta::specta]
pub async fn account_verify_admin_pin(
    accounts: State<'_, AccountService>,
    pin: String,
) -> Result<PinCheck, CommandError> {
    app::account_verify_admin_pin(&accounts, pin).await
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
