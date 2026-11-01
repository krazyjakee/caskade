//! #130: each create-plugin template, generated into a temp folder, builds,
//! and the host discovers, validates and runs it. Needs Node with the
//! workspace's packages installed (`pnpm install`) and the wasm32-wasip2
//! target, like the rest of the plugin tests.

use caskade_core::db::LibraryStore;
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices, PluginStatus};
use serde_json::{Value, json};
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

/// Generates a template with the CLI and builds both halves.
fn scaffold(root: &Path, folder: &str, halves: &[&str]) -> PathBuf {
    let dir = root.join(folder);
    run(Command::new("node")
        .arg(repo().join("packages/create-plugin/index.mjs"))
        .arg(&dir)
        .args(["--id", &format!("dev.example.{folder}"), "--name", folder])
        .args(halves)
        .arg("--sdk")
        .arg(repo()));
    if !halves.contains(&"--no-ui") {
        run(Command::new("node")
            .arg(repo().join("apps/desktop/scripts/build-plugin-ui.mjs"))
            .arg(&dir));
    }
    if !halves.contains(&"--no-backend") {
        run(Command::new("node")
            .arg("scripts/build-backend.mjs")
            .current_dir(&dir)
            .env(
                "CARGO_TARGET_DIR",
                Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-guest"),
            )
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS"));
    }
    dir
}

#[tokio::test]
async fn every_template_builds_and_runs_in_the_host() {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let ui = scaffold(&plugins, "ui-only", &["--ui", "--no-backend"]);
    let backend = scaffold(&plugins, "backend-only", &["--no-ui", "--backend"]);
    let both = scaffold(&plugins, "both", &["--ui", "--backend"]);
    assert!(ui.join("dist/ui.js").is_file() && ui.join("dist/ui.css").is_file());
    assert!(!ui.join("dist/backend.wasm").exists());
    assert!(backend.join("dist/backend.wasm").is_file() && !backend.join("dist/ui.js").exists());
    assert!(both.join("dist/ui.js").is_file() && both.join("dist/backend.wasm").is_file());

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
    assert_eq!(found.len(), 3);
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
        host.set_enabled(&plugin.id, true).await.unwrap();
    }
    let summary = |id: &str| found.iter().find(|plugin| plugin.id == id).unwrap().clone();
    assert!(summary("dev.example.ui-only").has_ui && !summary("dev.example.ui-only").has_backend);
    assert!(summary("dev.example.backend-only").has_backend);

    for id in ["dev.example.backend-only", "dev.example.both"] {
        let answer: Value = serde_json::from_str(
            &host
                .request(id, "hello", &json!({"name": "Ada"}).to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(answer, json!({"message": "Hello, Ada!"}), "{id}");
        assert!(host.request(id, "nope", "null").await.is_err());
        // The template's event handler runs too.
        host.event(id, "game.exited", r#"{"version":1,"data":{}}"#)
            .await
            .unwrap();
    }
}
