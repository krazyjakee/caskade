//! Themes (#139, #140) over `caskade_core::themes::ThemeService`: the
//! installed themes, the active one (from `core.appearance.theme`, with the
//! default as fallback), safe mode (#142) and dev themes watched in place.
//! The shell serves stylesheets and previews; `url` turns a theme's id and
//! a path in it into the shell's address for it.
use crate::error::{CommandError, refused, report};
use crate::events::{Event, Events};
use caskade_core::settings::{AppearanceSettings, SettingsService};
use caskade_core::themes::{DEV_DEBOUNCE, ThemeInfo, ThemeService};
use caskade_plugin_host::PluginHost;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
async fn sync_plugins(plugins: Option<&PluginHost>, themes: &ThemeService) {
    if let Some(host) = plugins {
        themes.set_plugin_folders(plugin_folders(host).await);
    }
}

fn with_urls(themes: Vec<ThemeInfo>, url: impl Fn(&str, &str) -> String) -> Vec<ThemeInfo> {
    themes
        .into_iter()
        .map(|mut theme| {
            theme.previews = theme
                .previews
                .iter()
                .map(|preview| url(&theme.id, preview))
                .collect();
            theme
        })
        .collect()
}

/// Safe mode (#142): `--safe-mode` or `CASKADE_SAFE_MODE=1` starts Caskade
/// with the default theme and without the user's CSS, to recover from a
/// broken look. A shell may set it (tests do); otherwise the process decides.
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

/// Whether Caskade runs in safe mode: `set` when the shell holds one,
/// otherwise the process's arguments and environment.
pub fn safe_mode(set: Option<SafeMode>) -> bool {
    static PROCESS: OnceLock<SafeMode> = OnceLock::new();
    match set {
        Some(mode) => mode.0,
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

/// The notice a theme carries in safe mode, in English.
pub const SAFE_MODE_NOTICE: &str =
    "Safe mode: Caskade started with the default theme and without your custom CSS.";

async fn selected(settings: Option<&SettingsService>) -> String {
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
/// notice. Safe mode (`safe`) always gives the default.
pub async fn theme_resolve(
    themes: &ThemeService,
    settings: Option<&SettingsService>,
    plugins: Option<&PluginHost>,
    safe: bool,
    url: impl Fn(&str, &str) -> String,
    id: Option<String>,
) -> Result<ResolvedTheme, CommandError> {
    let requested = match id {
        Some(id) => id,
        None => selected(settings).await,
    };
    sync_plugins(plugins, themes).await;
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
    themes.stylesheet(&active.id).map_err(report)?;
    Ok(ResolvedTheme {
        url: url(&active.id, "theme.css"),
        id: active.id,
        name: active.name,
        requested: active.requested,
        modes: active.modes,
        notice: active.notice,
        safe_mode: safe,
    })
}

/// Every installed theme with its problems; previews come back as URLs.
pub async fn themes_list(
    themes: &ThemeService,
    plugins: Option<&PluginHost>,
    url: impl Fn(&str, &str) -> String,
) -> Result<Vec<ThemeInfo>, CommandError> {
    sync_plugins(plugins, themes).await;
    Ok(with_urls(themes.list(), url))
}

/// The URL of a theme's compiled stylesheet, once it compiles.
pub fn theme_stylesheet(
    themes: &ThemeService,
    url: impl Fn(&str, &str) -> String,
    id: &str,
) -> Result<String, CommandError> {
    themes.stylesheet(id).map_err(report)?;
    Ok(url(id, "theme.css"))
}

/// Reads every theme folder again, e.g. after installing or editing one.
pub async fn themes_reload(
    themes: &ThemeService,
    plugins: Option<&PluginHost>,
    url: impl Fn(&str, &str) -> String,
) -> Result<Vec<ThemeInfo>, CommandError> {
    sync_plugins(plugins, themes).await;
    themes.reload();
    Ok(with_urls(themes.list(), url))
}

/// A dev theme (#140) was recompiled after its folder changed: re-apply it
/// when it is the active one. `error` when it no longer compiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeReloaded {
    pub theme: String,
    pub error: Option<String>,
}

/// Loads `folder` in place as a dev theme and tells `events` after every
/// change (`caskade dev <theme folder>`, Appearance's "Load theme folder" in
/// development builds). Must run inside the async runtime. Returns its id.
pub fn load_dev(
    events: Events,
    themes: Option<&ThemeService>,
    folder: &Path,
) -> Result<String, String> {
    let themes = themes.ok_or_else(|| caskade_core::tr!("themes are not ready"))?;
    themes
        .watch_dev(folder, DEV_DEBOUNCE, move |theme, error| {
            events.send(Event::ThemeReloaded(ThemeReloaded {
                theme: theme.to_owned(),
                error,
            }));
        })
        .map_err(|error| format!("{}: {error}", folder.display()))
}

/// Whether `folder` is a theme rather than a plugin.
pub fn is_theme_folder(folder: &Path) -> bool {
    folder.join("theme.json").is_file() && !folder.join("plugin.json").is_file()
}

/// Appearance → "Load theme folder": development builds only.
pub async fn theme_load_unpacked(
    events: Events,
    themes: Option<&ThemeService>,
    folder: &str,
) -> Result<String, CommandError> {
    if !cfg!(debug_assertions) {
        return Err(refused(caskade_core::tr!(
            "Loading a theme folder is only available in development builds; run `caskade dev <folder>`"
        )));
    }
    load_dev(events, themes, Path::new(folder)).map_err(CommandError::from)
}
