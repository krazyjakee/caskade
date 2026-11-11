//! Plugins for Settings → Plugins (#104): the installed ones with their
//! status, permissions and contributions; turning them on and off;
//! previewing a folder before it's installed, installing and uninstalling;
//! through the app layer's commands as the webview's `api/pluginManager.ts`
//! does. Changes arrive as `PluginsChanged` (`Key::Plugins`).

use caskade_app::commands::plugins as commands;
use caskade_core::error::ErrorReport;
pub use caskade_plugin_host::manifest::{LibraryAccess, ManifestError};
pub use caskade_plugin_host::{PluginHost, PluginPreview, PluginStatus, PluginSummary};

type Result<T> = std::result::Result<T, ErrorReport>;

/// The plugin host over one app's services, when it has one.
#[derive(Clone)]
pub struct Plugins {
    host: Option<PluginHost>,
}

fn unavailable() -> ErrorReport {
    ErrorReport::new(
        caskade_core::error::ErrorCode::Unavailable,
        caskade_core::tr!("Plugins are unavailable in this session."),
    )
}

impl Plugins {
    pub fn new(host: Option<PluginHost>) -> Self {
        Self { host }
    }

    /// The running app's plugins.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(services.plugins.clone())
    }

    fn host(&self) -> Result<&PluginHost> {
        self.host.as_ref().ok_or_else(unavailable)
    }

    /// Every plugin in the plugins folders, read again. No plugin code runs.
    pub async fn list(&self) -> Result<Vec<PluginSummary>> {
        commands::plugins_list(self.host()?).await
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginSummary> {
        let host = self.host()?;
        if enabled {
            commands::plugin_enable(host, id.to_owned()).await
        } else {
            commands::plugin_disable(host, id.to_owned()).await
        }
    }

    /// What installing `folder` would add. Copies and runs nothing.
    pub async fn preview(&self, folder: &str) -> Result<PluginPreview> {
        commands::plugin_install_preview(self.host()?, folder.to_owned()).await
    }

    /// Installs a reviewed folder and grants what it asks for.
    pub async fn install(&self, folder: &str) -> Result<PluginSummary> {
        commands::plugin_install(self.host()?, folder.to_owned()).await
    }

    pub async fn uninstall(&self, id: &str) -> Result<()> {
        commands::plugin_uninstall(self.host()?, id.to_owned()).await
    }
}

impl PartialEq for Plugins {
    /// One host per app: a view's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
