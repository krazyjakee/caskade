//! #128: a plugin's settings through the commands: listed with its schema,
//! validated on save, read by its backend, and removed with the plugin.

use crate::guest;
use caskade_app::commands::{plugin_ui, plugins, settings as settings_commands};
use caskade_core::settings::SettingsService;
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices};
use serde_json::json;

const ID: &str = "dev.example.stats";
const NAMESPACE: &str = "plugins.dev.example.stats";

#[tokio::test(flavor = "multi_thread")]
async fn plugin_settings_save_validated_reach_the_backend_and_go_with_uninstall() {
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
    let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
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

    let listed = plugins::plugins_list(&host).await.unwrap();
    assert_eq!(listed[0].settings_error, None);
    plugins::plugin_enable(&host, ID.into()).await.unwrap();
    let sections = settings_commands::settings_sections(&settings, Some(&host)).unwrap();
    let section = sections
        .iter()
        .find(|section| section.namespace == NAMESPACE)
        .expect("the plugin's schema is a settings section");
    assert_eq!(section.defaults, json!({"limit": 5}));

    let rejected =
        settings_commands::settings_set(&settings, NAMESPACE, json!({"limit": 11}), Some(0))
            .await
            .unwrap_err();
    assert!(
        rejected
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("/limit"),
        "{rejected:?}"
    );
    let saved = settings_commands::settings_set(&settings, NAMESPACE, json!({"limit": 7}), Some(0))
        .await
        .unwrap();
    assert_eq!(saved.revision, 1);
    // The backend's `settings-get` sees the saved value.
    let read = plugin_ui::plugin_request(&host, ID.into(), "settings".into(), json!(null))
        .await
        .unwrap();
    assert_eq!(read, json!({"limit": 7}));

    plugins::plugin_uninstall(&host, ID.into()).await.unwrap();
    assert!(
        settings_commands::settings_get(&settings, NAMESPACE)
            .await
            .is_err()
    );
    assert!(plugins::plugins_list(&host).await.unwrap().is_empty());
    assert!(!plugin.exists());
}
