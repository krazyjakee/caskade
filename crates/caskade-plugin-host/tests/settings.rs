//! Plugin settings (#128) against the real test guest: a manifest's schema
//! becomes a validated settings section the backend reads, changes reach it
//! as `settings.changed`, and uninstall removes everything.

use crate::guest;
use caskade_core::{db::LibraryStore, settings::SettingsService};
use caskade_plugin_host::{
    HostOptions, Limits, PluginHost, PluginServices, PluginStatus, settings,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const ID: &str = "dev.caskade.stats";
const NAMESPACE: &str = "plugins.dev.caskade.stats";

fn write_plugin(root: &Path, id: &str, settings: Value) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(guest(), dir.join("dist/backend.wasm")).unwrap();
    let manifest = json!({
        "id": id,
        "name": "Stats",
        "version": "1.2.3",
        "api": "^0.1",
        "backend": "dist/backend.wasm",
        "permissions": {},
        "activation": ["onRequest"],
        "contributes": {"hooks": ["settings.changed"], "settings": settings},
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    dir
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {"type": "integer", "title": "Top games", "minimum": 1, "maximum": 10, "default": 5},
            "showHours": {"type": "boolean", "default": true},
        },
        "additionalProperties": false,
    })
}

struct Setup {
    temp: tempfile::TempDir,
    store: LibraryStore,
    settings: SettingsService,
    host: PluginHost,
}

async fn setup() -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    write_plugin(&plugins, ID, schema());
    // `required` without a default: the defaults can't be valid.
    write_plugin(
        &plugins,
        "dev.caskade.broken",
        json!({"type": "object", "required": ["token"], "properties": {"token": {"type": "string"}}}),
    );
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: None,
            settings: Some(settings.clone()),
        },
        HostOptions {
            folders: vec![plugins],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    Setup {
        temp,
        store,
        settings,
        host,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schema_becomes_validated_settings_the_backend_reads_and_hears_about() {
    let Setup {
        temp,
        store,
        settings,
        host,
    } = setup().await;
    let found = host.discover().await.unwrap();
    let section = settings
        .sections()
        .unwrap()
        .into_iter()
        .find(|section| section.namespace == NAMESPACE)
        .expect("the plugin's settings are registered");
    assert_eq!(
        section.version,
        settings::schema_version(&"1.2.3".parse().unwrap())
    );
    assert_eq!(section.defaults, json!({"limit": 5, "showHours": true}));

    // A refused schema is reported on that plugin only; it stays valid.
    let broken = found.iter().find(|p| p.id == "dev.caskade.broken").unwrap();
    assert_eq!(broken.status, PluginStatus::Disabled);
    assert!(
        broken
            .settings_error
            .as_deref()
            .unwrap()
            .contains("settings")
    );
    assert!(
        found
            .iter()
            .find(|p| p.id == ID)
            .unwrap()
            .settings_error
            .is_none()
    );

    host.set_enabled(ID, true).await.unwrap();
    assert!(
        settings
            .set(NAMESPACE, json!({"limit": 0, "showHours": true}), None)
            .await
            .is_err(),
        "core validates against the plugin's schema"
    );
    let mut changes = settings.subscribe();
    settings
        .set(NAMESPACE, json!({"limit": 8, "showHours": false}), None)
        .await
        .unwrap();
    let read: Value =
        serde_json::from_str(&host.request(ID, "settings", "").await.unwrap()).unwrap();
    assert_eq!(read, json!({"limit": 8, "showHours": false}));

    // The change reaches the backend's on-event, which the guest records.
    let changed = changes.recv().await.unwrap();
    assert!(settings::deliver(&host, &settings, &changed).await.unwrap());
    let heard = store
        .plugin_kv_get(ID, "event:settings.changed")
        .await
        .unwrap()
        .expect("the guest stored the event");
    let heard: Value = serde_json::from_slice(&heard).unwrap();
    assert_eq!(heard["version"], 1);
    assert_eq!(heard["data"]["namespace"], NAMESPACE);
    assert_eq!(
        heard["data"]["value"],
        json!({"limit": 8, "showHours": false})
    );
    // A disabled plugin doesn't hear about changes.
    host.set_enabled(ID, false).await.unwrap();
    assert!(!settings::deliver(&host, &settings, &changed).await.unwrap());

    // Uninstall forgets the settings, the KV, the state and the data folder.
    host.uninstall(ID).await.unwrap();
    assert!(settings.get(NAMESPACE).await.is_err());
    assert!(
        store
            .get_setting(&format!("settings.section.{NAMESPACE}"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .plugin_kv_get(ID, "event:settings.changed")
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.plugin_state(ID).await.unwrap().is_none());
    assert!(!temp.path().join("plugins").join(ID).exists());
    assert!(!temp.path().join("plugin-data").join(ID).exists());
    assert!(host.list().await.iter().all(|plugin| plugin.id != ID));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_plugin_version_keeps_the_values_that_still_fit() {
    let Setup {
        temp,
        settings,
        host,
        ..
    } = setup().await;
    host.discover().await.unwrap();
    settings
        .set(NAMESPACE, json!({"limit": 8, "showHours": false}), None)
        .await
        .unwrap();
    // 1.3.0 drops `showHours`.
    let folder = temp.path().join("plugins").join(ID);
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("plugin.json")).unwrap())
            .unwrap();
    manifest["version"] = json!("1.3.0");
    manifest["contributes"]["settings"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("showHours");
    std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
    host.discover().await.unwrap();
    assert_eq!(
        settings.get(NAMESPACE).await.unwrap().value,
        json!({"limit": 8})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstall_keeps_a_plugin_folder_loaded_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let dev = write_plugin(temp.path(), ID, schema());
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    let host = PluginHost::new(
        PluginServices {
            store,
            bus: None,
            settings: Some(settings.clone()),
        },
        HostOptions {
            // The plugin folder itself, as in dev mode.
            folders: vec![dev.clone()],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    host.discover().await.unwrap();
    host.uninstall(ID).await.unwrap();
    assert!(dev.join("plugin.json").exists());
    assert!(settings.get(NAMESPACE).await.is_err());
}
