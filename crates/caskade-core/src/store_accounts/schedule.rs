//! Background refreshes (#165): `core.storeAccounts`, off until the user
//! turns them on. Each connected account is refreshed an interval after
//! its last attempt; one that failed is tried again after 30 minutes,
//! doubling, four times, then an interval later; a store's Retry-After is
//! always waited out. They share the account's single-flight refresh with
//! the Refresh button, and failures are only recorded on the account,
//! never announced.

use super::{AuthKind, StoreAccount, StoreAccounts};
use crate::settings::{SectionDefinition, SettingsService};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const NAMESPACE: &str = "core.storeAccounts";
const HOUR_MS: i64 = 60 * 60 * 1000;
const RETRY_MS: i64 = HOUR_MS / 2;
const RETRIES: u32 = 4;
const SETTLE: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_secs(5 * 60);

/// Settings → Store accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoreAccountSettings {
    #[schemars(title = "Refresh libraries in the background")]
    /// Look for newly bought games without being asked.
    pub background_refresh: bool,
    #[schemars(title = "Refresh every (hours)", range(min = 1, max = 168))]
    pub interval_hours: u32,
    #[schemars(title = "Import play time")]
    /// Play time the stores report (#169): never, for new games only, or
    /// on every refresh.
    pub playtime_import: crate::db::PlaytimeImport,
}

impl Default for StoreAccountSettings {
    fn default() -> Self {
        Self {
            background_refresh: false,
            interval_hours: 24,
            playtime_import: crate::db::PlaytimeImport::Always,
        }
    }
}

/// Registers `core.storeAccounts`.
pub fn register_settings(settings: &SettingsService) -> crate::settings::Result<()> {
    settings.register(
        SectionDefinition::typed::<StoreAccountSettings>(NAMESPACE, 2)?
            // Version 2 imports play time (#169).
            .migration(1, |mut value| {
                if let Some(object) = value.as_object_mut() {
                    object.insert("playtimeImport".into(), "always".into());
                }
                Ok(value)
            })?,
    )
}

/// When `account` is next due for a background refresh.
pub(super) fn due(account: &StoreAccount, interval_ms: i64) -> i64 {
    let sync = &account.sync;
    let after = |from: Option<i64>, wait: i64| from.map_or(0, |at| at + wait);
    let next = match sync.failures {
        0 => after(sync.last_attempt_ms.or(sync.last_ok_ms), interval_ms),
        n if n > RETRIES => after(sync.last_attempt_ms, interval_ms),
        n => after(sync.last_attempt_ms, (RETRY_MS << (n - 1)).min(interval_ms)),
    };
    next.max(sync.retry_at_ms.unwrap_or(0))
}

impl StoreAccounts {
    /// Reads `core.storeAccounts` from `settings`; registers it first.
    pub fn set_settings(&self, settings: SettingsService) {
        if let Err(error) = register_settings(&settings) {
            tracing::error!(%error, "store account settings unavailable");
        }
        let _ = self.inner.settings.set(settings);
    }

    pub(super) async fn settings(&self) -> StoreAccountSettings {
        let Some(settings) = self.inner.settings.get() else {
            return StoreAccountSettings::default();
        };
        settings.read_or_default(NAMESPACE).await
    }

    /// Refreshes the connected accounts due at `now`, when background
    /// refreshes are on. Returns the ids it refreshed (or tried to).
    pub async fn run_due(&self, now: i64) -> Vec<String> {
        let settings = self.settings().await;
        if !settings.background_refresh {
            return Vec::new();
        }
        let interval = i64::from(settings.interval_hours.max(1)) * HOUR_MS;
        let accounts = match self.inner.store.store_accounts().await {
            Ok(accounts) => accounts,
            Err(error) => {
                tracing::warn!(%error, "store accounts unavailable");
                return Vec::new();
            }
        };
        let mut tried = Vec::new();
        for account in accounts {
            let Some(provider) = self.provider(&account.provider) else {
                continue;
            };
            if !provider.capabilities().library {
                continue;
            }
            // Expired sign-ins wait for the user; nothing nags.
            let signed_in = provider.capabilities().auth == AuthKind::Launcher
                || self.inner.credentials.get(&account.id).await.is_some();
            if !account.enabled || !signed_in || due(&account, interval) > now {
                continue;
            }
            if let Err(error) = self
                .refresh(&account.id, &super::CancellationToken::new())
                .await
            {
                tracing::info!(account = %account.id, %error, "background refresh failed");
            }
            tried.push(account.id);
        }
        tried
    }

    /// Background refreshes, until the process ends.
    pub(super) async fn schedule(self) {
        tokio::time::sleep(SETTLE).await;
        loop {
            let _ = self.run_due(crate::now_ms()).await;
            tokio::time::sleep(TICK).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_accounts::SyncState;

    fn account(sync: SyncState) -> StoreAccount {
        StoreAccount {
            id: "a".into(),
            provider: "p".into(),
            external_id: "x".into(),
            display_name: "X".into(),
            owner_id: None,
            enabled: true,
            sync,
            created_at_ms: 0,
        }
    }

    #[test]
    fn refreshes_wait_an_interval_back_off_and_honour_retry_after() {
        let day = 24 * HOUR_MS;
        assert_eq!(due(&account(SyncState::default()), day), 0);
        let ok = SyncState {
            last_ok_ms: Some(10 * day),
            last_attempt_ms: Some(10 * day),
            ..SyncState::default()
        };
        assert_eq!(due(&account(ok.clone()), day), 11 * day);
        let waits: Vec<i64> = (1..=6)
            .map(|failures| {
                due(
                    &account(SyncState {
                        failures,
                        ..ok.clone()
                    }),
                    day,
                ) - 10 * day
            })
            .collect();
        assert_eq!(
            waits,
            [RETRY_MS, 2 * RETRY_MS, 4 * RETRY_MS, 8 * RETRY_MS, day, day]
        );
        let limited = SyncState {
            failures: 1,
            retry_at_ms: Some(12 * day),
            ..ok
        };
        assert_eq!(due(&account(limited), day), 12 * day);
    }
}
