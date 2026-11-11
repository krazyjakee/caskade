//! Settings for views (#103): the sections and their schemas, reading,
//! changing and resetting one, and which sections the signed-in account may
//! change. Every call goes through the app layer's commands
//! (`caskade_app::commands::settings`), so validation, revisions and change
//! events are the core's, as in the webview.

use caskade_app::commands::settings;
use caskade_core::{
    accounts::{AccountRole, AccountService},
    error::ErrorReport,
    settings::{SettingsSection, SettingsService, SettingsSnapshot},
};
use serde_json::Value;

type Result<T> = std::result::Result<T, ErrorReport>;

/// What Settings views read and change, over one app's services.
#[derive(Clone)]
pub struct Settings {
    settings: SettingsService,
    accounts: AccountService,
}

/// Which sections the signed-in account may change (#205).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editable {
    /// Every section: an admin, or no accounts at all.
    All,
    /// These sections' ids (the settings tabs'), as an admin gave them.
    Only(Vec<String>),
}

impl Editable {
    /// Whether the account may change `section`.
    pub fn allows(&self, section: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(sections) => sections.iter().any(|known| known == section),
        }
    }
}

impl Settings {
    pub fn new(settings: SettingsService, accounts: AccountService) -> Self {
        Self { settings, accounts }
    }

    /// The settings in a running app.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(services.settings.clone(), services.accounts.clone())
    }

    /// The accounts, for Settings → Accounts and My profile.
    pub fn accounts(&self) -> crate::accounts::Accounts {
        crate::accounts::Accounts::new(self.accounts.clone())
    }

    /// Every section, with its schema and defaults. A plugin's titles stay
    /// in its own words until the native UI hosts plugins (#108).
    pub fn sections(&self) -> Result<Vec<SettingsSection>> {
        settings::settings_sections(&self.settings, |_, _| {})
    }

    /// A section's value and revision.
    pub async fn get(&self, namespace: &str) -> Result<SettingsSnapshot> {
        settings::settings_get(&self.settings, namespace).await
    }

    /// Changes a section from its latest value: `change` gets what is saved
    /// now, so a second quick change never writes back an older snapshot.
    pub async fn update(
        &self,
        namespace: &str,
        change: impl FnOnce(Value) -> Value,
    ) -> Result<SettingsSnapshot> {
        let latest = self.get(namespace).await?;
        settings::settings_set(
            &self.settings,
            namespace,
            change(latest.value),
            Some(latest.revision),
        )
        .await
    }

    /// Puts a section back to its defaults.
    pub async fn reset(&self, namespace: &str) -> Result<SettingsSnapshot> {
        let latest = self.get(namespace).await?;
        settings::settings_reset(&self.settings, namespace, Some(latest.revision)).await
    }

    /// Which sections the signed-in account may change: an admin (or a
    /// copy without accounts) every one, a standard account those it was
    /// given. The core refuses the rest whatever a view shows.
    pub async fn editable(&self) -> Result<Editable> {
        let active = self
            .accounts
            .active()
            .await
            .map_err(caskade_app::error::report)?;
        Ok(match active {
            None => Editable::All,
            Some(account) if account.role == AccountRole::Admin => Editable::All,
            Some(account) => Editable::Only(account.permissions.settings),
        })
    }
}
