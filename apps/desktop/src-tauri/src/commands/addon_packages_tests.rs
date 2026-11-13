//! Catalogue installs through IPC against a local server (#182): review,
//! updates that ask for more, integrity, unsafe archives, identity,
//! failed activation with rollback, themes and uninstalls.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_plugin_host::{HostOptions, Limits, PluginServices};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn zip(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

fn plugin(version: &str, permissions: Value, backend: bool) -> Vec<u8> {
    let mut manifest = json!({
        "id": "dev.example.stats", "name": "Stats", "version": version, "api": "^0.1",
        "ui": "dist/ui.js", "permissions": permissions,
        "contributes": {"views": [{"id": "stats", "title": "Stats"}]}
    });
    let mut files = vec![("stats/dist/ui.js", b"export default {}".to_vec())];
    if backend {
        manifest["backend"] = json!("dist/backend.wasm");
        files.push(("stats/dist/backend.wasm", b"not a component".to_vec()));
    }
    files.push(("stats/plugin.json", manifest.to_string().into_bytes()));
    zip(&files)
}

fn theme(version: &str) -> Vec<u8> {
    let root = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../themes/paper"
    ));
    let mut files = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let name = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let mut bytes = std::fs::read(&path).unwrap();
            if name == "theme.json" {
                let mut manifest: Value = serde_json::from_slice(&bytes).unwrap();
                manifest["id"] = json!("dev.example.paper");
                manifest["version"] = json!(version);
                bytes = manifest.to_string().into_bytes();
            }
            files.push((name, bytes));
        }
    }
    let named: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    zip(&named)
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct World {
    server: MockServer,
    temp: tempfile::TempDir,
    store: LibraryStore,
    host: PluginHost,
    themes: ThemeService,
    catalogue: CatalogueService,
}

/// A release in the catalogue: its archive is served at `/p/<file>`.
struct Release {
    kind: &'static str,
    id: &'static str,
    version: &'static str,
    file: &'static str,
    bytes: Vec<u8>,
    sha256: Option<String>,
}

impl World {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let host = PluginHost::new(
            PluginServices {
                store: store.clone(),
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
        let server = MockServer::start().await;
        let catalogue = CatalogueService::new(
            temp.path().join("cache"),
            &format!("{}/catalogue.json", server.uri()),
            None,
        )
        .unwrap();
        Self {
            themes: ThemeService::new(temp.path().join("themes")),
            server,
            temp,
            store,
            host,
            catalogue,
        }
    }

    /// Serves `releases` (newest first per id) and takes the catalogue.
    async fn publish(&self, releases: Vec<Release>) {
        self.server.reset().await;
        let mut packages: Vec<Value> = Vec::new();
        for release in releases {
            Mock::given(method("GET"))
                .and(path(format!("/p/{}", release.file)))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(release.bytes.clone()))
                .mount(&self.server)
                .await;
            let record = json!({
                "version": release.version,
                "api": (release.kind == "plugin").then_some("^0.1"),
                "artifact": {"url": format!("p/{}", release.file),
                    "sha256": release.sha256.unwrap_or_else(|| sha(&release.bytes)),
                    "size": release.bytes.len()}
            });
            match packages.iter_mut().find(|p| p["id"] == release.id) {
                Some(package) => package["releases"].as_array_mut().unwrap().push(record),
                None => packages.push(json!({"id": release.id, "kind": release.kind, "name": "N",
                    "author": "A", "summary": "S", "license": "MIT", "releases": [record]})),
            }
        }
        let doc = json!({"format": 1, "packages": packages}).to_string();
        self.catalogue
            .accept(doc.as_bytes(), None, 1)
            .await
            .unwrap();
    }

    fn app(&self) -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
        let specta = tauri_specta::Builder::<MockRuntime>::new()
            .commands(tauri_specta::collect_commands![
                addons_install::<MockRuntime>,
                addons_uninstall::<MockRuntime>,
                addons_records
            ])
            .events(tauri_specta::collect_events![AddonProgress]);
        let app = mock_builder()
            .manage(self.store.clone())
            .manage(self.host.clone())
            .manage(self.themes.clone())
            .manage(self.catalogue.clone())
            .manage(AddonFolders {
                plugins: self.temp.path().join("plugins"),
                themes: self.temp.path().join("themes"),
                work: self.temp.path().join("addons"),
            })
            .manage(AddonJobs::default())
            .invoke_handler(specta.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        specta.mount_events(&app);
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        (app, window)
    }
}

fn install(
    window: &tauri::WebviewWindow<MockRuntime>,
    id: &str,
    accept: bool,
    retry: bool,
) -> Value {
    invoke(
        window,
        "addons_install",
        json!({"id": id, "mode": "desktop", "accept": accept, "retry": retry}),
    )
    .unwrap()
}

fn plugin_release(version: &'static str, file: &'static str, bytes: Vec<u8>) -> Release {
    Release {
        kind: "plugin",
        id: "dev.example.stats",
        version,
        file,
        bytes,
        sha256: None,
    }
}

#[test]
fn a_plugin_installs_after_review_and_updates_keep_data_without_new_grants() {
    let world = tauri::async_runtime::block_on(World::new());
    tauri::async_runtime::block_on(world.publish(vec![plugin_release(
        "0.1.0",
        "stats-0.1.0.zip",
        plugin("0.1.0", json!({}), false),
    )]));
    let (_app, window) = world.app();
    let review = install(&window, "dev.example.stats", false, false);
    assert_eq!(review["status"], "needsReview");
    assert_eq!(review["preview"]["plugin"]["id"], "dev.example.stats");
    // Nothing installed before the user accepts.
    assert!(!world.temp.path().join("plugins/dev.example.stats").exists());
    let done = install(&window, "dev.example.stats", true, false);
    assert_eq!(done["status"], "installed", "{done}");
    assert!(world.host.enabled("dev.example.stats"));
    let data = world.temp.path().join("plugin-data/dev.example.stats");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("saved.json"), b"{}").unwrap();

    // 0.2.0 asks for the network: installed, but off until the user agrees.
    tauri::async_runtime::block_on(world.publish(vec![
        plugin_release(
            "0.2.0",
            "stats-0.2.0.zip",
            plugin("0.2.0", json!({"network": ["example.org"]}), false),
        ),
        plugin_release(
            "0.1.0",
            "stats-0.1.0.zip",
            plugin("0.1.0", json!({}), false),
        ),
    ]));
    let updated = install(&window, "dev.example.stats", false, false);
    assert_eq!(updated["status"], "installed", "{updated}");
    assert_eq!(updated["needsConsent"], true);
    assert!(!world.host.enabled("dev.example.stats"));
    assert!(data.join("saved.json").is_file());
    let records = invoke(&window, "addons_records", json!({})).unwrap();
    assert_eq!(records["addons"]["dev.example.stats"]["version"], "0.2.0");
}

#[test]
fn bad_downloads_are_refused_and_the_installed_version_stays() {
    let world = tauri::async_runtime::block_on(World::new());
    tauri::async_runtime::block_on(world.publish(vec![plugin_release(
        "0.1.0",
        "stats-0.1.0.zip",
        plugin("0.1.0", json!({}), false),
    )]));
    let (_app, window) = world.app();
    assert_eq!(
        install(&window, "dev.example.stats", true, false)["status"],
        "installed"
    );
    let installed = || {
        tauri::async_runtime::block_on(world.host.list())
            .into_iter()
            .find(|p| p.id == "dev.example.stats")
            .and_then(|p| p.version)
    };

    // The checksum doesn't match: refused, remembered, not retried.
    let mut bad = plugin_release("0.2.0", "bad.zip", plugin("0.2.0", json!({}), false));
    bad.sha256 = Some("0".repeat(64));
    tauri::async_runtime::block_on(world.publish(vec![bad]));
    let refused = install(&window, "dev.example.stats", false, false);
    assert_eq!(
        (refused["status"].as_str(), refused["kind"].as_str()),
        (Some("failed"), Some("integrity"))
    );
    let requests = tauri::async_runtime::block_on(world.server.received_requests())
        .unwrap()
        .len();
    let again = install(&window, "dev.example.stats", false, false);
    assert_eq!(again["kind"], "integrity");
    assert_eq!(
        tauri::async_runtime::block_on(world.server.received_requests())
            .unwrap()
            .len(),
        requests
    );
    assert_eq!(installed().as_deref(), Some("0.1.0"));

    // An archive that writes outside its folder.
    tauri::async_runtime::block_on(world.publish(vec![plugin_release(
        "0.3.0",
        "evil.zip",
        zip(&[("../../evil.txt", b"x".to_vec())]),
    )]));
    assert_eq!(
        install(&window, "dev.example.stats", false, true)["kind"],
        "unsafe"
    );
    assert!(!world.temp.path().join("evil.txt").exists());

    // A package that is another version.
    tauri::async_runtime::block_on(world.publish(vec![plugin_release(
        "0.4.0",
        "liar.zip",
        plugin("0.9.0", json!({}), false),
    )]));
    assert_eq!(
        install(&window, "dev.example.stats", false, true)["kind"],
        "identity"
    );

    // It installs but won't start: the previous version is back.
    tauri::async_runtime::block_on(world.publish(vec![plugin_release(
        "0.5.0",
        "broken.zip",
        plugin("0.5.0", json!({}), true),
    )]));
    let broken = install(&window, "dev.example.stats", false, true);
    assert_eq!(
        (broken["kind"].as_str(), broken["rolledBack"].as_bool()),
        (Some("activation"), Some(true)),
        "{broken}"
    );
    assert_eq!(installed().as_deref(), Some("0.1.0"));
    assert!(world.host.enabled("dev.example.stats"));
    assert!(!world.temp.path().join("addons/journal.json").exists());
    assert!(
        !world.temp.path().join("addons/staging").exists()
            || std::fs::read_dir(world.temp.path().join("addons/staging"))
                .unwrap()
                .count()
                == 0
    );
}

#[test]
fn themes_install_and_uninstall_and_plugins_keep_or_drop_data() {
    let world = tauri::async_runtime::block_on(World::new());
    tauri::async_runtime::block_on(world.publish(vec![
        Release {
            kind: "theme",
            id: "dev.example.paper",
            version: "1.0.0",
            file: "paper.zip",
            bytes: theme("1.0.0"),
            sha256: None,
        },
        plugin_release("0.1.0", "stats.zip", plugin("0.1.0", json!({}), false)),
    ]));
    let (_app, window) = world.app();
    let done = install(&window, "dev.example.paper", false, false);
    assert_eq!(done["status"], "installed", "{done}");
    let theme = world
        .themes
        .list()
        .into_iter()
        .find(|t| t.id == "dev.example.paper")
        .unwrap();
    assert!(theme.problems.is_empty());
    invoke(
        &window,
        "addons_uninstall",
        json!({"id": "dev.example.paper", "keepData": false}),
    )
    .unwrap();
    assert!(
        !world
            .themes
            .list()
            .iter()
            .any(|t| t.id == "dev.example.paper")
    );
    // Built-in themes can't be removed.
    assert!(
        invoke(
            &window,
            "addons_uninstall",
            json!({"id": "caskade.default", "keepData": false})
        )
        .is_err()
    );

    assert_eq!(
        install(&window, "dev.example.stats", true, false)["status"],
        "installed"
    );
    let data = world.temp.path().join("plugin-data/dev.example.stats");
    std::fs::create_dir_all(&data).unwrap();
    invoke(
        &window,
        "addons_uninstall",
        json!({"id": "dev.example.stats", "keepData": true}),
    )
    .unwrap();
    assert!(data.is_dir());
    assert!(!world.temp.path().join("plugins/dev.example.stats").exists());
    // A reinstall is reviewed again: the grant went.
    assert_eq!(
        install(&window, "dev.example.stats", false, false)["status"],
        "needsReview"
    );
    assert_eq!(
        install(&window, "dev.example.stats", true, false)["status"],
        "installed"
    );
    invoke(
        &window,
        "addons_uninstall",
        json!({"id": "dev.example.stats", "keepData": false}),
    )
    .unwrap();
    assert!(!data.exists());
}
