//! Add-ons the user's other devices have and this one doesn't (#245): what
//! Settings → Plugins offers to install, through the catalogue.
//!
//! Forwarded to `caskade_app::commands::addon_offers`, with the services
//! that may not be up yet.
use crate::error::CommandError;
use caskade_app::commands::addon_offers as app;
use caskade_core::{
    devices::Devices,
    emulation::{EmulationService, EmulatorSetup},
    sync::SyncService,
};
use tauri::{AppHandle, Manager, Runtime};

pub use app::{DeviceAddon, DeviceEmulator};

/// What paired devices have that this one lacks; empty while Plugins and
/// themes is This device only here, or before this device can sync.
#[tauri::command]
#[specta::specta]
pub async fn addons_from_devices<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<DeviceAddon>, CommandError> {
    let sync = app.try_state::<SyncService>();
    let devices = app.try_state::<Devices>();
    app::addons_from_devices(sync.as_deref(), devices.as_deref()).await
}

/// Preferred emulators from paired devices waiting here for their
/// emulator; empty while Preferred emulators is This device only.
#[tauri::command]
#[specta::specta]
pub async fn emulators_from_devices<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<DeviceEmulator>, CommandError> {
    let sync = app.try_state::<SyncService>();
    let devices = app.try_state::<Devices>();
    let emulation = app.try_state::<EmulationService>();
    let setup = app.try_state::<EmulatorSetup>();
    app::emulators_from_devices(
        sync.as_deref(),
        devices.as_deref(),
        emulation.as_deref(),
        setup.as_deref(),
    )
    .await
}

/// Keeps this device's emulator for a platform instead of the one another
/// device chose.
#[tauri::command]
#[specta::specta]
pub async fn emulator_offer_keep<R: Runtime>(
    app: AppHandle<R>,
    platform_id: String,
) -> Result<(), CommandError> {
    let sync = app.try_state::<SyncService>();
    app::emulator_offer_keep(sync.as_deref(), &platform_id).await
}

/// The games `device_id` has installed that can be copied here (#276), for
/// first-run setup from that device.
#[tauri::command]
#[specta::specta]
pub async fn device_games(
    store: tauri::State<'_, caskade_core::db::LibraryStore>,
    devices: tauri::State<'_, Devices>,
    device_id: String,
) -> Result<Vec<caskade_core::sync::offers::DeviceGame>, CommandError> {
    app::device_games(&store, &devices, &device_id).await
}
