//! The add-on catalogue (#181): its entries with what each means here
//! (installed, update, compatible), its status, a refresh, and its images
//! through the artwork cache, so nothing in the catalogue runs in the
//! webview. Plugins and themes only.
use crate::error::CommandError;
pub use app::*;
use caskade_addons::{AppMode, CatalogueService, InstalledAddon};
use caskade_app::commands::addons as app;
use caskade_core::{artwork::CachedArtwork, themes::ThemeService};
use caskade_plugin_host::PluginHost;
use tauri::{Manager, Runtime, State};
/// The catalogue client (`caskade_app`'s), checked against the release key
/// the updater uses unless the environment names another.
pub fn service(
    app: &tauri::AppHandle<impl Runtime>,
    dir: std::path::PathBuf,
) -> Option<CatalogueService> {
    app::service(dir, release_key(app))
}

/// The key Caskade's releases are signed with: the updater's.
pub fn release_key(app: &tauri::AppHandle<impl Runtime>) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|key| key.as_str())
        .map(str::to_owned)
}

/// What is installed: plugins and themes.
pub async fn installed<R: Runtime>(app: &tauri::AppHandle<R>) -> Vec<InstalledAddon> {
    app::installed(
        app.try_state::<PluginHost>().as_deref(),
        app.try_state::<ThemeService>().as_deref(),
    )
    .await
}

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
