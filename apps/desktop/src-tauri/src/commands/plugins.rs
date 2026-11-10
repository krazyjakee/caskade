//! Settings → Plugins IPC (#123): list, enable and disable plugins. The
//! UI half and backend calls are #127's.
use caskade_app::commands::plugins as app;

use crate::error::CommandError;
use caskade_plugin_host::{PluginHost, PluginPreview, PluginSummary};
type State<'a> = tauri::State<'a, PluginHost>;

/// Every plugin in the plugins folders, read again from disk. No plugin
/// code runs.
#[tauri::command]
#[specta::specta]
pub async fn plugins_list(state: State<'_>) -> Result<Vec<PluginSummary>, CommandError> {
    app::plugins_list(&state).await
}

/// Enables a plugin with the permissions its manifest asks for.
#[tauri::command]
#[specta::specta]
pub async fn plugin_enable(state: State<'_>, id: String) -> Result<PluginSummary, CommandError> {
    app::plugin_enable(&state, id).await
}

/// Disables a plugin and stops its backend.
#[tauri::command]
#[specta::specta]
pub async fn plugin_disable(state: State<'_>, id: String) -> Result<PluginSummary, CommandError> {
    app::plugin_disable(&state, id).await
}

/// What installing a plugin folder would add (name, version,
/// contributions, permissions). Copies and runs nothing.
#[tauri::command]
#[specta::specta]
pub async fn plugin_install_preview(
    state: State<'_>,
    folder: String,
) -> Result<PluginPreview, CommandError> {
    app::plugin_install_preview(&state, folder).await
}

/// Installs a reviewed plugin folder and grants what it asks for. The UI
/// calls this only after the user accepted the preview.
#[tauri::command]
#[specta::specta]
pub async fn plugin_install(
    state: State<'_>,
    folder: String,
) -> Result<PluginSummary, CommandError> {
    app::plugin_install(&state, folder).await
}

/// Removes a plugin with its settings, stored data and grants (#128).
#[tauri::command]
#[specta::specta]
pub async fn plugin_uninstall(state: State<'_>, id: String) -> Result<(), CommandError> {
    app::plugin_uninstall(&state, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_ipc::{assert_report, invoke};
    use caskade_plugin_host::{HostOptions, Limits, PluginServices};
    use serde_json::json;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn plugins_are_listed_enabled_and_disabled_through_typed_ipc() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = temp.path().join("plugins/stats");
        std::fs::create_dir_all(plugin.join("dist")).unwrap();
        std::fs::write(plugin.join("dist/ui.js"), b"export default {}").unwrap();
        std::fs::write(
            plugin.join("plugin.json"),
            json!({
                "id": "dev.example.stats",
                "name": "Playtime stats",
                "version": "0.1.0",
                "api": "^0.1",
                "ui": "dist/ui.js",
                "contributes": {"views": [{"id": "stats", "title": "Stats"}]}
            })
            .to_string(),
        )
        .unwrap();
        let store =
            tauri::async_runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
        let host = PluginHost::new(
            PluginServices {
                store,
                bus: None,
                settings: None,
            },
            HostOptions {
                folders: vec![temp.path().join("plugins")],
                data: temp.path().join("plugin-data"),
                limits: Limits::default(),
            },
        )
        .unwrap();
        let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
            tauri_specta::collect_commands![
                plugins_list,
                plugin_enable,
                plugin_disable,
                plugin_install_preview,
                plugin_install
            ],
        );
        let app = mock_builder()
            .manage(host)
            .invoke_handler(specta.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let list = invoke(&window, "plugins_list", json!({})).unwrap();
        assert_eq!(list[0]["id"], "dev.example.stats");
        assert_eq!(list[0]["status"], "disabled");
        assert_eq!(list[0]["hasUi"], true);
        assert_eq!(list[0]["contributes"]["views"][0]["title"], "Stats");
        let enabled = invoke(&window, "plugin_enable", json!({"id": "dev.example.stats"})).unwrap();
        assert_eq!(enabled["status"], "enabled");
        let disabled = invoke(
            &window,
            "plugin_disable",
            json!({"id": "dev.example.stats"}),
        )
        .unwrap();
        assert_eq!(disabled["status"], "disabled");
        let gone = invoke(&window, "plugin_enable", json!({"id": "dev.example.nope"})).unwrap_err();
        assert_report(&gone, "conflict", &["dev.example.nope"]);
        let empty = temp.path().join("Downloads/empty");
        std::fs::create_dir_all(&empty).unwrap();
        let invalid = invoke(
            &window,
            "plugin_install",
            json!({"folder": empty.to_string_lossy()}),
        )
        .unwrap_err();
        assert_report(&invalid, "invalid", &["Downloads", "plugin.json"]);
        assert!(invalid["detail"].as_str().unwrap().contains("plugin.json"));

        // Installing from a folder: the preview copies nothing, so declining
        // leaves nothing installed; accepting installs and enables it.
        let offered = temp.path().join("Downloads/badges");
        std::fs::create_dir_all(offered.join("dist")).unwrap();
        std::fs::write(offered.join("dist/ui.js"), b"export default {}").unwrap();
        std::fs::write(
            offered.join("plugin.json"),
            json!({
                "id": "dev.example.badges",
                "name": "Badges",
                "version": "2.0.0",
                "api": "^0.1",
                "ui": "dist/ui.js",
                "permissions": {"library": "read"}
            })
            .to_string(),
        )
        .unwrap();
        let preview = invoke(
            &window,
            "plugin_install_preview",
            json!({"folder": offered.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(preview["plugin"]["name"], "Badges");
        assert_eq!(preview["plugin"]["permissions"]["library"], "read");
        assert!(!temp.path().join("plugins/dev.example.badges").exists());
        let list = invoke(&window, "plugins_list", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);

        let installed = invoke(
            &window,
            "plugin_install",
            json!({"folder": offered.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(installed["status"], "enabled");
        assert!(
            temp.path()
                .join("plugins/dev.example.badges/plugin.json")
                .is_file()
        );
    }
}
