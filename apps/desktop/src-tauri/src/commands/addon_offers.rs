//! Add-ons the user's other devices have and this one doesn't (#245): what
//! Settings → Plugins offers to install, through the catalogue.
use crate::error::{CommandError, report};
use caskade_core::{devices::Devices, sync::SyncService};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime};

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
#[tauri::command]
#[specta::specta]
pub async fn addons_from_devices<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<DeviceAddon>, CommandError> {
    let (Some(sync), Some(devices)) = (app.try_state::<SyncService>(), app.try_state::<Devices>())
    else {
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
#[tauri::command]
#[specta::specta]
pub async fn emulators_from_devices<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<DeviceEmulator>, CommandError> {
    let (Some(sync), Some(devices), Some(emulation)) = (
        app.try_state::<SyncService>(),
        app.try_state::<Devices>(),
        app.try_state::<caskade_core::emulation::EmulationService>(),
    ) else {
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
    let installable: std::collections::HashSet<String> =
        match app.try_state::<caskade_core::emulation::EmulatorSetup>() {
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
#[tauri::command]
#[specta::specta]
pub async fn emulator_offer_keep<R: Runtime>(
    app: AppHandle<R>,
    platform_id: String,
) -> Result<(), CommandError> {
    let Some(sync) = app.try_state::<SyncService>() else {
        return Ok(());
    };
    let Some(engine) = sync.engine() else {
        return Ok(());
    };
    caskade_core::sync::offers::keep_emulator(&engine, &platform_id)
        .await
        .map_err(report)
}
