//! The host against a real backend component: discovery without running
//! code, the host functions, and every failure mode contained (#123).

use crate::guest;
use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
};
use caskade_plugin_host::{
    CallError, HostError, HostOptions, InterceptResult, Limits, PluginHost, PluginServices,
    PluginStatus,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn write_plugin(root: &Path, folder: &str, manifest: serde_json::Value) -> PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(guest(), dir.join("dist/backend.wasm")).unwrap();
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    dir
}

fn manifest(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": "Test plugin",
        "version": "1.2.3",
        "api": "^0.1",
        "backend": "dist/backend.wasm",
        "permissions": {"library": "read"},
        "activation": ["onRequest"],
        "contributes": {
            "hooks": ["game.exited", "launch.before"],
            "slots": [{"slot": "game.details.tabs", "id": "info", "title": "Info"}]
        }
    })
}

struct Setup {
    _temp: tempfile::TempDir,
    store: LibraryStore,
    bus: Bus,
    host: PluginHost,
}

async fn setup(limits: Limits) -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    write_plugin(&plugins, "good", manifest("dev.caskade.test"));
    let mut bad = manifest("NotReverseDns");
    bad["unexpected"] = serde_json::json!(true);
    write_plugin(&plugins, "bad", bad);
    let mut escaping = manifest("dev.caskade.escape");
    escaping["backend"] = serde_json::json!("../good/dist/backend.wasm");
    write_plugin(&plugins, "escape", escaping);
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
            settings: None,
        },
        HostOptions {
            folders: vec![plugins],
            data: temp.path().join("plugin-data"),
            limits,
        },
    )
    .unwrap();
    Setup {
        _temp: temp,
        store,
        bus,
        host,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_are_discovered_and_listed_without_running_them() {
    let s = setup(Limits::default()).await;
    let plugins = s.host.discover().await.unwrap();
    assert_eq!(plugins.len(), 3);
    let good = plugins.iter().find(|p| p.id == "dev.caskade.test").unwrap();
    assert_eq!(good.status, PluginStatus::Disabled);
    assert_eq!(good.version.as_deref(), Some("1.2.3"));
    assert!(good.has_backend && !good.has_ui);
    assert_eq!(
        good.contributes.as_ref().unwrap().hooks,
        ["game.exited", "launch.before"]
    );
    // Invalid ones are listed with what's wrong; they don't block the rest.
    let bad = plugins.iter().find(|p| p.folder.ends_with("bad")).unwrap();
    assert_eq!(bad.status, PluginStatus::Invalid);
    assert_eq!(bad.errors[0].key, "unexpected");
    let escape = plugins
        .iter()
        .find(|p| p.folder.ends_with("escape"))
        .unwrap();
    assert_eq!(escape.status, PluginStatus::Invalid);
    assert_eq!(escape.errors[0].key, "backend");

    // Disabled plugins don't run; enabling persists, and nothing starts
    // until the first call.
    assert!(matches!(
        s.host.request("dev.caskade.test", "echo", "{}").await,
        Err(HostError::Disabled(_))
    ));
    let enabled = s.host.set_enabled("dev.caskade.test", true).await.unwrap();
    assert_eq!(enabled.status, PluginStatus::Enabled);
    let state = s
        .store
        .plugin_state("dev.caskade.test")
        .await
        .unwrap()
        .unwrap();
    assert!(state.enabled);
    assert_eq!(state.grants["library"], "read");
    assert!(matches!(
        s.host.set_enabled(&bad.id, true).await,
        Err(HostError::Invalid { .. })
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_answers_and_uses_the_host_functions() {
    let s = setup(Limits::default()).await;
    s.host.discover().await.unwrap();
    s.host.set_enabled("dev.caskade.test", true).await.unwrap();
    let id = "dev.caskade.test";
    assert_eq!(
        s.host.request(id, "echo", r#"{"hi":1}"#).await.unwrap(),
        r#"{"hi":1}"#
    );
    let listed = s.host.list().await;
    assert_eq!(
        listed.iter().find(|p| p.id == id).unwrap().status,
        PluginStatus::Active
    );
    assert!(s.host.data_folder(id).unwrap().is_dir());

    // Key-value storage is the plugin's own and bounded by the store.
    assert_eq!(
        s.host.request(id, "kv", "stored").await.unwrap(),
        "stored:true"
    );
    s.host
        .event(id, "game.exited", r#"{"gameId":"g1"}"#)
        .await
        .unwrap();
    assert_eq!(
        s.store
            .plugin_kv_get(id, "event:game.exited")
            .await
            .unwrap(),
        Some(br#"{"gameId":"g1"}"#.to_vec())
    );

    // Events it emits reach the bus, tagged with the plugin.
    let mut stream = s.bus.stream();
    s.host.request(id, "emit", r#"{"n":2}"#).await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), stream.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        Event::PluginEvent {
            plugin: id.into(),
            name: "greeted".into(),
            payload: r#"{"n":2}"#.into(),
        }
    );

    // Interceptors rewrite or cancel.
    let rewritten = s
        .host
        .intercept(id, "launch.before", r#"{"program":"game"}"#)
        .await
        .unwrap();
    assert_eq!(
        rewritten,
        InterceptResult::Continue(r#"{"program":"gamemoderun game"}"#.into())
    );
    assert!(matches!(
        s.host.intercept(id, "launch.before", r#"{"cancel-me":1}"#).await.unwrap(),
        InterceptResult::Cancel(reason) if reason.contains("not today")
    ));
    // A plugin's own error is an answer, not a crash.
    assert!(matches!(
        s.host.request(id, "nope", "").await,
        Err(HostError::Plugin { code, .. }) if code == "unknown_method"
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn loops_memory_hogs_and_traps_are_contained() {
    let s = setup(Limits {
        memory: 32 << 20,
        call_timeout: Duration::from_millis(300),
        ..Limits::default()
    })
    .await;
    s.host.discover().await.unwrap();
    s.host.set_enabled("dev.caskade.test", true).await.unwrap();
    let id = "dev.caskade.test";
    let failed = |result: Result<String, HostError>| match result {
        Err(HostError::Call { source, .. }) => source,
        other => panic!("{other:?}"),
    };
    // The bound below is the call's time box. Start the backend first, so
    // it doesn't count the compile, and hold the resource queue every test
    // in this binary shares, so it doesn't count other tests' plugin work.
    assert_eq!(s.host.request(id, "echo", "warm").await.unwrap(), "warm");
    let elapsed = caskade_jobs::global()
        .run("spin", async {
            let started = std::time::Instant::now();
            assert!(matches!(
                failed(s.host.request(id, "spin", "").await),
                CallError::TimedOut(_)
            ));
            started.elapsed()
        })
        .await;
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert_eq!(
        s.host.request(id, "echo", "after the loop").await.unwrap(),
        "after the loop"
    );
    assert_eq!(
        failed(s.host.request(id, "grow", "").await),
        CallError::MemoryLimit
    );
    assert_eq!(
        s.host.request(id, "echo", "after the hog").await.unwrap(),
        "after the hog"
    );
    assert!(matches!(
        failed(s.host.request(id, "trap", "").await),
        CallError::Trap(_)
    ));
    assert_eq!(
        s.host.request(id, "echo", "after the trap").await.unwrap(),
        "after the trap"
    );
    let listed = s.host.list().await;
    let plugin = listed.iter().find(|p| p.id == id).unwrap();
    assert_eq!(plugin.failures, 3);
    assert!(plugin.last_error.as_deref().unwrap().contains("crashed"));
}

/// A tiny HTTP server: `/redirect` points at `localhost`, `/slow` answers
/// after 1.2 s, anything else is 200.
fn http_server() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]);
            if request.starts_with("GET /slow") {
                std::thread::sleep(Duration::from_millis(1200));
            }
            let response = if request.starts_with("GET /redirect") {
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_owned()
            };
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

async fn host_with(
    permissions: serde_json::Value,
) -> (tempfile::TempDir, LibraryStore, PluginHost) {
    host_with_limits(permissions, Limits::default()).await
}

async fn host_with_limits(
    permissions: serde_json::Value,
    limits: Limits,
) -> (tempfile::TempDir, LibraryStore, PluginHost) {
    let temp = tempfile::tempdir().unwrap();
    let plugins = temp.path().join("plugins");
    let mut plugin = manifest("dev.caskade.caps");
    plugin["permissions"] = permissions;
    write_plugin(&plugins, "caps", plugin);
    let store = LibraryStore::in_memory().await.unwrap();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![plugins],
            data: temp.path().join("plugin-data"),
            limits,
        },
    )
    .unwrap();
    host.discover().await.unwrap();
    host.set_enabled("dev.caskade.caps", true).await.unwrap();
    (temp, store, host)
}

fn refused(result: Result<String, HostError>, code: &str) -> String {
    match result {
        Err(HostError::Plugin {
            code: got, message, ..
        }) if got == code => message,
        other => panic!("expected a {code} error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn http_reaches_granted_hosts_only_and_redirects_are_checked() {
    let port = http_server();
    let id = "dev.caskade.caps";
    let (_temp, _store, host) =
        host_with(serde_json::json!({"network": [format!("127.0.0.1:{port}")]})).await;
    let url = |host: &str, path: &str| format!("http://{host}:{port}{path}");
    assert_eq!(
        host.request(id, "http", &url("127.0.0.1", "/ok"))
            .await
            .unwrap(),
        "200"
    );
    let message = refused(
        host.request(id, "http", &url("localhost", "/ok")).await,
        "http",
    );
    assert!(message.contains("HttpRequestDenied"), "{message}");
    // The redirect itself is allowed; following it to localhost is not.
    let message = refused(
        host.request(id, "http", &url("127.0.0.1", "/redirect"))
            .await,
        "http",
    );
    assert!(message.contains("HttpRequestDenied"), "{message}");

    let (_temp, _store, offline) = host_with(serde_json::json!({})).await;
    let message = refused(
        offline.request(id, "http", &url("127.0.0.1", "/ok")).await,
        "http",
    );
    assert!(message.contains("HttpRequestDenied"), "{message}");

    // Without a port, a grant reaches 80 and 443 only (CSK-03).
    let (_temp, _store, portless) = host_with(serde_json::json!({"network": ["127.0.0.1"]})).await;
    let message = refused(
        portless.request(id, "http", &url("127.0.0.1", "/ok")).await,
        "http",
    );
    assert!(message.contains("HttpRequestDenied"), "{message}");
}

/// A backend that may go online waits on servers, so a slow answer stays
/// inside its call; one without the grant keeps the short time box.
#[tokio::test(flavor = "multi_thread")]
async fn network_granted_backends_get_the_network_time_budget() {
    let port = http_server();
    let id = "dev.caskade.caps";
    let limits = Limits {
        call_timeout: Duration::from_millis(300),
        network_call_timeout: Duration::from_secs(10),
        ..Limits::default()
    };
    let (_temp, _store, online) = host_with_limits(
        serde_json::json!({"network": [format!("127.0.0.1:{port}")]}),
        limits.clone(),
    )
    .await;
    assert_eq!(
        online
            .request(id, "http", &format!("http://127.0.0.1:{port}/slow"))
            .await
            .unwrap(),
        "200"
    );
    let (_temp, _store, offline) = host_with_limits(serde_json::json!({}), limits).await;
    match offline.request(id, "spin", "").await {
        Err(HostError::Call {
            source: CallError::TimedOut(limit),
            ..
        }) => assert_eq!(limit, Duration::from_millis(300)),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn granted_folders_are_visible_read_only_folders_stay_read_only() {
    let outside = tempfile::tempdir().unwrap();
    let granted = outside.path().join("ROMs");
    let hidden = outside.path().join("Private");
    std::fs::create_dir_all(&granted).unwrap();
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(granted.join("list.txt"), "rom list").unwrap();
    std::fs::write(hidden.join("secret.txt"), "secret").unwrap();
    let id = "dev.caskade.caps";
    let (_temp, _store, host) = host_with(serde_json::json!({
        "fs": [format!("{}:ro", granted.display())]
    }))
    .await;
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    assert_eq!(
        host.request(id, "read", &path(&granted.join("list.txt")))
            .await
            .unwrap(),
        "rom list"
    );
    refused(
        host.request(id, "write", &path(&granted.join("new.txt")))
            .await,
        "fs",
    );
    assert!(!granted.join("new.txt").exists());
    refused(
        host.request(id, "read", &path(&hidden.join("secret.txt")))
            .await,
        "fs",
    );
    // Its private data folder is its own to write.
    assert_eq!(
        host.request(id, "write", "/data/note.txt").await.unwrap(),
        "ok"
    );
    assert!(host.data_folder(id).unwrap().join("note.txt").is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn library_access_follows_the_grant() {
    let id = "dev.caskade.caps";
    let query = r#"{"games": {"limit": 5}}"#;
    let (_temp, _store, none) = host_with(serde_json::json!({})).await;
    refused(
        none.request(id, "library", query).await,
        "permission_denied",
    );

    let (_temp, store, reader) = host_with(serde_json::json!({"library": "read"})).await;
    let game = store
        .upsert_game(&caskade_core::model::Game::new("Celeste"))
        .await
        .unwrap();
    let page: serde_json::Value =
        serde_json::from_str(&reader.request(id, "library", query).await.unwrap()).unwrap();
    assert_eq!(page["total"], 1);
    let update = format!(
        r#"{{"setFavorite": {{"gameId": "{}", "favorite": true}}}}"#,
        game.id
    );
    refused(
        reader.request(id, "library-update", &update).await,
        "permission_denied",
    );

    let (_temp, store, writer) = host_with(serde_json::json!({"library": "write"})).await;
    let game = store
        .upsert_game(&caskade_core::model::Game::new("Hades"))
        .await
        .unwrap();
    let update = format!(
        r#"{{"setFavorite": {{"gameId": "{}", "favorite": true}}}}"#,
        game.id
    );
    writer.request(id, "library-update", &update).await.unwrap();
    assert!(store.game(&game.id).await.unwrap().unwrap().favorite);
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_for_more_than_was_granted_needs_consent_again() {
    let id = "dev.caskade.caps";
    let (temp, _store, host) = host_with(serde_json::json!({"library": "read"})).await;
    assert_eq!(host.request(id, "echo", "x").await.unwrap(), "x");
    // An update asks for network access too.
    let mut grown = manifest(id);
    grown["permissions"] = serde_json::json!({"library": "read", "network": ["example.com"]});
    std::fs::write(
        temp.path().join("plugins/caps/plugin.json"),
        grown.to_string(),
    )
    .unwrap();
    let plugins = host.discover().await.unwrap();
    assert_eq!(plugins[0].status, PluginStatus::NeedsConsent);
    assert!(matches!(
        host.request(id, "echo", "x").await,
        Err(HostError::Disabled(_))
    ));
    host.set_enabled(id, true).await.unwrap();
    assert_eq!(host.request(id, "echo", "y").await.unwrap(), "y");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ui_half_added_later_needs_consent_again() {
    // CSK-05: the grant records whether the manifest had a UI half.
    let id = "dev.caskade.caps";
    let (temp, store, host) = host_with(serde_json::json!({"library": "read"})).await;
    let state = store.plugin_state(id).await.unwrap().unwrap();
    assert_eq!(state.grants["ui"], false);
    // A grant stored before the `ui` key existed still covers a backend.
    store
        .upsert_plugin_state(&caskade_core::db::PluginRow {
            id: id.into(),
            enabled: true,
            grants: serde_json::json!({"library": "read"}),
        })
        .await
        .unwrap();
    let plugins = host.discover().await.unwrap();
    assert!(matches!(
        plugins[0].status,
        PluginStatus::Enabled | PluginStatus::Active
    ));
    // The folder gains a UI half with the same permissions.
    let folder = temp.path().join("plugins/caps");
    std::fs::write(folder.join("ui.js"), "export default {};").unwrap();
    let mut grown = manifest(id);
    grown["ui"] = "ui.js".into();
    std::fs::write(folder.join("plugin.json"), grown.to_string()).unwrap();
    let plugins = host.discover().await.unwrap();
    assert_eq!(plugins[0].status, PluginStatus::NeedsConsent);
    assert!(!host.enabled(id));
    // Consent again grants the UI half, and it stays granted.
    host.set_enabled(id, true).await.unwrap();
    let state = store.plugin_state(id).await.unwrap().unwrap();
    assert_eq!(state.grants["ui"], true);
    let plugins = host.discover().await.unwrap();
    assert!(matches!(
        plugins[0].status,
        PluginStatus::Enabled | PluginStatus::Active
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reviewed_folder_installs_and_a_preview_copies_nothing() {
    let s = setup(Limits::default()).await;
    s.host.discover().await.unwrap();
    let offered = tempfile::tempdir().unwrap();
    let mut plugin = manifest("dev.caskade.offered");
    plugin["permissions"] = serde_json::json!({"network": ["example.com"]});
    let folder = write_plugin(offered.path(), "download", plugin);
    let preview = s.host.preview_folder(&folder).await.unwrap();
    assert_eq!(preview.plugin.id, "dev.caskade.offered");
    assert_eq!(
        preview.plugin.permissions.as_ref().unwrap().network,
        ["example.com"]
    );
    assert_eq!(preview.installed_version, None);
    // Previewing (and declining) leaves the plugins folder untouched.
    assert!(s.host.manifest("dev.caskade.offered").is_none());
    assert!(
        s.host
            .list()
            .await
            .iter()
            .all(|p| p.id != "dev.caskade.offered")
    );

    let installed = s.host.install_folder(&folder).await.unwrap();
    assert_eq!(installed.status, PluginStatus::Enabled);
    assert!(installed.folder.ends_with("dev.caskade.offered"));
    let state = s
        .store
        .plugin_state("dev.caskade.offered")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.grants["network"][0], "example.com");
    assert_eq!(
        s.host
            .request("dev.caskade.offered", "echo", "hi")
            .await
            .unwrap(),
        "hi"
    );
    let again = s.host.preview_folder(&folder).await.unwrap();
    assert_eq!(again.installed_version.as_deref(), Some("1.2.3"));
}
