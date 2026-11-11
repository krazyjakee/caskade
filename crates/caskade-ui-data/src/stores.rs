//! Store accounts for Settings → Store accounts (#104): the stores that
//! can be connected, the accounts and their health, sign-ins under way;
//! connecting, refreshing, disconnecting and removing, through the app
//! layer's commands as the webview's `api/storeAccounts.ts` does. Changes
//! arrive as `StoreAccountsChanged` (`Key::Stores`).

use caskade_app::commands::store_accounts as commands;
use caskade_core::{
    accounts::AccountService,
    error::ErrorReport,
    store_accounts::{
        AuthFlow, AuthInput, ProviderCapabilities, RemovalPreview, StoreAccountSummary,
        StoreAccounts,
    },
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What Settings → Store accounts shows at once.
#[derive(Clone, Debug, PartialEq)]
pub struct StoresSnapshot {
    pub providers: Vec<ProviderCapabilities>,
    pub accounts: Vec<StoreAccountSummary>,
    pub flows: Vec<AuthFlow>,
}

/// Store accounts over one app's service.
#[derive(Clone)]
pub struct Stores {
    accounts: StoreAccounts,
    /// Whose a new account is, when Caskade has accounts.
    signed_in: Option<AccountService>,
}

impl Stores {
    pub fn new(accounts: StoreAccounts, signed_in: Option<AccountService>) -> Self {
        Self {
            accounts,
            signed_in,
        }
    }

    /// The running app's store accounts.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(
            services.store_accounts.clone(),
            Some(services.accounts.clone()),
        )
    }

    /// The service itself, for tests that add a provider.
    pub fn service(&self) -> &StoreAccounts {
        &self.accounts
    }

    pub async fn snapshot(&self) -> Result<StoresSnapshot> {
        Ok(StoresSnapshot {
            providers: commands::store_providers(&self.accounts),
            accounts: commands::store_accounts_list(&self.accounts).await?,
            flows: commands::store_auth_flows(&self.accounts),
        })
    }

    pub async fn refresh(&self, id: &str) -> Result<()> {
        commands::store_account_refresh(&self.accounts, id)
            .await
            .map(|_| ())
    }

    pub fn cancel_refresh(&self, id: &str) {
        commands::store_account_cancel_refresh(&self.accounts, id);
    }

    pub async fn disconnect(&self, id: &str) -> Result<()> {
        commands::store_account_disconnect(&self.accounts, id).await
    }

    pub async fn removal_preview(&self, id: &str) -> Result<RemovalPreview> {
        commands::store_account_removal_preview(&self.accounts, id).await
    }

    pub async fn remove(&self, id: &str) -> Result<RemovalPreview> {
        commands::store_account_remove(&self.accounts, id).await
    }

    /// Starts signing in to `provider`, or `reconnect` again.
    pub async fn begin(&self, provider: &str, reconnect: Option<String>) -> Result<AuthFlow> {
        commands::store_connect_begin(&self.accounts, provider, reconnect).await
    }

    /// Finishes a sign-in; the account's library refreshes in the
    /// background.
    pub async fn complete(&self, flow_id: &str, input: AuthInput) -> Result<()> {
        commands::store_connect_complete(&self.accounts, self.signed_in.as_ref(), flow_id, &input)
            .await
            .map(|_| ())
    }

    pub fn cancel(&self, flow_id: &str) {
        commands::store_connect_cancel(&self.accounts, flow_id);
    }
}
