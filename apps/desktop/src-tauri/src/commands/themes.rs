//! Themes IPC (#139, #140), forwarded to `caskade_app::commands::themes`;
//! stylesheets and previews are served on `caskade-theme:`.
pub use app::{ResolvedTheme, SAFE_MODE_NOTICE, SafeMode, is_theme_folder, plugin_folders};
// The webview's event, which the app sends as `caskade_app`'s.
pub use crate::events::ThemeReloaded;
use caskade_app::commands::themes as app;

use crate::error::CommandError;
use caskade_core::settings::SettingsService;
use caskade_core::themes::{ThemeInfo, ThemeService};
use caskade_plugin_host::PluginHost;
use std::path::Path;
use tauri::Manager;

/// Safe mode: the one tests manage, otherwise the process's.
fn safe_mode<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app::safe_mode(app.try_state::<SafeMode>().map(|mode| *mode))
}

/// Loads `folder` in place as a dev theme, telling the webview after every
/// change. Must run inside the async runtime. Returns its id.
pub fn load_dev<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    folder: &Path,
) -> Result<String, String> {
    app::load_dev(
        crate::events::sink(app.clone()),
        app.try_state::<ThemeService>().as_deref(),
        folder,
    )
}

/// The theme to apply for `id`, or for `core.appearance.theme` without one:
/// it, when it is installed and compiles, otherwise the default with a
/// notice. Safe mode always gives the default.
#[tauri::command]
#[specta::specta]
pub async fn theme_resolve<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    themes: tauri::State<'_, ThemeService>,
    id: Option<String>,
) -> Result<ResolvedTheme, CommandError> {
    app::theme_resolve(
        &themes,
        app.try_state::<SettingsService>().as_deref(),
        app.try_state::<PluginHost>().as_deref(),
        safe_mode(&app),
        crate::theme_scheme::url,
        id,
    )
    .await
}

/// Whether Caskade started in safe mode (`--safe-mode`,
/// `CASKADE_SAFE_MODE=1`): the default theme and no custom CSS.
#[tauri::command]
#[specta::specta]
pub fn appearance_safe_mode<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> bool {
    safe_mode(&app)
}

/// Every installed theme with its problems; previews come back as URLs.
#[tauri::command]
#[specta::specta]
pub async fn themes_list<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    themes: tauri::State<'_, ThemeService>,
) -> Result<Vec<ThemeInfo>, CommandError> {
    app::themes_list(
        &themes,
        app.try_state::<PluginHost>().as_deref(),
        crate::theme_scheme::url,
    )
    .await
}

/// The URL of a theme's compiled stylesheet, once it compiles.
#[tauri::command]
#[specta::specta]
pub fn theme_stylesheet(
    themes: tauri::State<'_, ThemeService>,
    id: String,
) -> Result<String, CommandError> {
    app::theme_stylesheet(&themes, crate::theme_scheme::url, &id)
}

/// Reads every theme folder again, e.g. after installing or editing one.
#[tauri::command]
#[specta::specta]
pub async fn themes_reload<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    themes: tauri::State<'_, ThemeService>,
) -> Result<Vec<ThemeInfo>, CommandError> {
    app::themes_reload(
        &themes,
        app.try_state::<PluginHost>().as_deref(),
        crate::theme_scheme::url,
    )
    .await
}

/// Appearance → "Load theme folder": development builds only.
#[tauri::command]
#[specta::specta]
pub async fn theme_load_unpacked<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    folder: String,
) -> Result<String, CommandError> {
    app::theme_load_unpacked(
        crate::events::sink(app.clone()),
        app.try_state::<ThemeService>().as_deref(),
        &folder,
    )
    .await
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;
