//! Installing an owned game through its store's launcher (#170).
//!
//! Each owned release of a game ([`StoreAccounts::owned_releases`]) says
//! whether it can be installed here and how: through the launcher whose
//! URI its provider names ([`AccountProvider::install_target`]), when
//! discovery found that launcher on this computer and the game runs on
//! this system. Installing opens that URI and marks the release as being
//! installed; it is done only when discovery finds the installed copy,
//! which joins the owned game ([`StoreAccounts::reconcile`]). Cancelling,
//! a failed handoff or an install the launcher abandons leave ownership as
//! it was. Uninstalling through the launcher makes discovery mark its copy
//! not installed; the owned release stays.

use super::{OwnedProduct, StoreAccounts};
use crate::{Error, Result, model::InstallState};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeSet;

/// How long a handoff waits for discovery before it's no longer shown as
/// under way.
const PENDING_MS: i64 = 24 * 60 * 60 * 1000;

/// What a provider installs through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    /// The discovery source of the launcher (`steam`, `gog`, `epic`): it
    /// must be found on this computer.
    pub launcher: String,
    /// The launcher's name for people.
    pub name: String,
    /// What opens the launcher's install for the product.
    pub uri: String,
}

/// Whether an owned release can be installed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[serde(rename = "OwnedInstallState")]
pub enum OwnedInstallState {
    /// A copy is installed (found by discovery).
    Installed,
    /// Install opens `launcher`.
    Available { launcher: String },
    /// Handed to `launcher`; waiting for discovery to find the copy.
    Installing { launcher: String, since_ms: i64 },
    /// `launcher` isn't on this computer.
    LauncherMissing { launcher: String },
    /// The store says it runs on `systems`, not this one.
    UnsupportedSystem { systems: Vec<String> },
    /// The account no longer owns it, or is disconnected or unsupported.
    Unavailable { reason: UnavailableReason },
    /// The store can't install games from Caskade.
    NotSupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "OwnedUnavailableReason")]
pub enum UnavailableReason {
    NoLongerOwned,
    Disconnected,
    NoProvider,
}

/// One owned release of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename = "OwnedRelease")]
pub struct OwnedRelease {
    pub entry_id: String,
    pub store_account_id: String,
    pub provider: String,
    /// The provider's name for people (`Steam`).
    pub provider_name: String,
    pub account_name: String,
    pub product_id: String,
    pub title: String,
    pub state: OwnedInstallState,
    /// When the account's library was last read in full: how fresh this is.
    pub refreshed_at_ms: Option<i64>,
}

/// This system, as stores name systems.
pub fn this_system() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "mac",
        _ => "linux",
    }
}

impl StoreAccounts {
    /// `game_id`'s owned releases and whether each can be installed here.
    /// `launchers` are the discovery sources whose launcher was found.
    pub async fn owned_releases(
        &self,
        game_id: &str,
        launchers: &BTreeSet<String>,
    ) -> Result<Vec<OwnedRelease>> {
        let entries = self.inner.store.entries_for_game(game_id).await?;
        let installed = entries
            .iter()
            .any(|e| matches!(e.state, InstallState::Installed | InstallState::Updating));
        let now = crate::now_ms();
        let mut out = Vec::new();
        for owner in self.inner.store.ownership(game_id).await? {
            let Some(entitlement) = self
                .inner
                .store
                .entitlements(&owner.store_account_id)
                .await?
                .into_iter()
                .find(|e| e.product.product_id == owner.product_id)
            else {
                continue;
            };
            let Some(entry_id) = entitlement.entry_id.clone() else {
                continue;
            };
            let account = self.account(&owner.store_account_id).await?;
            let provider = self.provider(&account.provider);
            let pending = crate::lock(&self.inner.installing)
                .get(&entry_id)
                .copied()
                .filter(|since| now - since < PENDING_MS);
            let target = provider
                .as_ref()
                .and_then(|p| p.install_target(&entitlement.product));
            let state = if installed {
                OwnedInstallState::Installed
            } else if provider.is_none() {
                OwnedInstallState::Unavailable {
                    reason: UnavailableReason::NoProvider,
                }
            } else if entitlement.lost_at_ms.is_some() {
                OwnedInstallState::Unavailable {
                    reason: UnavailableReason::NoLongerOwned,
                }
            } else if !account.enabled {
                OwnedInstallState::Unavailable {
                    reason: UnavailableReason::Disconnected,
                }
            } else {
                match target {
                    None => OwnedInstallState::NotSupported,
                    Some(target) => installable(&entitlement.product, &target, launchers, pending),
                }
            };
            out.push(OwnedRelease {
                entry_id,
                store_account_id: account.id.clone(),
                provider_name: provider
                    .as_ref()
                    .map(|p| p.capabilities().name)
                    .unwrap_or_else(|| account.provider.clone()),
                provider: account.provider.clone(),
                account_name: account.display_name.clone(),
                product_id: entitlement.product.product_id.clone(),
                title: entitlement.product.title.clone(),
                state,
                refreshed_at_ms: account.sync.last_ok_ms,
            });
        }
        if installed {
            // Done: what was handed off is installed now.
            let mut installing = crate::lock(&self.inner.installing);
            for release in &out {
                installing.remove(&release.entry_id);
            }
        }
        Ok(out)
    }

    /// Hands the owned release `entry_id` to its launcher. Nothing is
    /// claimed: the release shows as being installed until discovery finds
    /// the copy. `launchers` as for [`Self::owned_releases`].
    pub async fn install(
        &self,
        entry_id: &str,
        launchers: &BTreeSet<String>,
    ) -> Result<OwnedRelease> {
        let entry = self
            .inner
            .store
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| crate::db::missing("entry", entry_id))?;
        let release = self
            .owned_releases(&entry.game_id, launchers)
            .await?
            .into_iter()
            .find(|release| release.entry_id == entry_id)
            .ok_or_else(|| Error::Invalid(crate::tr!("that release isn't owned by an account")))?;
        let OwnedInstallState::Available { .. } = &release.state else {
            return Err(Error::Invalid(crate::tr!(
                "{title} can't be installed from here now",
                title = release.title
            )));
        };
        let account = self.account(&release.store_account_id).await?;
        let product = self
            .inner
            .store
            .entitlements(&account.id)
            .await?
            .into_iter()
            .find(|e| e.product.product_id == release.product_id)
            .map(|e| e.product)
            .ok_or_else(|| crate::db::missing("entitlement", &release.product_id))?;
        let target = self
            .provider(&account.provider)
            .and_then(|p| p.install_target(&product))
            .ok_or_else(|| Error::Invalid(crate::tr!("the store can't install from Caskade")))?;
        let opener = crate::read(&self.inner.opener).clone();
        opener(&target.uri)?;
        tracing::info!(entry = %entry_id, launcher = %target.launcher, "install handed to the launcher");
        crate::lock(&self.inner.installing).insert(entry_id.to_owned(), crate::now_ms());
        self.changed();
        self.owned_releases(&entry.game_id, launchers)
            .await?
            .into_iter()
            .find(|release| release.entry_id == entry_id)
            .ok_or_else(|| crate::db::missing("entry", entry_id))
    }

    /// Stops showing the release as being installed (the launcher's install
    /// was cancelled or failed there).
    pub fn cancel_install(&self, entry_id: &str) {
        if crate::lock(&self.inner.installing)
            .remove(entry_id)
            .is_some()
        {
            self.changed();
        }
    }

    /// How launcher URIs open: the system's handler, or a fake launcher in
    /// tests.
    pub fn set_opener(&self, opener: Opener) {
        *crate::write(&self.inner.opener) = opener;
    }
}

/// Opens a launcher URI.
pub type Opener = std::sync::Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;

/// The system's URI handler, with one URI argument and no shell.
pub fn system_opener() -> Opener {
    std::sync::Arc::new(|uri: &str| {
        let command =
            crate::launch::uri_command(uri).map_err(|error| Error::Invalid(error.to_string()))?;
        let mut opener = std::process::Command::new(&command.program);
        // The launcher starts outside Caskade's AppImage (#274).
        crate::managed::outside_own_appimage(&mut opener);
        let mut child = opener.args(&command.args).spawn().map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                crate::tr!("the launcher couldn't be opened: {error}", error = error),
            ))
        })?;
        // Reaped off the caller's thread; the launcher outlives it.
        std::thread::spawn(move || child.wait());
        Ok(())
    })
}

fn installable(
    product: &OwnedProduct,
    target: &InstallTarget,
    launchers: &BTreeSet<String>,
    pending: Option<i64>,
) -> OwnedInstallState {
    if !product.systems.is_empty() && !product.systems.iter().any(|s| s == this_system()) {
        return OwnedInstallState::UnsupportedSystem {
            systems: product.systems.clone(),
        };
    }
    if !launchers.contains(&target.launcher) {
        return OwnedInstallState::LauncherMissing {
            launcher: target.name.clone(),
        };
    }
    match pending {
        Some(since_ms) => OwnedInstallState::Installing {
            launcher: target.name.clone(),
            since_ms,
        },
        None => OwnedInstallState::Available {
            launcher: target.name.clone(),
        },
    }
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
