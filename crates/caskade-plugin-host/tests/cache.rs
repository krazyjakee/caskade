//! The compiled-component cache (#136) with a real backend: a second start
//! of the app loads the verified entry instead of compiling.

use crate::guest;
use caskade_core::db::LibraryStore;
use caskade_plugin_host::{CompileCounts, HostOptions, Limits, PluginHost, PluginServices};
use std::path::Path;

const ID: &str = "dev.caskade.cached";

fn write_plugin(plugins: &Path, backend: &[u8]) {
    let dir = plugins.join("cached");
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::write(dir.join("dist/backend.wasm"), backend).unwrap();
    let manifest = serde_json::json!({
        "id": ID,
        "name": "Cached",
        "version": "0.1.0",
        "api": "^0.1",
        "backend": "dist/backend.wasm",
        "permissions": {},
        "activation": ["onRequest"],
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
}

/// One app start: a host over the same database and folders.
async fn start(store: &LibraryStore, root: &Path) -> PluginHost {
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![root.join("plugins")],
            data: root.join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    host.enable_component_cache(root.join("component-cache"))
        .await
        .unwrap();
    host.discover().await.unwrap();
    host.set_enabled(ID, true).await.unwrap();
    assert_eq!(host.request(ID, "echo", "hi").await.unwrap(), "hi");
    host
}

fn counts(cached: u32, compiled: u32) -> CompileCounts {
    CompileCounts { cached, compiled }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_start_skips_compiling_an_unchanged_plugin() {
    let temp = tempfile::tempdir().unwrap();
    write_plugin(
        &temp.path().join("plugins"),
        &std::fs::read(guest()).unwrap(),
    );
    let store = LibraryStore::in_memory().await.unwrap();

    let first = start(&store, temp.path()).await;
    assert_eq!(first.compile_counts(), counts(0, 1));
    drop(first);
    let second = start(&store, temp.path()).await;
    assert_eq!(
        second.compile_counts(),
        counts(1, 0),
        "loaded, not compiled"
    );

    // Another install (another key in its database) doesn't trust the entry.
    let other = LibraryStore::in_memory().await.unwrap();
    assert_eq!(
        start(&other, temp.path()).await.compile_counts(),
        counts(0, 1)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_component_is_compiled_again() {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    let mut backend = std::fs::read(guest()).unwrap();
    write_plugin(&plugins, &backend);
    let store = LibraryStore::in_memory().await.unwrap();
    start(&store, temp.path()).await;
    // The same component with an extra custom section: other bytes.
    backend.extend_from_slice(&[0x00, 0x04, 0x03, b'n', b'e', b'w']);
    write_plugin(&plugins, &backend);
    assert_eq!(
        start(&store, temp.path()).await.compile_counts(),
        counts(0, 1)
    );
}
