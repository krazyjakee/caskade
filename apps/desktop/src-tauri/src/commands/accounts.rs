//! Local accounts (#202): the list, changes, signing in and out, and PIN
//! checks. Every rule is the core's (`caskade_core::accounts`); changes are
//! made as the active account the app holds, never one the webview names.
use caskade_core::accounts::{Account, AccountService, AccountUpdate, NewAccount, PinCheck};
use tauri::State;

use crate::error::{CommandError, report};

/// Every account (admins first, then by name), and which one is signed in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountList {
    pub accounts: Vec<Account>,
    pub active_id: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn accounts_list(
    accounts: State<'_, AccountService>,
) -> Result<AccountList, CommandError> {
    Ok(AccountList {
        accounts: accounts.list().await.map_err(report)?,
        active_id: accounts.active_id(),
    })
}

/// Makes an account; admins only.
#[tauri::command]
#[specta::specta]
pub async fn account_create(
    accounts: State<'_, AccountService>,
    account: NewAccount,
) -> Result<Account, CommandError> {
    accounts.create(account).await.map_err(report)
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
    accounts.update(&id, update).await.map_err(report)
}

/// Deletes an account and its collections; admins only.
#[tauri::command]
#[specta::specta]
pub async fn account_delete(
    accounts: State<'_, AccountService>,
    id: String,
) -> Result<bool, CommandError> {
    accounts.delete(&id).await.map_err(report)
}

/// Signs an account in; `pin` is needed when it has one.
#[tauri::command]
#[specta::specta]
pub async fn account_sign_in(
    accounts: State<'_, AccountService>,
    id: String,
    pin: Option<String>,
) -> Result<PinCheck, CommandError> {
    accounts.sign_in(&id, pin).await.map_err(report)
}

/// Async, so the bus emits on the runtime.
#[tauri::command]
#[specta::specta]
pub async fn account_sign_out(accounts: State<'_, AccountService>) -> Result<(), CommandError> {
    accounts.sign_out();
    Ok(())
}

/// Sets an account's PIN (4–8 digits), or clears it with `null`.
#[tauri::command]
#[specta::specta]
pub async fn account_set_pin(
    accounts: State<'_, AccountService>,
    id: String,
    pin: Option<String>,
) -> Result<Account, CommandError> {
    accounts.set_pin(&id, pin).await.map_err(report)
}

/// Whether `pin` is any admin's; never which.
#[tauri::command]
#[specta::specta]
pub async fn account_verify_admin_pin(
    accounts: State<'_, AccountService>,
    pin: String,
) -> Result<PinCheck, CommandError> {
    accounts.verify_admin_pin(pin).await.map_err(report)
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
