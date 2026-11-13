//! Dev plugins (#131): in debug builds, `caskade dev <folder>` (also from a
//! second launch, forwarded by single-instance) and Settings → Plugins "Load
//! unpacked" load a plugin folder in place, without review, and watch it.
//! Release builds ignore both (CSK-06). Forwarded to
//! `caskade_app::dev_plugins`, with the services that may not be up yet.
use caskade_app::dev_plugins as app;
use caskade_core::themes::ThemeService;
use caskade_plugin_host::PluginHost;
use std::path::{Path, PathBuf};
use tauri::Manager;

pub use app::DevPlugins;

/// Loads `folder` as a dev plugin unless it is already watched.
pub async fn load<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    folder: &Path,
) -> Result<(), String> {
    let (themes, host, bus, dev_plugins) = (
        app.try_state::<ThemeService>(),
        app.try_state::<PluginHost>(),
        app.try_state::<caskade_core::bus::Bus>(),
        app.try_state::<DevPlugins>(),
    );
    app::load(
        app::DevServices {
            events: crate::events::sink(app.clone()),
            themes: themes.as_deref(),
            host: host.as_deref(),
            bus: bus.as_deref(),
            dev_plugins: dev_plugins.as_deref(),
        },
        folder,
    )
    .await
}

/// [`load`] on the async runtime, logging a failure.
pub fn load_in_background<R: tauri::Runtime>(app: &tauri::AppHandle<R>, folder: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = load(&app, &folder).await {
            tracing::warn!(%error, "could not load the dev plugin");
        }
    });
}

/// Settings → Plugins "Load unpacked": debug builds only.
#[tauri::command]
#[specta::specta]
pub async fn plugin_load_unpacked(
    app: tauri::AppHandle,
    folder: String,
) -> Result<(), crate::error::CommandError> {
    let (themes, host, bus, dev_plugins) = (
        app.try_state::<ThemeService>(),
        app.try_state::<PluginHost>(),
        app.try_state::<caskade_core::bus::Bus>(),
        app.try_state::<DevPlugins>(),
    );
    app::plugin_load_unpacked(
        app::DevServices {
            events: crate::events::sink(app.clone()),
            themes: themes.as_deref(),
            host: host.as_deref(),
            bus: bus.as_deref(),
            dev_plugins: dev_plugins.as_deref(),
        },
        &folder,
    )
    .await
}
