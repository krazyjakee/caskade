//! The UI half of plugins (#127): which UI modules to load, and calls from a
//! plugin's UI to its own backend half. Names and titles come in the
//! language in use, from the plugin's own catalog (#219).
use caskade_plugin_host::{PluginHost, PluginMessages, PluginStatus};
use serde::Serialize;
use specta::Type;
/// An enabled plugin with a UI half.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The `ui` module's `caskade-plugin://` URL.
    pub entry: String,
    /// `<ui stem>.css` next to the module, when there is one.
    pub style: Option<String>,
    /// Slots, views and commands from the manifest; the UI may only
    /// register these.
    pub contributes: Vec<UiContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiContribution {
    pub slot: String,
    pub id: String,
    pub title: Option<String>,
    /// Where it shows (#152): `desktop`, `console`, or empty for the
    /// slot's default.
    pub modes: Vec<String>,
}

/// Enabled plugins whose UI half the webview should load. No plugin code
/// runs here.
/// `url` is where the shell serves a plugin's file (its id, the file's path
/// in the plugin).
pub async fn plugins_ui(
    state: &PluginHost,
    url: impl Fn(&str, &std::path::Path) -> String,
) -> Result<Vec<UiPlugin>, String> {
    Ok(ui_plugins(state, url).await)
}

pub async fn ui_plugins(
    host: &PluginHost,
    url: impl Fn(&str, &std::path::Path) -> String,
) -> Vec<UiPlugin> {
    let mut plugins = Vec::new();
    for summary in host.list().await {
        if !matches!(summary.status, PluginStatus::Enabled | PluginStatus::Active) {
            continue;
        }
        let (Some(manifest), Some(folder)) = (
            host.translated_manifest(&summary.id),
            host.folder(&summary.id),
        ) else {
            continue;
        };
        let Some(ui) = &manifest.ui else {
            continue;
        };
        let style = ui.with_extension("css");
        let contributes = &manifest.contributes;
        plugins.push(UiPlugin {
            entry: url(&manifest.id, ui),
            style: folder
                .join(&style)
                .is_file()
                .then(|| url(&manifest.id, &style)),
            contributes: contributes
                .slots
                .iter()
                .map(|slot| UiContribution {
                    slot: slot.slot.clone(),
                    id: slot.id.clone(),
                    title: slot.title.clone(),
                    modes: slot.modes.clone(),
                })
                .chain(contributes.views.iter().map(|view| UiContribution {
                    slot: "views".into(),
                    id: view.id.clone(),
                    title: Some(view.title.clone()),
                    modes: view.modes.clone(),
                }))
                .chain(contributes.commands.iter().map(|command| UiContribution {
                    slot: "commands".into(),
                    id: command.id.clone(),
                    title: Some(command.title.clone()),
                    modes: Vec::new(),
                }))
                .collect(),
            id: manifest.id,
            name: manifest.name,
            version: manifest.version.to_string(),
        });
    }
    plugins
}

/// The plugin's own catalog for the language in use (#219), for `t` and
/// `<Trans>` in its UI half; no messages (English) without one. The webview
/// asks again after `LocaleChanged`.
pub async fn plugin_messages(
    state: &PluginHost,
    plugin_id: String,
) -> Result<PluginMessages, String> {
    state
        .messages(&plugin_id)
        .map_err(|error| error.to_string())
}

/// The UI opened one of the plugin's views: starts its backend when the
/// manifest lists `onView:<view>` (#125).
pub async fn plugin_view_opened(
    hooks: &std::sync::Arc<caskade_plugin_host::PluginHooks>,
    plugin_id: String,
    view_id: String,
) -> Result<(), String> {
    hooks.view_opened(&plugin_id, &view_id).await;
    Ok(())
}

/// `ctx.backend.call(method, payload)`: the plugin's `handle-request`
/// export, activating its backend on first use. Payloads cross as JSON.
pub async fn plugin_request(
    state: &PluginHost,
    plugin_id: String,
    method: String,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    crate::permissions::enforce("plugin_request", serde_json::Value::default)
        .map_err(|refusal| refusal.summary)?;
    let answer = state
        .request(&plugin_id, &method, &payload.to_string())
        .await
        .map_err(|error| error.to_string())?;
    Ok(serde_json::from_str(&answer).unwrap_or(serde_json::Value::String(answer)))
}
