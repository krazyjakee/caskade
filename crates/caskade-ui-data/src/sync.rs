//! Settings → Devices (#231, #105): this device, the paired ones, pairing
//! nearby or by link, syncing now, forgetting, the sync key, and the
//! copies under way, through the app layer's commands as the webview's
//! `api/sync.ts` does. The core refuses what the signed-in account may not
//! do; the panel shows only an admin the controls.

use caskade_app::commands::{copies, sync};
use caskade_core::accounts::{AccountRole, AccountService};
use caskade_core::devices::Devices;
use caskade_core::error::ErrorReport;
use caskade_core::sync::SyncService;
use caskade_core::transfer::Transfers;

/// The types the panel shows.
pub use caskade_app::commands::copies::{TransferDirection, TransferState, TransferView};
pub use caskade_core::devices::{
    DeviceKind, DeviceLink, DeviceOs, DevicesStatus, IdentityStatus, NearbyDevice, PairedDevice,
    Pairing as Pair, PairingLinkInfo,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What Settings → Devices reads and does, over one app's devices.
#[derive(Clone)]
pub struct DeviceSync {
    devices: Devices,
    sync: Option<SyncService>,
    transfers: Option<Transfers>,
    accounts: AccountService,
}

impl DeviceSync {
    pub fn new(
        devices: Devices,
        sync: Option<SyncService>,
        transfers: Option<Transfers>,
        accounts: AccountService,
    ) -> Self {
        Self {
            devices,
            sync,
            transfers,
            accounts,
        }
    }

    /// The running app's.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(
            services.devices.clone(),
            Some(services.sync.clone()),
            services.transfers.clone(),
            services.accounts.clone(),
        )
    }

    /// This device, the paired ones, the nearby ones and a pairing under way.
    pub async fn status(&self) -> Result<DevicesStatus> {
        sync::devices_status(&self.devices).await
    }

    pub async fn rename(&self, name: &str) -> Result<()> {
        sync::devices_rename(&self.devices, name).await
    }

    pub async fn set_kind(&self, kind: DeviceKind) -> Result<()> {
        sync::devices_set_kind(&self.devices, kind).await
    }

    /// Syncs with `device_id` now, instead of at the next poll.
    pub async fn sync_now(&self, device_id: &str) -> Result<()> {
        match &self.sync {
            Some(service) => sync::devices_sync_now(service, device_id).await,
            None => Ok(()),
        }
    }

    pub async fn forget(&self, device_id: &str) -> Result<()> {
        sync::devices_forget(&self.devices, device_id).await
    }

    /// Starts pairing with a nearby device: both show the code (the
    /// pairing host asks to confirm it).
    pub async fn pair_nearby(&self, device_id: &str) -> Result<Pair> {
        sync::devices_pair_nearby(&self.devices, device_id).await
    }

    /// A link that pairs another device with this one: once, for ten
    /// minutes.
    pub async fn pairing_link(&self) -> Result<PairingLinkInfo> {
        sync::devices_pairing_link(&self.devices).await
    }

    /// Pairs with the device a pasted link names.
    pub async fn accept_pairing_link(&self, link: &str) -> Result<PairedDevice> {
        sync::devices_accept_pairing_link(&self.devices, link).await
    }

    /// A new sync key, after the keychain lost the old one.
    pub async fn create_identity(&self) -> Result<()> {
        sync::devices_create_identity(&self.devices).await
    }

    /// Reads the keychain again after it didn't answer.
    pub async fn retry_identity(&self) -> Result<()> {
        sync::devices_retry_identity(&self.devices).await
    }

    /// Copies going on to and from this device.
    pub async fn transfers(&self) -> Result<Vec<TransferView>> {
        match &self.transfers {
            Some(transfers) => copies::devices_transfers(transfers, &self.devices).await,
            None => Ok(Vec::new()),
        }
    }

    /// Whether the signed-in account is an admin (or no account exists).
    pub async fn is_admin(&self) -> bool {
        match self.accounts.active().await {
            Ok(Some(account)) => account.role == AccountRole::Admin,
            Ok(None) => self
                .accounts
                .list()
                .await
                .map(|accounts| accounts.is_empty())
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Hears every change to the devices.
    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.devices.subscribe()
    }
}

impl PartialEq for DeviceSync {
    /// One set of devices per app: a panel's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
