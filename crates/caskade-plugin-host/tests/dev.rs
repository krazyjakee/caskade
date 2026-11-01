//! #131: dev plugins load in place, are never persisted, and reload on
//! change; a broken rebuild reports its error and nothing else stops.

use crate::guest;
use caskade_core::{
    bus::{Bus, Event, Outcome},
    db::LibraryStore,
    launch::{LaunchBefore, LaunchCommand},
};
use caskade_plugin_host::{
    HostOptions, Limits, PluginHooks, PluginHost, PluginServices, PluginStatus, dev,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::broadcast;

fn manifest(id: &str, hooks: &[&str]) -> Value {
    json!({
        "id": id,
        "name": "Dev plugin",
        "version": "0.1.0",
        "api": "^0.1",
        "ui": "dist/ui.js",
        "backend": "dist/backend.wasm",
        "permissions": {"process": true},
        "contributes": {"hooks": hooks},
    })
}

fn write_plugin(dir: &Path, manifest: &Value) {
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(guest(), dir.join("dist/backend.wasm")).unwrap();
    std::fs::write(dir.join("dist/ui.js"), b"export default {}").unwrap();
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
}

struct Setup {
    temp: tempfile::TempDir,
    store: LibraryStore,
    bus: Bus,
    host: PluginHost,
    hooks: std::sync::Arc<PluginHooks>,
}

async fn setup() -> Setup {
    let temp = tempfile::tempdir().unwrap();
    // An installed plugin next to the dev one keeps working throughout.
    write_plugin(
        &temp.path().join("plugins/installed"),
        &manifest("dev.test.installed", &[]),
    );
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
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
    host.set_enabled("dev.test.installed", true).await.unwrap();
    let hooks = PluginHooks::new(host.clone(), bus.clone());
    Setup {
        temp,
        store,
        bus,
        host,
        hooks,
    }
}

/// The next `plugin.reloaded` on the bus. A reload compiles the backend in
/// the resource queue every test in this binary shares, which can take long.
async fn reloaded(stream: &mut broadcast::Receiver<Event>) -> (String, bool, Option<String>) {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if let Ok(Event::PluginReloaded { plugin, ui, error }) = stream.recv().await {
                return (plugin, ui, error);
            }
        }
    })
    .await
    .expect("a reload")
}

/// Up to a minute, for the same reason as [`reloaded`].
async fn until(mut check: impl FnMut() -> bool) {
    for _ in 0..2400 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("condition not met");
}

#[tokio::test]
async fn a_dev_plugin_loads_in_place_without_being_persisted() {
    let s = setup().await;
    let folder = s.temp.path().join("work/my-plugin");
    write_plugin(&folder, &manifest("dev.test.mine", &[]));
    let watch = dev::watch(s.host.clone(), s.bus.clone(), &folder, dev::DEBOUNCE)
        .await
        .unwrap();
    let summary = s
        .host
        .list()
        .await
        .into_iter()
        .find(|plugin| plugin.id == "dev.test.mine")
        .unwrap();
    assert!(summary.dev);
    assert_eq!(summary.status, PluginStatus::Enabled);
    assert!(
        s.host.grants("dev.test.mine").await.unwrap().process,
        "grants follow the dev manifest"
    );
    assert_eq!(
        s.host
            .request("dev.test.mine", "echo", "\"hi\"")
            .await
            .unwrap(),
        "\"hi\""
    );
    // Nothing stored, nothing copied into the plugins folder.
    assert!(
        s.store
            .plugin_state("dev.test.mine")
            .await
            .unwrap()
            .is_none()
    );
    assert!(!s.temp.path().join("plugins/dev.test.mine").exists());
    // Turning it off is for this session only.
    s.host.set_enabled("dev.test.mine", false).await.unwrap();
    assert!(
        s.store
            .plugin_state("dev.test.mine")
            .await
            .unwrap()
            .is_none()
    );
    drop(watch);
    s.host
        .remove_dev_folder(&folder.canonicalize().unwrap())
        .await
        .unwrap();
    assert!(s.host.manifest("dev.test.mine").is_none());
}

#[tokio::test]
async fn changes_reload_the_ui_and_backend_and_a_broken_build_reports_its_error() {
    let s = setup().await;
    let folder = s.temp.path().join("work/my-plugin");
    write_plugin(&folder, &manifest("dev.test.mine", &[]));
    let mut stream = s.bus.stream();
    let _watch = dev::watch(
        s.host.clone(),
        s.bus.clone(),
        &folder,
        Duration::from_millis(50),
    )
    .await
    .unwrap();
    s.host.request("dev.test.mine", "echo", "1").await.unwrap();
    assert!(s.host.is_active("dev.test.mine"));

    std::fs::write(folder.join("dist/ui.js"), b"export default { v: 2 }").unwrap();
    assert_eq!(
        reloaded(&mut stream).await,
        ("dev.test.mine".to_owned(), true, None)
    );

    // A broken backend: the error is reported and the app carries on.
    std::fs::write(folder.join("dist/backend.wasm"), b"not wasm").unwrap();
    let (plugin, ui, error) = reloaded(&mut stream).await;
    assert_eq!((plugin.as_str(), ui), ("dev.test.mine", false));
    assert!(error.is_some());
    assert!(!s.host.is_active("dev.test.mine"));
    assert!(s.host.request("dev.test.mine", "echo", "1").await.is_err());
    assert_eq!(
        s.host
            .request("dev.test.installed", "echo", "\"ok\"")
            .await
            .unwrap(),
        "\"ok\""
    );

    // The fixed build is compiled and runs again.
    std::fs::copy(guest(), folder.join("dist/backend.wasm")).unwrap();
    assert_eq!(
        reloaded(&mut stream).await,
        ("dev.test.mine".to_owned(), false, None)
    );
    assert_eq!(
        s.host.request("dev.test.mine", "echo", "2").await.unwrap(),
        "2"
    );
}

#[tokio::test]
async fn a_manifest_change_reattaches_the_hooks_of_the_new_version() {
    let s = setup().await;
    let folder = s.temp.path().join("work/my-plugin");
    write_plugin(&folder, &manifest("dev.test.mine", &["launch.before"]));
    let mut stream = s.bus.stream();
    let _watch = dev::watch(
        s.host.clone(),
        s.bus.clone(),
        &folder,
        Duration::from_millis(50),
    )
    .await
    .unwrap();
    let hooks = s.hooks.clone();
    until(|| hooks.attached("dev.test.mine")).await;
    let command = LaunchCommand::executable("/games/game");
    let Outcome::Continue(wrapped) = s.bus.run::<LaunchBefore>(command.clone()).await.outcome
    else {
        panic!("cancelled");
    };
    assert_eq!(wrapped.program, PathBuf::from("gamemoderun /games/game"));

    // The new version no longer intercepts launches.
    std::fs::write(
        folder.join("plugin.json"),
        manifest("dev.test.mine", &[]).to_string(),
    )
    .unwrap();
    let (_, ui, error) = reloaded(&mut stream).await;
    assert!(ui);
    assert_eq!(error, None);
    until(|| s.hooks.attached("dev.test.mine")).await;
    for _ in 0..40 {
        if s.bus.run::<LaunchBefore>(command.clone()).await.outcome
            == Outcome::Continue(command.clone())
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the old interceptor is still registered");
}

#[tokio::test]
async fn an_invalid_manifest_is_reported() {
    let s = setup().await;
    let folder = s.temp.path().join("work/my-plugin");
    write_plugin(&folder, &manifest("dev.test.mine", &[]));
    let mut stream = s.bus.stream();
    let _watch = dev::watch(
        s.host.clone(),
        s.bus.clone(),
        &folder,
        Duration::from_millis(50),
    )
    .await
    .unwrap();
    let mut broken = manifest("dev.test.mine", &[]);
    broken["unexpected"] = json!(true);
    std::fs::write(folder.join("plugin.json"), broken.to_string()).unwrap();
    let (_, _, error) = reloaded(&mut stream).await;
    assert!(error.unwrap().contains("unexpected"));
    // The installed plugin is unaffected.
    assert_eq!(
        s.host
            .request("dev.test.installed", "echo", "\"ok\"")
            .await
            .unwrap(),
        "\"ok\""
    );
}
