//! The add-on catalogue (#181): its entries with what each means here
//! (installed, update, compatible), its status, a refresh, and its images
//! through the artwork cache, so nothing in the catalogue runs in the
//! webview. Plugins and themes only.
use crate::error::CommandError;
pub use app::*;
use caskade_addons::{AppMode, CatalogueService};
use caskade_app::commands::addons as app;
use caskade_core::{artwork::CachedArtwork, themes::ThemeService};
use caskade_plugin_host::PluginHost;
use tauri::{Manager, Runtime, State};
/// The catalogue as last fetched (offline too), with what each entry means
/// here in `mode`. Fetches nothing.
#[tauri::command]
#[specta::specta]
pub async fn addons_catalogue<R: Runtime>(
    app: tauri::AppHandle<R>,
    mode: AppMode,
) -> Result<CatalogueView, CommandError> {
    app::addons_catalogue(
        app.try_state::<CatalogueService>().as_deref(),
        app.try_state::<PluginHost>().as_deref(),
        app.try_state::<ThemeService>().as_deref(),
        mode,
    )
    .await
}

/// Fetches the catalogue again; on failure the last good one stays and
/// the status says why.
#[tauri::command]
#[specta::specta]
pub async fn addons_catalogue_refresh<R: Runtime>(
    app: tauri::AppHandle<R>,
    mode: AppMode,
) -> Result<CatalogueView, CommandError> {
    app::addons_catalogue_refresh(
        app.try_state::<CatalogueService>().as_deref(),
        app.try_state::<PluginHost>().as_deref(),
        app.try_state::<ThemeService>().as_deref(),
        mode,
    )
    .await
}

/// One of an entry's screenshots, fetched and re-encoded by the artwork
/// cache (an image, never markup or script). Only the catalogue's own
/// images can be asked for; `null` when it isn't there or can't load.
#[tauri::command]
#[specta::specta]
pub async fn addons_media<R: Runtime>(
    app: tauri::AppHandle<R>,
    media: State<'_, AddonMedia>,
    id: String,
    index: u32,
) -> Result<Option<CachedArtwork>, CommandError> {
    app::addons_media(
        app.try_state::<CatalogueService>().as_deref(),
        &media,
        &id,
        index,
    )
    .await
}

#[cfg(test)]
#[path = "addons_tests.rs"]
mod tests;
