//! The UI half of plugins (#127): which UI modules to load, and calls from a
//! plugin's UI to its own backend half. Names and titles come in the
//! language in use, from the plugin's own catalog (#219).
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
