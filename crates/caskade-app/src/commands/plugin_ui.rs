//! Plugins' UI. From API 0.2 (#109) the backend draws it: which plugins
//! draw contributions ([`drawn_plugins`]), drawing one
//! ([`plugin_ui_render`]) and what the user did with it
//! ([`plugin_ui_act`]). Names and titles come in the language in use, from
//! the plugin's own catalog (#219).
use caskade_plugin_host::{PluginHost, PluginStatus};
/// What a drawn contribution is drawn with (#109): its context, the tree it
/// draws, and what an action answers.
pub use caskade_plugin_ui::{Context as UiContext, Node as UiNode, Outcome as UiOutcome};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiContribution {
    pub slot: String,
    pub id: String,
    pub title: Option<String>,
    /// Where it shows (#152): `desktop`, `console`, or empty for the
    /// slot's default.
    pub modes: Vec<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
