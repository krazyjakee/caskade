//! A game on the user's other devices (#232, #234, #235, #108): its copies
//! there, the paired devices a copy can go to, playing it there and
//! sending it there, through the app layer's commands as the webview's
//! `api/sync.ts` does.

use caskade_app::commands::{copies, remote_play, sync};
use caskade_core::db::LibraryStore;
use caskade_core::devices::Devices;
use caskade_core::error::ErrorReport;
use caskade_core::remote_play::RemotePlay;
use caskade_core::sync::presence::Presence;
use caskade_core::transfer::Transfers;

/// The types flows show.
pub use caskade_core::devices::{DeviceLink, DeviceOs, PairedDevice};
pub use caskade_core::sync::copies::{CopyBlocked, PeerCopy};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What a game's other devices read and do, over one app's devices.
#[derive(Clone)]
pub struct OtherDevices {
    store: LibraryStore,
    devices: Devices,
    presence: Option<Presence>,
    play: RemotePlay,
    transfers: Transfers,
}

impl OtherDevices {
    pub fn new(
        store: LibraryStore,
        devices: Devices,
        presence: Option<Presence>,
        play: RemotePlay,
        transfers: Transfers,
    ) -> Self {
        Self {
            store,
            devices,
            presence,
            play,
            transfers,
        }
    }

    /// The running app's; `None` where it moves no files between devices.
    pub fn of(services: &caskade_app::start::Services) -> Option<Self> {
        Some(Self::new(
            services.store.clone(),
            services.devices.clone(),
            Some(services.presence.clone()),
            services.remote_play.clone(),
            services.transfers.clone()?,
        ))
    }

    /// `game_id`'s copies on paired devices, the best reached first.
    pub async fn copies(&self, game_id: &str) -> Result<Vec<PeerCopy>> {
        match &self.presence {
            Some(presence) => {
                sync::game_other_devices(&self.store, &self.devices, presence, game_id).await
            }
            None => {
                caskade_core::sync::copies::peer_copies(&self.store, &self.devices, None, game_id)
                    .await
                    .map_err(|error| ErrorReport::from_error(&error))
            }
        }
    }

    /// The paired devices.
    pub async fn paired(&self) -> Result<Vec<PairedDevice>> {
        Ok(sync::devices_status(&self.devices).await?.devices)
    }

    /// Sends `game_id`'s copy here to `device_id`; it asks there first.
    pub async fn send_to(&self, game_id: &str, device_id: &str) -> Result<()> {
        copies::game_send_to(
            &self.transfers,
            &self.store,
            game_id.to_owned(),
            device_id.to_owned(),
        )
        .await
    }

    /// Starts `game_id` on `device_id`, waking it first when it can.
    pub async fn play_on(&self, game_id: &str, device_id: &str) -> Result<()> {
        remote_play::game_play_on(&self.play, game_id, device_id).await
    }
}

impl PartialEq for OtherDevices {
    /// One set of devices per app: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
