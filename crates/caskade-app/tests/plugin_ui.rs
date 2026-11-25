//! #109: a plugin backend draws its UI contributions and handles actions
//! through the commands.

use crate::guest;
use caskade_app::commands::plugin_ui::{self, UiContext};
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn drawn_plugin_lists_contributions_renders_and_handles_actions() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = temp.path().join("plugins/hltb");
    std::fs::create_dir_all(plugin.join("dist")).unwrap();
    std::fs::copy(guest(), plugin.join("dist/backend.wasm")).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        json!({
            "id": "dev.example.hltb",
            "name": "HowLongToBeat",
            "version": "0.1.0",
            "api": "^0.2",
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

    let listed = plugin_ui::drawn_plugins(&host).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "dev.example.hltb");
    assert_eq!(listed[0].contributes[0].slot, "game.details.tabs");

    let tree = plugin_ui::plugin_ui_render(
        &host,
        "dev.example.hltb".into(),
        "game.details.tabs".into(),
        "hltb".into(),
        UiContext::default(),
    )
    .await
    .unwrap();
    assert!(!tree.is_empty());

    let outcome = plugin_ui::plugin_ui_act(
        &host,
        "dev.example.hltb".into(),
        "game.details.tabs".into(),
        "hltb".into(),
        "view".into(),
        json!("table"),
        UiContext::default(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.state, Some(json!({"view": "table"})));

    assert!(
        plugin_ui::plugin_ui_act(
            &host,
            "dev.example.hltb".into(),
            "game.details.tabs".into(),
            "hltb".into(),
            "nope".into(),
            json!(null),
            UiContext::default(),
        )
        .await
        .is_err()
    );
    assert!(
        plugin_ui::plugin_ui_render(
            &host,
            "dev.example.other".into(),
            "game.details.tabs".into(),
            "hltb".into(),
            UiContext::default(),
        )
        .await
        .is_err()
    );
}
