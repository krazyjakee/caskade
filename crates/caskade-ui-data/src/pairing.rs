//! Pairing devices for flows (#226, #231, #108): the pairing in progress
//! and its code, confirming or refusing it, and a `caskade://pair` link
//! opened from outside, through the app layer's commands as the webview's
//! `api/sync.ts` does.

use caskade_app::commands::sync::{self, OpenedPairingLink};
use caskade_core::{
    accounts::{AccountRole, AccountService},
    devices::{Devices, DevicesStatus, PairedDevice, PairingLinkTarget},
    error::ErrorReport,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What pairing flows read and change, over one app's devices.
#[derive(Clone)]
pub struct Pairing {
    devices: Devices,
    accounts: AccountService,
    /// The link opened from outside; `None` where links don't reach (tests).
    opened: Option<&'static OpenedPairingLink>,
}

impl Pairing {
    pub fn new(
        devices: Devices,
        accounts: AccountService,
        opened: Option<&'static OpenedPairingLink>,
    ) -> Self {
        Self {
            devices,
            accounts,
            opened,
        }
    }

    /// The running app's.
    pub fn of(services: &'static caskade_app::start::Services) -> Self {
        Self::new(
            services.devices.clone(),
            services.accounts.clone(),
            Some(&services.opened_pairing_link),
        )
    }

    /// This device, the paired ones, and the pairing in progress.
    pub async fn status(&self) -> Result<DevicesStatus> {
        sync::devices_status(&self.devices).await
    }

    /// This user confirms the codes match.
    pub async fn confirm(&self, device_id: &str) -> Result<()> {
        sync::devices_confirm_pairing(&self.devices, device_id).await
    }

    /// Cancels a pairing, or dismisses a finished one.
    pub async fn cancel(&self, device_id: &str) -> Result<()> {
        sync::devices_cancel_pairing(&self.devices, device_id).await
    }

    /// The pairing link opened from outside, if one waits.
    pub async fn opened_link(&self) -> Result<Option<PairingLinkTarget>> {
        match self.opened {
            Some(opened) => sync::devices_opened_link(opened).await,
            None => Ok(None),
        }
    }

    /// Pairs with the device the opened link names.
    pub async fn accept_opened_link(&self) -> Result<Option<PairedDevice>> {
        match self.opened {
            Some(opened) => sync::devices_accept_opened_link(&self.devices, opened)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    /// Lets the opened link go.
    pub async fn dismiss_opened_link(&self) -> Result<()> {
        match self.opened {
            Some(opened) => sync::devices_dismiss_opened_link(opened).await,
            None => Ok(()),
        }
    }

    /// Whether the signed-in account may pair devices: an admin. The core
    /// refuses anyone else.
    pub async fn is_admin(&self) -> bool {
        match self.accounts.active().await {
            Ok(Some(account)) => account.role == AccountRole::Admin,
            Ok(None) | Err(_) => false,
        }
    }

    /// Hears every change to the devices (a pairing's state among them).
    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.devices.subscribe()
    }
}

impl PartialEq for Pairing {
    /// One set of devices per app: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
