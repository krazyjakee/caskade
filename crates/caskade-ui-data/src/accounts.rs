//! Local accounts (#202, #105): the list and who is signed in, adding,
//! changing and deleting accounts, PINs and checking them, through the app
//! layer's commands as the webview's `api/accounts.ts` does. Every rule is
//! the core's; changes are made as the active account the app holds.
//! Settings → Accounts and My profile read it; sign-in and the age gate
//! (#106) can too.

use caskade_app::commands::accounts as commands;
use caskade_core::accounts::AccountService;
use caskade_core::error::ErrorReport;

/// The types the panels show.
pub use caskade_app::commands::accounts::AccountList;
pub use caskade_core::accounts::{
    Account, AccountAccess, AccountRole, AccountUpdate, AgeRestriction, NewAccount, Permissions,
    PinCheck,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What account panels read and change, over one app's accounts.
#[derive(Clone)]
pub struct Accounts {
    accounts: AccountService,
}

impl Accounts {
    pub fn new(accounts: AccountService) -> Self {
        Self { accounts }
    }

    /// The running app's.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(services.accounts.clone())
    }

    /// Every account (admins first, then by name), and who is signed in.
    pub async fn list(&self) -> Result<AccountList> {
        commands::accounts_list(&self.accounts).await
    }

    pub async fn create(&self, account: NewAccount) -> Result<Account> {
        commands::account_create(&self.accounts, account).await
    }

    pub async fn update(&self, id: &str, update: AccountUpdate) -> Result<Account> {
        commands::account_update(&self.accounts, id, update).await
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        commands::account_delete(&self.accounts, id).await
    }

    /// Signs `id` in (or checks its PIN, for an account changing its own).
    pub async fn sign_in(&self, id: &str, pin: Option<String>) -> Result<PinCheck> {
        commands::account_sign_in(&self.accounts, id, pin).await
    }

    /// Sets `id`'s PIN, or removes it with `None`.
    pub async fn set_pin(&self, id: &str, pin: Option<String>) -> Result<Account> {
        commands::account_set_pin(&self.accounts, id, pin).await
    }

    /// Signs out, to the account picker.
    pub async fn sign_out(&self) -> Result<()> {
        commands::account_sign_out(&self.accounts).await
    }

    /// Whether the signed-in account's age limit holds `game_id` back
    /// (#209): what an admin's PIN would approve.
    pub async fn age_restriction(&self, game_id: &str) -> Result<Option<AgeRestriction>> {
        caskade_app::commands::launch::game_age_check(&self.accounts, game_id).await
    }

    /// An admin's `pin` approves `game_id` for `account_id`: once, or for
    /// good with `permanent`.
    pub async fn approve_play(
        &self,
        game_id: &str,
        account_id: &str,
        pin: String,
        permanent: bool,
    ) -> Result<PinCheck> {
        caskade_app::commands::launch::game_age_approve(
            &self.accounts,
            game_id,
            account_id,
            pin,
            permanent,
        )
        .await
    }

    /// Whether `pin` is any admin's; never which.
    pub async fn verify_admin_pin(&self, pin: String) -> Result<PinCheck> {
        commands::account_verify_admin_pin(&self.accounts, pin).await
    }
}

impl PartialEq for Accounts {
    /// One app's: a panel's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
