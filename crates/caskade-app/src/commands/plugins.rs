//! Settings → Plugins IPC (#123): list, enable and disable plugins. The
//! UI half and backend calls are #127's.

use crate::error::{CommandError, ErrorCode, detailed, report};
use caskade_plugin_host::{HostError, PluginHost, PluginPreview, PluginSummary};
/// A plugin host's failure as a command's error. The host's text names
/// plugins by id, so it goes in the detail. Its core and I/O errors are
/// transparent: their causes don't include them, so they're reported here.
pub(crate) fn failure(error: HostError) -> CommandError {
    let detail = error.to_string();
    let (code, summary) = match error {
        HostError::Core(error) => return report(error),
        HostError::Io(error) => return report(error),
        HostError::Unknown(_) => (
            ErrorCode::Conflict,
            caskade_core::tr!("This plugin isn't installed any more."),
        ),
        HostError::Invalid { .. } => (
            ErrorCode::Invalid,
            caskade_core::tr!("This isn't a valid plugin. The details say what's wrong."),
        ),
        HostError::Disabled(_) => (
            ErrorCode::Invalid,
            caskade_core::tr!("This plugin is turned off."),
        ),
        HostError::NoBackend(_) => (
            ErrorCode::Invalid,
            caskade_core::tr!("This plugin has nothing to run."),
        ),
        HostError::NoUi(_) => (
            ErrorCode::Invalid,
            caskade_core::tr!("This plugin has no UI to show."),
        ),
        HostError::Undeclared { .. } | HostError::BadUi { .. } => (
            ErrorCode::Other,
            caskade_core::tr!("This plugin's UI can't be shown. The details say why."),
        ),
        HostError::Call { .. } | HostError::Plugin { .. } => (
            ErrorCode::Other,
            caskade_core::tr!("This plugin ran into a problem. The details say what."),
        ),
    };
    detailed(CommandError::new(code, summary), detail)
}

/// Every plugin in the plugins folders, read again from disk. No plugin
/// code runs.
pub async fn plugins_list(state: &PluginHost) -> Result<Vec<PluginSummary>, CommandError> {
    state.discover().await.map_err(failure)
}

/// Enables a plugin with the permissions its manifest asks for.
pub async fn plugin_enable(state: &PluginHost, id: String) -> Result<PluginSummary, CommandError> {
    crate::permissions::enforce("plugin_enable", serde_json::Value::default)?;
    state.set_enabled(&id, true).await.map_err(failure)
}

/// Disables a plugin and stops its backend.
pub async fn plugin_disable(state: &PluginHost, id: String) -> Result<PluginSummary, CommandError> {
    crate::permissions::enforce("plugin_disable", serde_json::Value::default)?;
    state.set_enabled(&id, false).await.map_err(failure)
}

/// What installing a plugin folder would add (name, version,
/// contributions, permissions). Copies and runs nothing.
pub async fn plugin_install_preview(
    state: &PluginHost,
    folder: String,
) -> Result<PluginPreview, CommandError> {
    crate::permissions::enforce("plugin_install_preview", serde_json::Value::default)?;
    state
        .preview_folder(std::path::Path::new(&folder))
        .await
        .map_err(failure)
}

/// Installs a reviewed plugin folder and grants what it asks for. The UI
/// calls this only after the user accepted the preview.
pub async fn plugin_install(
    state: &PluginHost,
    folder: String,
) -> Result<PluginSummary, CommandError> {
    crate::permissions::enforce("plugin_install", serde_json::Value::default)?;
    state
        .install_folder(std::path::Path::new(&folder))
        .await
        .map_err(failure)
}

/// Removes a plugin with its settings, stored data and grants (#128).
pub async fn plugin_uninstall(state: &PluginHost, id: String) -> Result<(), CommandError> {
    crate::permissions::enforce("plugin_uninstall", serde_json::Value::default)?;
    state.uninstall(&id).await.map_err(failure)
}
