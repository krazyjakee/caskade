//! Installing, updating and removing catalogue add-ons (#182) through the
//! plugin host and the theme service, not a second package manager.
//!
//! An install downloads the release the catalogue offers, checks it
//! against the catalogue's size and SHA-256, unpacks it into staging,
//! checks it is that add-on at that version and that it is valid, then
//! swaps it into place with a journal (`caskade_addons::package`).
//! A plugin that won't start, or a theme with problems, rolls back to the
//! previous version. A first plugin install is reviewed (its permissions)
//! before it is enabled; an update keeps the user's grant, so a plugin that
//! asks for more is left waiting for consent in Settings → Plugins, never
//! granted silently.
use crate::error::CommandError;
pub use app::*;
// The webview's event, which the app sends as `caskade_app`'s.
pub use crate::events::AddonProgress;
use caskade_addons::{
    AppMode, CatalogueService,
    package::{AddonRecords, UpdatePreference},
};
use caskade_app::commands::addon_packages as app;
use caskade_core::{db::LibraryStore, settings::SettingsService, themes::ThemeService};
use caskade_plugin_host::PluginHost;
use tauri::{Manager, Runtime, State};
/// The services an install or removal uses, from the app's state.
fn services<R: Runtime>(app: &tauri::AppHandle<R>) -> app::AddonServices<'_> {
    app::AddonServices {
        events: crate::events::sink(app.clone()),
        catalogue: app
            .try_state::<CatalogueService>()
            .map(|state| state.inner()),
        plugins: app.try_state::<PluginHost>().map(|state| state.inner()),
        themes: app.try_state::<ThemeService>().map(|state| state.inner()),
        bus: app
            .try_state::<caskade_core::bus::Bus>()
            .map(|state| state.inner()),
        settings: app
            .try_state::<SettingsService>()
            .map(|state| state.inner()),
    }
}

/// Installs or updates `id` to the newest release that fits. A first
/// plugin install comes back for review unless `accept`; a release that
/// failed a check that can't pass by retrying is refused again unless
/// `retry`.
#[tauri::command]
#[specta::specta]
// Tauri states and the call's arguments.
#[allow(clippy::too_many_arguments)]
pub async fn addons_install<R: Runtime>(
    app: tauri::AppHandle<R>,
    folders: State<'_, AddonFolders>,
    jobs: State<'_, AddonJobs>,
    store: State<'_, LibraryStore>,
    id: String,
    mode: AppMode,
    accept: bool,
    retry: bool,
) -> Result<AddonOutcome, CommandError> {
    app::addons_install(
        &services(&app),
        &folders,
        &jobs,
        &store,
        id,
        mode,
        accept,
        retry,
    )
    .await
}

/// Stops an install of `id` while it downloads; whether one was running.
#[tauri::command]
#[specta::specta]
pub fn addons_install_cancel(jobs: State<'_, AddonJobs>, id: String) -> bool {
    app::addons_install_cancel(&jobs, id)
}

/// Removes an installed add-on: a plugin, keeping its data and settings
/// when `keep_data`; a theme Caskade installed (the active one gives way to
/// the default theme). Built-in themes and dev folders stay.
#[tauri::command]
#[specta::specta]
pub async fn addons_uninstall<R: Runtime>(
    app: tauri::AppHandle<R>,
    folders: State<'_, AddonFolders>,
    store: State<'_, LibraryStore>,
    id: String,
    keep_data: bool,
) -> Result<(), CommandError> {
    app::addons_uninstall(&services(&app), &folders, &store, id, keep_data).await
}

/// What Caskade remembers about catalogue add-ons, and the update choice.
#[tauri::command]
#[specta::specta]
pub async fn addons_records(store: State<'_, LibraryStore>) -> Result<AddonRecords, CommandError> {
    app::addons_records(&store).await
}

/// Skips `version` of `id`'s updates (`null`: offer every update again).
#[tauri::command]
#[specta::specta]
pub async fn addons_skip_update(
    store: State<'_, LibraryStore>,
    id: String,
    version: Option<String>,
) -> Result<AddonRecords, CommandError> {
    app::addons_skip_update(&store, id, version).await
}

/// Whether Settings checks for add-on updates when it opens.
#[tauri::command]
#[specta::specta]
pub async fn addons_set_update_preference(
    store: State<'_, LibraryStore>,
    preference: UpdatePreference,
) -> Result<AddonRecords, CommandError> {
    app::addons_set_update_preference(&store, preference).await
}

#[cfg(test)]
#[path = "addon_smoke_tests.rs"]
mod smoke_tests;

#[cfg(test)]
#[path = "addon_packages_tests.rs"]
mod tests;
