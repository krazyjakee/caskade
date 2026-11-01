//! #128: a plugin's settings through typed IPC: listed with its schema,
//! validated on save, read by its backend, and removed with the plugin.

use crate::{guest, ipc::invoke};
use caskade_core::settings::SettingsService;
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices};
use serde_json::{Value, json};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

const ID: &str = "dev.example.stats";
const NAMESPACE: &str = "plugins.dev.example.stats";

#[test]
fn plugin_settings_save_validated_reach_the_backend_and_go_with_uninstall() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = temp.path().join("plugins").join(ID);
    std::fs::create_dir_all(plugin.join("dist")).unwrap();
    std::fs::copy(guest(), plugin.join("dist/backend.wasm")).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        json!({
            "id": ID,
            "name": "Playtime stats",
            "version": "0.1.0",
            "api": "^0.1",
            "backend": "dist/backend.wasm",
            "activation": ["onRequest"],
            "contributes": {
                "hooks": ["settings.changed"],
                "settings": {
                    "type": "object",
                    "properties": {
                        "limit": {"type": "integer", "minimum": 1, "maximum": 10, "default": 5}
                    },
                    "additionalProperties": false
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let store =
        tauri::async_runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
    let settings = SettingsService::new(store.clone());
    let host = PluginHost::new(
        PluginServices {
            store,
            bus: None,
            settings: Some(settings.clone()),
        },
        HostOptions {
            folders: vec![temp.path().join("plugins")],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            caskade_desktop::commands::plugins::plugins_list,
            caskade_desktop::commands::plugins::plugin_enable,
            caskade_desktop::commands::plugins::plugin_uninstall,
            caskade_desktop::commands::plugin_ui::plugin_request,
            caskade_desktop::commands::settings::settings_sections::<MockRuntime>,
            caskade_desktop::commands::settings::settings_get,
            caskade_desktop::commands::settings::settings_set,
        ]);
    let app = mock_builder()
        .manage(host)
        .manage(settings)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let listed = invoke(&window, "plugins_list", json!({})).unwrap();
    assert_eq!(listed[0]["settingsError"], Value::Null);
    invoke(&window, "plugin_enable", json!({"id": ID})).unwrap();
    let sections = invoke(&window, "settings_sections", json!({})).unwrap();
    let section = sections
        .as_array()
        .unwrap()
        .iter()
        .find(|section| section["namespace"] == NAMESPACE)
        .expect("the plugin's schema is a settings section");
    assert_eq!(section["defaults"], json!({"limit": 5}));

    let rejected = invoke(
        &window,
        "settings_set",
        json!({"namespace": NAMESPACE, "value": {"limit": 11}, "expectedRevision": 0}),
    );
    assert!(rejected.unwrap_err().to_string().contains("/limit"));
    let saved = invoke(
        &window,
        "settings_set",
        json!({"namespace": NAMESPACE, "value": {"limit": 7}, "expectedRevision": 0}),
    )
    .unwrap();
    assert_eq!(saved["revision"], 1);
    // The backend's `settings-get` sees the saved value.
    let read = invoke(
        &window,
        "plugin_request",
        json!({"pluginId": ID, "method": "settings", "payload": null}),
    )
    .unwrap();
    assert_eq!(read, json!({"limit": 7}));

    invoke(&window, "plugin_uninstall", json!({"id": ID})).unwrap();
    assert!(invoke(&window, "settings_get", json!({"namespace": NAMESPACE})).is_err());
    assert_eq!(
        invoke(&window, "plugins_list", json!({})).unwrap(),
        json!([])
    );
    assert!(!plugin.exists());
}
