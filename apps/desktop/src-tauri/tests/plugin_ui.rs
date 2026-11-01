//! #127: a plugin's UI half reaches its own backend half through typed IPC.

use crate::{guest, ipc::invoke};
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices};
use serde_json::json;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

#[test]
fn ui_half_lists_its_module_and_calls_its_backend() {
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
    tauri::async_runtime::block_on(async {
        host.discover().await.unwrap();
        host.set_enabled("dev.example.hltb", true).await.unwrap();
    });
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            caskade_desktop::commands::plugin_ui::plugins_ui,
            caskade_desktop::commands::plugin_ui::plugin_request,
        ]);
    let app = mock_builder()
        .manage(host)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let listed = invoke(&window, "plugins_ui", json!({})).unwrap();
    assert_eq!(listed[0]["id"], "dev.example.hltb");
    assert!(
        listed[0]["entry"]
            .as_str()
            .unwrap()
            .ends_with("/dev.example.hltb/dist/ui.js")
    );
    assert_eq!(listed[0]["contributes"][0]["slot"], "game.details.tabs");

    // `ctx.backend.call("echo", …)` reaches the backend's handle-request.
    let payload = json!({"title": "Celeste", "fields": ["main", "extras"]});
    let answer = invoke(
        &window,
        "plugin_request",
        json!({"pluginId": "dev.example.hltb", "method": "echo", "payload": payload}),
    )
    .unwrap();
    assert_eq!(answer, payload);
    let unknown = invoke(
        &window,
        "plugin_request",
        json!({"pluginId": "dev.example.hltb", "method": "nope", "payload": null}),
    )
    .unwrap_err();
    assert!(unknown.to_string().contains("nope"), "{unknown}");
    assert!(
        invoke(
            &window,
            "plugin_request",
            json!({"pluginId": "dev.example.other", "method": "echo", "payload": null}),
        )
        .is_err()
    );
}
