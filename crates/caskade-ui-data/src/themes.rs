//! Themes for Settings → Appearance (#104): the installed ones with their
//! problems and previews, the one applied for the setting (with why it
//! isn't the one asked for), a rescan, and in development builds a theme
//! folder loaded in place; through the app layer's commands, as the
//! webview's `themes/api.ts` does.

use caskade_app::commands::themes::{self as commands, ResolvedTheme};
use caskade_app::events::Events;
use caskade_core::{
    error::ErrorReport,
    settings::SettingsService,
    themes::{ThemeInfo, ThemeService},
};
use caskade_plugin_host::PluginHost;

type Result<T> = std::result::Result<T, ErrorReport>;

/// Previews stay paths in their theme's folder: the view turns them into
/// its own addresses.
fn path(_: &str, path: &str) -> String {
    path.to_owned()
}

/// What Appearance reads and changes about themes, over one app's services.
#[derive(Clone)]
pub struct Themes {
    themes: ThemeService,
    settings: SettingsService,
    plugins: Option<PluginHost>,
    events: Events,
}

impl Themes {
    pub fn new(
        themes: ThemeService,
        settings: SettingsService,
        plugins: Option<PluginHost>,
        events: Events,
    ) -> Self {
        Self {
            themes,
            settings,
            plugins,
            events,
        }
    }

    /// The running app's themes, telling `events`.
    pub fn of(services: &caskade_app::start::Services, events: Events) -> Self {
        Self::new(
            services.themes.clone(),
            services.settings.clone(),
            services.plugins.clone(),
            events,
        )
    }

    /// Every installed theme with its problems; previews are paths in the
    /// theme's folder.
    pub async fn list(&self) -> Result<Vec<ThemeInfo>> {
        commands::themes_list(&self.themes, self.plugins.as_ref(), path).await
    }

    /// Reads every theme folder again.
    pub async fn reload(&self) -> Result<Vec<ThemeInfo>> {
        commands::themes_reload(&self.themes, self.plugins.as_ref(), path).await
    }

    /// The theme applied for the setting, and why it isn't the one asked
    /// for (missing, broken, or safe mode).
    pub async fn active(&self) -> Result<ResolvedTheme> {
        commands::theme_resolve(
            &self.themes,
            Some(&self.settings),
            self.plugins.as_ref(),
            commands::safe_mode(None),
            path,
            None,
        )
        .await
    }

    /// Whether this build loads a theme folder in place (development
    /// builds).
    pub fn loads_unpacked(&self) -> bool {
        cfg!(debug_assertions)
    }

    /// Loads `folder` in place as a dev theme, watched for changes; its id.
    pub async fn load_unpacked(&self, folder: &str) -> Result<String> {
        commands::theme_load_unpacked(self.events.clone(), Some(&self.themes), folder).await
    }

    /// A theme's file (a preview), for the view's resource loader.
    pub fn asset(&self, id: &str, path: &str) -> Option<Vec<u8>> {
        self.themes.asset(id, path)
    }
}
