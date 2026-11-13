//! Local accounts (#202): the list, changes, signing in and out, and PIN
//! checks. Every rule is the core's (`caskade_core::accounts`); changes are
//! made as the active account the app holds, never one the UI names.
use caskade_core::accounts::{Account, AccountService, AccountUpdate, NewAccount, PinCheck};

use crate::error::{CommandError, report};

/// Every account (admins first, then by name), and which one is signed in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountList {
    pub accounts: Vec<Account>,
    pub active_id: Option<String>,
}

/// Every account, and which one is signed in.
pub async fn accounts_list(accounts: &AccountService) -> Result<AccountList, CommandError> {
    Ok(AccountList {
        accounts: accounts.list().await.map_err(report)?,
        active_id: accounts.active_id(),
    })
}

/// Makes an account; admins only.
pub async fn account_create(
    accounts: &AccountService,
    account: NewAccount,
) -> Result<Account, CommandError> {
    accounts.create(account).await.map_err(report)
}

/// Changes an account's name and avatar (an admin or the account itself)
/// and its access (an admin).
pub async fn account_update(
    accounts: &AccountService,
    id: &str,
    update: AccountUpdate,
) -> Result<Account, CommandError> {
    accounts.update(id, update).await.map_err(report)
}

/// Deletes an account and its collections; admins only.
pub async fn account_delete(accounts: &AccountService, id: &str) -> Result<bool, CommandError> {
    accounts.delete(id).await.map_err(report)
}

/// Signs an account in; `pin` is needed when it has one.
pub async fn account_sign_in(
    accounts: &AccountService,
    id: &str,
    pin: Option<String>,
) -> Result<PinCheck, CommandError> {
    accounts.sign_in(id, pin).await.map_err(report)
}

/// Signs out. Async, so the bus emits on the runtime.
pub async fn account_sign_out(accounts: &AccountService) -> Result<(), CommandError> {
    accounts.sign_out();
    Ok(())
}

/// Sets an account's PIN (4–8 digits), or clears it with `None`.
pub async fn account_set_pin(
    accounts: &AccountService,
    id: &str,
    pin: Option<String>,
) -> Result<Account, CommandError> {
    accounts.set_pin(id, pin).await.map_err(report)
}

/// Whether `pin` is any admin's; never which.
pub async fn account_verify_admin_pin(
    accounts: &AccountService,
    pin: String,
) -> Result<PinCheck, CommandError> {
    accounts.verify_admin_pin(pin).await.map_err(report)
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
