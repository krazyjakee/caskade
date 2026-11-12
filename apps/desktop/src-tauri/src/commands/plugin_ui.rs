//! Plugins' UI. From API 0.2 (#109) the backend draws it: which plugins
//! draw contributions ([`plugins_drawn`]), drawing one
//! ([`plugin_ui_render`]) and what the user did with it
//! ([`plugin_ui_act`]). A 0.1 plugin's JavaScript UI half (#127), which the
//! webview loads until the cutover: which modules to load, and calls from
//! it to its backend. Names and titles come in the language in use, from
//! the plugin's own catalog (#219).
pub use app::*;

/// The UI plugins, with their files at `caskade-plugin://` URLs.
pub async fn ui_plugins(host: &caskade_plugin_host::PluginHost) -> Vec<UiPlugin> {
    app::ui_plugins(host, crate::plugin_scheme::url).await
}
use caskade_app::commands::plugin_ui as app;
use caskade_plugin_host::{PluginHost, PluginMessages};
type State<'a> = tauri::State<'a, PluginHost>;

/// Enabled plugins whose UI half the webview should load. No plugin code
/// runs here.
#[tauri::command]
#[specta::specta]
pub async fn plugins_ui(state: State<'_>) -> Result<Vec<UiPlugin>, String> {
    app::plugins_ui(&state, crate::plugin_scheme::url).await
}

/// The plugin's own catalog for the language in use (#219), for `t` and
/// `<Trans>` in its UI half; no messages (English) without one. The webview
/// asks again after `LocaleChanged`.
#[tauri::command]
#[specta::specta]
pub async fn plugin_messages(
    state: State<'_>,
    plugin_id: String,
) -> Result<PluginMessages, String> {
    app::plugin_messages(&state, plugin_id).await
}

/// The UI opened one of the plugin's views: starts its backend when the
/// manifest lists `onView:<view>` (#125).
#[tauri::command]
#[specta::specta]
pub async fn plugin_view_opened(
    hooks: tauri::State<'_, std::sync::Arc<caskade_plugin_host::PluginHooks>>,
    plugin_id: String,
    view_id: String,
) -> Result<(), String> {
    app::plugin_view_opened(&hooks, plugin_id, view_id).await
}

/// `ctx.backend.call(method, payload)`: the plugin's `handle-request`
/// export, activating its backend on first use. Payloads cross as JSON.
#[tauri::command]
#[specta::specta]
pub async fn plugin_request(
    state: State<'_>,
    plugin_id: String,
    method: String,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    app::plugin_request(&state, plugin_id, method, payload).await
}

/// Enabled plugins whose backends draw their UI, with what they draw.
/// Nothing runs: a contribution is drawn when it shows.
#[tauri::command]
#[specta::specta]
pub async fn plugins_drawn(state: State<'_>) -> Result<Vec<DrawnPlugin>, String> {
    app::plugins_drawn(&state).await
}

/// Draws contribution `id` of `slot` (`views` for a view) of plugin
/// `plugin_id`: Caskade's components as a tree, checked before it's shown.
/// `context` has the mode, the game in a game slot and the contribution's
/// state.
#[tauri::command]
#[specta::specta]
pub async fn plugin_ui_render(
    state: State<'_>,
    plugin_id: String,
    slot: String,
    id: String,
    context: UiContext,
) -> Result<Vec<UiNode>, crate::error::CommandError> {
    app::plugin_ui_render(&state, plugin_id, slot, id, context).await
}

/// The user used `action` (a button's or a segmented control's, with its
/// `value`; `run` for a game action or a command) in contribution `id` of
/// `slot`. Answers the contribution's new state and a notice to show.
#[tauri::command]
#[specta::specta]
pub async fn plugin_ui_act(
    state: State<'_>,
    plugin_id: String,
    slot: String,
    id: String,
    action: String,
    value: serde_json::Value,
    context: UiContext,
) -> Result<UiOutcome, crate::error::CommandError> {
    app::plugin_ui_act(&state, plugin_id, slot, id, action, value, context).await
}
