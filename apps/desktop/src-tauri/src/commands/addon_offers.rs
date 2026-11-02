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
