//! Plugins' UI. From API 0.2 (#109) the backend draws it: which plugins
//! draw contributions ([`plugins_drawn`]), drawing one
//! ([`plugin_ui_render`]) and what the user did with it
//! ([`plugin_ui_act`]). A 0.1 plugin's JavaScript UI half (#127), which the
//! webview loads until the cutover: which modules to load, and calls from
//! it to its backend. Names and titles come in the language in use, from
//! the plugin's own catalog (#219).
use caskade_plugin_host::{PluginHost, PluginMessages, PluginStatus};
/// What a drawn contribution is drawn with (#109): its context, the tree it
/// draws, and what an action answers.
pub use caskade_plugin_ui::{Context as UiContext, Node as UiNode, Outcome as UiOutcome};
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
        plugins.push(UiPlugin {
            entry: url(&manifest.id, ui),
            style: folder
                .join(&style)
                .is_file()
                .then(|| url(&manifest.id, &style)),
            contributes: contributions(&manifest),
            id: manifest.id,
            name: manifest.name,
            version: manifest.version.to_string(),
        });
    }
    plugins
}

/// The slots, views and commands `manifest` declares.
fn contributions(manifest: &caskade_plugin_host::Manifest) -> Vec<UiContribution> {
    let contributes = &manifest.contributes;
    contributes
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
        .collect()
}

/// An enabled plugin whose backend draws its UI (API 0.2, #109).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawnPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    /// What it may draw: the slots, views and commands of its manifest.
    pub contributes: Vec<UiContribution>,
}

/// Enabled plugins whose backends draw their UI, with what they draw.
/// Nothing runs: a contribution is drawn when it shows.
pub async fn plugins_drawn(state: &PluginHost) -> Result<Vec<DrawnPlugin>, String> {
    Ok(drawn_plugins(state).await)
}

pub async fn drawn_plugins(host: &PluginHost) -> Vec<DrawnPlugin> {
    let mut plugins = Vec::new();
    for summary in host.list().await {
        if !matches!(summary.status, PluginStatus::Enabled | PluginStatus::Active) {
            continue;
        }
        let Some(manifest) = host.translated_manifest(&summary.id) else {
            continue;
        };
        if !caskade_plugin_host::manifest::draws_ui(&manifest.api) || manifest.backend.is_none() {
            continue;
        }
        plugins.push(DrawnPlugin {
            contributes: contributions(&manifest),
            id: manifest.id,
            name: manifest.name,
            version: manifest.version.to_string(),
        });
    }
    plugins
}

/// Draws contribution `id` of `slot` (`views` for a view) of plugin
/// `plugin_id`: Caskade's components as a tree, checked before it's shown.
/// `context` has the mode, the game in a game slot and the contribution's
/// state.
pub async fn plugin_ui_render(
    state: &PluginHost,
    plugin_id: String,
    slot: String,
    id: String,
    context: UiContext,
) -> Result<Vec<UiNode>, crate::error::CommandError> {
    state
        .ui_render(&plugin_id, &slot, &id, &context)
        .await
        .map_err(super::plugins::failure)
}

/// The user used `action` (a button's or a segmented control's, with its
/// `value`; `run` for a game action or a command) in contribution `id` of
/// `slot`. Answers the contribution's new state and a notice to show.
pub async fn plugin_ui_act(
    state: &PluginHost,
    plugin_id: String,
    slot: String,
    id: String,
    action: String,
    value: serde_json::Value,
    context: UiContext,
) -> Result<UiOutcome, crate::error::CommandError> {
    crate::permissions::enforce("plugin_ui_act", serde_json::Value::default)?;
    state
        .ui_act(&plugin_id, &slot, &id, &action, &value, &context)
        .await
        .map_err(super::plugins::failure)
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
