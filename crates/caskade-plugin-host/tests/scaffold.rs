//! #130: each create-plugin template, generated into a temp folder, builds,
//! and the host discovers, validates and runs it; the view template's
//! backend draws its view (plugin API 0.2, #109). Needs Node and the
//! wasm32-wasip2 target, like the rest of the plugin tests.

use caskade_core::db::LibraryStore;
use caskade_plugin_host::{
    HostError, HostOptions, Limits, PluginHost, PluginServices, PluginStatus,
};
use caskade_plugin_ui::Context;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn run(command: &mut Command) {
    let status = command.status().expect("run the command");
    assert!(status.success(), "{command:?} failed");
}

/// Generates a template with the CLI and builds its backend.
fn scaffold(root: &Path, folder: &str, view: &str) -> PathBuf {
    let dir = root.join(folder);
    run(Command::new("node")
        .arg(repo().join("packages/create-plugin/index.mjs"))
        .arg(&dir)
        .args(["--id", &format!("dev.example.{folder}"), "--name", folder])
        .arg(view)
        .arg("--sdk")
        .arg(repo()));
    run(Command::new("node")
        .arg("scripts/build-backend.mjs")
        .current_dir(&dir)
        .env(
            "CARGO_TARGET_DIR",
            Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-guest"),
        )
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS"));
    dir
}

#[tokio::test(flavor = "multi_thread")]
async fn every_template_builds_and_runs_in_the_host() {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let view = scaffold(&plugins, "with-view", "--ui");
    let hooks = scaffold(&plugins, "hooks-only", "--no-ui");
    for dir in [&view, &hooks] {
        assert!(dir.join("dist/backend.wasm").is_file());
        assert!(!dir.join("dist/ui.js").exists());
    }

    let host = PluginHost::new(
        PluginServices {
            store: LibraryStore::in_memory().await.unwrap(),
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![plugins],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    let found = host.discover().await.unwrap();
    assert_eq!(found.len(), 2);
    for plugin in &found {
        assert!(
            plugin.errors.is_empty(),
            "{}: {:?}",
            plugin.id,
            plugin.errors
        );
        assert_eq!(plugin.status, PluginStatus::Disabled, "{}", plugin.id);
        // Minimal permissions: nothing to review beyond the plugin itself.
        assert_eq!(
            plugin.permissions,
            Some(Default::default()),
            "{}",
            plugin.id
        );
        assert!(plugin.has_backend && !plugin.has_ui, "{}", plugin.id);
        host.set_enabled(&plugin.id, true).await.unwrap();
        // The template's event handler runs.
        host.event(&plugin.id, "game.exited", r#"{"version":1,"data":{}}"#)
            .await
            .unwrap();
    }

    // The view is drawn, and its button counts (#109).
    let id = "dev.example.with-view";
    let drawn = |state: serde_json::Value| {
        let host = host.clone();
        async move {
            serde_json::to_string(
                &host
                    .ui_render(
                        id,
                        "views",
                        "main",
                        &Context {
                            state,
                            ..Context::default()
                        },
                    )
                    .await
                    .unwrap(),
            )
            .unwrap()
        }
    };
    let first = drawn(serde_json::Value::Null).await;
    assert!(
        first.contains("Hello from with-view!") && first.contains("Clicked 0 times"),
        "{first}"
    );
    let outcome = host
        .ui_act(
            id,
            "views",
            "main",
            "click",
            &serde_json::Value::Null,
            &Context::default(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.state, Some(json!({"clicks": 1})));
    assert!(drawn(json!({"clicks": 1})).await.contains("Clicked 1 time"));
    // The template without a view draws nothing.
    assert!(matches!(
        host.ui_render(
            "dev.example.hooks-only",
            "views",
            "main",
            &Context::default()
        )
        .await,
        Err(HostError::Undeclared { .. })
    ));
}
