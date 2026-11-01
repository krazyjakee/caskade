//! Themes IPC (#139, #140) over `caskade_core::themes::ThemeService`: the
//! installed themes, the active one (from `core.appearance.theme`, with the
//! default as fallback), safe mode (#142) and dev themes watched in place.
use caskade_core::settings::{AppearanceSettings, SettingsService};
use caskade_core::themes::{DEV_DEBOUNCE, ThemeInfo, ThemeService};
use caskade_plugin_host::PluginHost;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::Manager;
use tauri_specta::Event as _;

/// Theme folders of enabled plugins (`contributes.themes`).
pub async fn plugin_folders(host: &PluginHost) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for plugin in host.list().await {
        if !host.enabled(&plugin.id) {
            continue;
        }
        if let (Some(manifest), Some(folder)) = (host.manifest(&plugin.id), host.folder(&plugin.id))
        {
            folders.extend(
                manifest
                    .contributes
                    .themes
                    .iter()
                    .map(|path| folder.join(path)),
            );
        }
    }
    folders
}

/// Follows plugins being enabled or disabled since the last call.
async fn sync_plugins<R: tauri::Runtime>(app: &tauri::AppHandle<R>, themes: &ThemeService) {
    if let Some(host) = app.try_state::<PluginHost>() {
        themes.set_plugin_folders(plugin_folders(&host).await);
    }
}

fn with_urls(themes: Vec<ThemeInfo>) -> Vec<ThemeInfo> {
    themes
        .into_iter()
        .map(|mut theme| {
            theme.previews = theme
                .previews
                .iter()
                .map(|preview| crate::theme_scheme::url(&theme.id, preview))
                .collect();
            theme
        })
        .collect()
}

/// Safe mode (#142): `--safe-mode` or `CASKADE_SAFE_MODE=1` starts Caskade
/// with the default theme and without the user's CSS, to recover from a
/// broken look. Tests manage it as state; otherwise the process decides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SafeMode(pub bool);

impl SafeMode {
    pub fn from(args: &[String], var: impl Fn(&str) -> Option<String>) -> Self {
        Self(
            args.iter().skip(1).any(|arg| arg == "--safe-mode")
                || matches!(
                    var("CASKADE_SAFE_MODE").as_deref().map(str::trim),
                    Some("1" | "true")
                ),
        )
    }
}

fn safe_mode<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    static PROCESS: OnceLock<SafeMode> = OnceLock::new();
    match app.try_state::<SafeMode>() {
        Some(state) => state.0,
        None => {
            PROCESS
                .get_or_init(|| {
                    let mode = SafeMode::from(&std::env::args().collect::<Vec<_>>(), |name| {
                        std::env::var(name).ok()
                    });
                    if mode.0 {
                        tracing::info!("safe mode: default theme, no custom CSS");
                    }
                    mode
                })
                .0
        }
    }
}

/// The theme to apply, ready for the stylesheet `<link>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTheme {
    /// The requested theme, or `caskade.default` when it can't be applied
    /// (or in safe mode).
    pub id: String,
    pub name: String,
    /// The theme the settings name.
    pub requested: String,
    /// Colour schemes it provides, its default first.
    pub modes: Vec<String>,
    /// The compiled stylesheet on the `caskade-theme:` scheme.
    pub url: String,
    /// Why the requested theme isn't the one applied.
    pub notice: Option<String>,
    /// Started with `--safe-mode`: no user CSS either.
    pub safe_mode: bool,
}

#[cfg(test)]
pub const SAFE_MODE_NOTICE: &str =
    "Safe mode: Caskade started with the default theme and without your custom CSS.";

async fn selected(settings: Option<tauri::State<'_, SettingsService>>) -> String {
    match settings {
        Some(settings) => {
            settings
                .read_or_default::<AppearanceSettings>("core.appearance")
                .await
                .theme
        }
        None => default_theme(),
    }
}

fn default_theme() -> String {
    AppearanceSettings::default().theme
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
) -> Result<ResolvedTheme, String> {
    let requested = match id {
        Some(id) => id,
        None => selected(app.try_state::<SettingsService>()).await,
    };
    sync_plugins(&app, &themes).await;
    let safe = safe_mode(&app);
    let target = if safe {
        default_theme()
    } else {
        requested.clone()
    };
    let mut active = themes.resolve(&target);
    if safe {
        active.requested = requested;
        active.notice = Some(caskade_core::tr!(
            "Safe mode: Caskade started with the default theme and without your custom CSS."
        ));
    }
    themes
        .stylesheet(&active.id)
        .map_err(|error| error.to_string())?;
    Ok(ResolvedTheme {
        url: crate::theme_scheme::url(&active.id, "theme.css"),
        id: active.id,
        name: active.name,
        requested: active.requested,
        modes: active.modes,
        notice: active.notice,
        safe_mode: safe,
    })
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
) -> Result<Vec<ThemeInfo>, String> {
    sync_plugins(&app, &themes).await;
    Ok(with_urls(themes.list()))
}

/// The URL of a theme's compiled stylesheet, once it compiles.
#[tauri::command]
#[specta::specta]
pub fn theme_stylesheet(
    themes: tauri::State<'_, ThemeService>,
    id: String,
) -> Result<String, String> {
    themes.stylesheet(&id).map_err(|error| error.to_string())?;
    Ok(crate::theme_scheme::url(&id, "theme.css"))
}

/// Reads every theme folder again, e.g. after installing or editing one.
#[tauri::command]
#[specta::specta]
pub async fn themes_reload<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    themes: tauri::State<'_, ThemeService>,
) -> Result<Vec<ThemeInfo>, String> {
    sync_plugins(&app, &themes).await;
    themes.reload();
    Ok(with_urls(themes.list()))
}

/// A dev theme (#140) was recompiled after its folder changed: re-apply it
/// when it is the active one. `error` when it no longer compiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ThemeReloaded {
    pub theme: String,
    pub error: Option<String>,
}

/// Loads `folder` in place as a dev theme and tells the webview after every
/// change (`caskade dev <theme folder>`, Appearance's "Load theme folder" in
/// development builds). Must run inside the async runtime. Returns its id.
pub fn load_dev<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    folder: &Path,
) -> Result<String, String> {
    let themes = app
        .try_state::<ThemeService>()
        .ok_or_else(|| caskade_core::tr!("themes are not ready"))?;
    let emitter = app.clone();
    themes
        .watch_dev(folder, DEV_DEBOUNCE, move |theme, error| {
            let event = ThemeReloaded {
                theme: theme.to_owned(),
                error,
            };
            if let Err(error) = event.emit(&emitter) {
                tracing::warn!(%error, "could not tell the webview about a dev theme");
            }
        })
        .map_err(|error| format!("{}: {error}", folder.display()))
}

/// Whether `folder` is a theme rather than a plugin.
pub fn is_theme_folder(folder: &Path) -> bool {
    folder.join("theme.json").is_file() && !folder.join("plugin.json").is_file()
}

/// Appearance → "Load theme folder": development builds only.
#[tauri::command]
#[specta::specta]
pub async fn theme_load_unpacked<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    folder: String,
) -> Result<String, String> {
    if !cfg!(debug_assertions) {
        return Err(caskade_core::tr!(
            "Loading a theme folder is only available in development builds; run `caskade dev <folder>`"
        ));
    }
    load_dev(&app, Path::new(&folder))
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;
