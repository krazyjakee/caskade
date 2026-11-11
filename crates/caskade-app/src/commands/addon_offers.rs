//! Add-ons the user's other devices have and this one doesn't (#245): what
//! Settings → Plugins offers to install, through the catalogue.
use caskade_core::{
    db::LibraryStore,
    devices::Devices,
    emulation::{EmulationService, EmulatorSetup},
    sync::SyncService,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{CommandError, report};

/// An add-on on another device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAddon {
    pub device_id: String,
    /// The device's name, as paired.
    pub device_name: String,
    pub addon_id: String,
    pub version: Option<String>,
}

/// What paired devices have that this one lacks; empty while Plugins and
/// themes is This device only here, or before this device can sync.
pub async fn addons_from_devices(
    sync: Option<&SyncService>,
    devices: Option<&Devices>,
) -> Result<Vec<DeviceAddon>, CommandError> {
    let (Some(sync), Some(devices)) = (sync, devices) else {
        return Ok(Vec::new());
    };
    let Some(engine) = sync.engine() else {
        return Ok(Vec::new());
    };
    let offers = caskade_core::sync::offers::addon_offers(&engine)
        .await
        .map_err(report)?;
    let paired = devices.status().await.devices;
    Ok(offers
        .into_iter()
        // A device forgotten since is offered no longer.
        .filter_map(|offer| {
            let device = paired.iter().find(|device| device.id == offer.device_id)?;
            Some(DeviceAddon {
                device_name: device.name.clone(),
                device_id: offer.device_id,
                addon_id: offer.addon_id,
                version: offer.version,
            })
        })
        .collect())
}

/// A platform's emulator on another device that this device doesn't have
/// (#275): offered in Settings → Emulation, installed only when asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEmulator {
    pub platform_id: String,
    pub platform_name: String,
    pub definition_id: String,
    pub emulator_name: String,
    pub device_id: String,
    pub device_name: String,
    /// Caskade can install it on this system.
    pub installable: bool,
}

/// Preferred emulators from paired devices waiting here for their
/// emulator; empty while Preferred emulators is This device only.
pub async fn emulators_from_devices(
    sync: Option<&SyncService>,
    devices: Option<&Devices>,
    emulation: Option<&EmulationService>,
    setup: Option<&EmulatorSetup>,
) -> Result<Vec<DeviceEmulator>, CommandError> {
    let (Some(sync), Some(devices), Some(emulation)) = (sync, devices, emulation) else {
        return Ok(Vec::new());
    };
    let Some(engine) = sync.engine() else {
        return Ok(Vec::new());
    };
    let offers = caskade_core::sync::offers::emulator_offers(&engine)
        .await
        .map_err(report)?;
    if offers.is_empty() {
        return Ok(Vec::new());
    }
    let installable: std::collections::HashSet<String> = match setup {
        Some(setup) => setup
            .installable()
            .await
            .map_err(report)?
            .into_iter()
            .map(|item| item.definition_id)
            .collect(),
        None => Default::default(),
    };
    let catalog = emulation.catalog();
    let paired = devices.status().await.devices;
    Ok(offers
        .into_iter()
        .filter_map(|offer| {
            let device = paired.iter().find(|device| device.id == offer.device_id);
            let platform = catalog.platform(&offer.platform_id)?;
            let definition = catalog.emulator(&offer.definition_id)?;
            Some(DeviceEmulator {
                platform_name: platform.name.clone(),
                emulator_name: definition.name.clone(),
                installable: installable.contains(&offer.definition_id),
                // A device forgotten since still chose it; it has no name here.
                device_name: device.map(|device| device.name.clone()).unwrap_or_default(),
                platform_id: offer.platform_id,
                definition_id: offer.definition_id,
                device_id: offer.device_id,
            })
        })
        .collect())
}

/// Keeps this device's emulator for a platform instead of the one another
/// device chose.
pub async fn emulator_offer_keep(
    sync: Option<&SyncService>,
    platform_id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("emulator_offer_keep", serde_json::Value::default)?;
    let Some(sync) = sync else {
        return Ok(());
    };
    let Some(engine) = sync.engine() else {
        return Ok(());
    };
    caskade_core::sync::offers::keep_emulator(&engine, platform_id)
        .await
        .map_err(report)
}

/// The games `device_id` has installed that can be copied here (#276), for
/// first-run setup from that device.
pub async fn device_games(
    store: &LibraryStore,
    devices: &Devices,
    device_id: &str,
) -> Result<Vec<caskade_core::sync::offers::DeviceGame>, CommandError> {
    caskade_core::sync::offers::device_games(store, devices, device_id)
        .await
        .map_err(report)
}
