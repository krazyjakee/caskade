//! #127: a plugin's UI half reaches its own backend half through the
//! commands.

use crate::guest;
use caskade_app::commands::plugin_ui;
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices};
use serde_json::json;
use std::path::Path;

/// Where a test shell would serve a plugin's file.
fn url(id: &str, relative: &Path) -> String {
    let path: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    format!("test-plugin://{id}/{}", path.join("/"))
}

#[tokio::test(flavor = "multi_thread")]
async fn ui_half_lists_its_module_and_calls_its_backend() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = temp.path().join("plugins/hltb");
    std::fs::create_dir_all(plugin.join("dist")).unwrap();
    std::fs::copy(guest(), plugin.join("dist/backend.wasm")).unwrap();
    std::fs::write(plugin.join("dist/ui.js"), b"export default {}").unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        json!({
            "id": "dev.example.hltb",
            "name": "HowLongToBeat",
            "version": "0.1.0",
            "api": "^0.1",
            "ui": "dist/ui.js",
            "backend": "dist/backend.wasm",
            "activation": ["onRequest"],
            "contributes": {
                "slots": [{"slot": "game.details.tabs", "id": "hltb", "title": "How long to beat"}]
            }
        })
        .to_string(),
    )
    .unwrap();
    let store = caskade_core::db::LibraryStore::in_memory().await.unwrap();
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
    host.discover().await.unwrap();
    host.set_enabled("dev.example.hltb", true).await.unwrap();

    let listed = plugin_ui::plugins_ui(&host, url).await.unwrap();
    assert_eq!(listed[0].id, "dev.example.hltb");
    assert!(
        listed[0].entry.ends_with("/dev.example.hltb/dist/ui.js"),
        "{}",
        listed[0].entry
    );
    assert_eq!(listed[0].contributes[0].slot, "game.details.tabs");

    // `ctx.backend.call("echo", …)` reaches the backend's handle-request.
    let payload = json!({"title": "Celeste", "fields": ["main", "extras"]});
    let answer = plugin_ui::plugin_request(
        &host,
        "dev.example.hltb".into(),
        "echo".into(),
        payload.clone(),
    )
    .await
    .unwrap();
    assert_eq!(answer, payload);
    let unknown =
        plugin_ui::plugin_request(&host, "dev.example.hltb".into(), "nope".into(), json!(null))
            .await
            .unwrap_err();
    assert!(unknown.contains("nope"), "{unknown}");
    assert!(
        plugin_ui::plugin_request(
            &host,
            "dev.example.other".into(),
            "echo".into(),
            json!(null)
        )
        .await
        .is_err()
    );
}
