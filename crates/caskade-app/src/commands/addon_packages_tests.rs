//! Catalogue installs against a local server (#182): review, updates that
//! ask for more, integrity, unsafe archives, identity, failed activation
//! with rollback, themes and uninstalls.
use super::*;
use caskade_plugin_host::{HostOptions, Limits, PluginServices};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// Where install progress goes: nowhere, here.
struct Discard;

impl crate::events::Emit for Discard {
    fn emit(&self, _event: Event) -> Result<(), String> {
        Ok(())
    }
}

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
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes/paper"));
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
    folders: AddonFolders,
    jobs: AddonJobs,
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
            folders: AddonFolders {
                plugins: temp.path().join("plugins"),
                themes: temp.path().join("themes"),
                work: temp.path().join("addons"),
            },
            jobs: AddonJobs::default(),
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

    /// The services as the shell holds them: no bus or settings here.
    fn services(&self) -> AddonServices<'_> {
        AddonServices {
            events: Events::new(Discard),
            catalogue: Some(&self.catalogue),
            plugins: Some(&self.host),
            themes: Some(&self.themes),
            bus: None,
            settings: None,
        }
    }

    async fn install(&self, id: &str, accept: bool, retry: bool) -> AddonOutcome {
        addons_install(
            &self.services(),
            &self.folders,
            &self.jobs,
            &self.store,
            id.to_owned(),
            AppMode::Desktop,
            accept,
            retry,
        )
        .await
        .unwrap()
    }

    async fn uninstall(&self, id: &str, keep_data: bool) -> Result<(), CommandError> {
        addons_uninstall(
            &self.services(),
            &self.folders,
            &self.store,
            id.to_owned(),
            keep_data,
        )
        .await
    }

    /// The version of the stats plugin the host has.
    async fn installed(&self) -> Option<String> {
        self.host
            .list()
            .await
            .into_iter()
            .find(|p| p.id == "dev.example.stats")
            .and_then(|p| p.version)
    }
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

fn is_installed(outcome: &AddonOutcome) -> bool {
    matches!(outcome, AddonOutcome::Installed { .. })
}

fn needs_review(outcome: &AddonOutcome) -> bool {
    matches!(outcome, AddonOutcome::NeedsReview { .. })
}

/// The failure's kind and whether the previous version came back.
fn failure(outcome: &AddonOutcome) -> Option<(AddonFailure, bool)> {
    match outcome {
        AddonOutcome::Failed {
            kind, rolled_back, ..
        } => Some((*kind, *rolled_back)),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_installs_after_review_and_updates_keep_data_without_new_grants() {
    let world = World::new().await;
    world
        .publish(vec![plugin_release(
            "0.1.0",
            "stats-0.1.0.zip",
            plugin("0.1.0", json!({}), false),
        )])
        .await;
    let review = world.install("dev.example.stats", false, false).await;
    match &review {
        AddonOutcome::NeedsReview { preview, .. } => {
            assert_eq!(preview.plugin.id, "dev.example.stats");
        }
        other => panic!("not up for review: {other:?}"),
    }
    // Nothing installed before the user accepts.
    assert!(!world.temp.path().join("plugins/dev.example.stats").exists());
    let done = world.install("dev.example.stats", true, false).await;
    assert!(is_installed(&done), "{done:?}");
    assert!(world.host.enabled("dev.example.stats"));
    let data = world.temp.path().join("plugin-data/dev.example.stats");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("saved.json"), b"{}").unwrap();

    // 0.2.0 asks for the network: installed, but off until the user agrees.
    world
        .publish(vec![
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
        ])
        .await;
    let updated = world.install("dev.example.stats", false, false).await;
    match &updated {
        AddonOutcome::Installed { needs_consent, .. } => assert!(*needs_consent),
        other => panic!("not installed: {other:?}"),
    }
    assert!(!world.host.enabled("dev.example.stats"));
    assert!(data.join("saved.json").is_file());
    let records = addons_records(&world.store).await.unwrap();
    assert_eq!(
        records.addons["dev.example.stats"].version.as_deref(),
        Some("0.2.0")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_downloads_are_refused_and_the_installed_version_stays() {
    let world = World::new().await;
    world
        .publish(vec![plugin_release(
            "0.1.0",
            "stats-0.1.0.zip",
            plugin("0.1.0", json!({}), false),
        )])
        .await;
    assert!(is_installed(
        &world.install("dev.example.stats", true, false).await
    ));

    // The checksum doesn't match: refused, remembered, not retried.
    let mut bad = plugin_release("0.2.0", "bad.zip", plugin("0.2.0", json!({}), false));
    bad.sha256 = Some("0".repeat(64));
    world.publish(vec![bad]).await;
    let refused = world.install("dev.example.stats", false, false).await;
    assert_eq!(
        failure(&refused).map(|(kind, _)| kind),
        Some(AddonFailure::Integrity),
        "{refused:?}"
    );
    let requests = world.server.received_requests().await.unwrap().len();
    let again = world.install("dev.example.stats", false, false).await;
    assert_eq!(
        failure(&again).map(|(kind, _)| kind),
        Some(AddonFailure::Integrity)
    );
    assert_eq!(
        world.server.received_requests().await.unwrap().len(),
        requests
    );
    assert_eq!(world.installed().await.as_deref(), Some("0.1.0"));

    // An archive that writes outside its folder.
    world
        .publish(vec![plugin_release(
            "0.3.0",
            "evil.zip",
            zip(&[("../../evil.txt", b"x".to_vec())]),
        )])
        .await;
    assert_eq!(
        failure(&world.install("dev.example.stats", false, true).await).map(|(kind, _)| kind),
        Some(AddonFailure::Unsafe)
    );
    assert!(!world.temp.path().join("evil.txt").exists());

    // A package that is another version.
    world
        .publish(vec![plugin_release(
            "0.4.0",
            "liar.zip",
            plugin("0.9.0", json!({}), false),
        )])
        .await;
    assert_eq!(
        failure(&world.install("dev.example.stats", false, true).await).map(|(kind, _)| kind),
        Some(AddonFailure::Identity)
    );

    // It installs but won't start: the previous version is back.
    world
        .publish(vec![plugin_release(
            "0.5.0",
            "broken.zip",
            plugin("0.5.0", json!({}), true),
        )])
        .await;
    let broken = world.install("dev.example.stats", false, true).await;
    assert_eq!(
        failure(&broken),
        Some((AddonFailure::Activation, true)),
        "{broken:?}"
    );
    assert_eq!(world.installed().await.as_deref(), Some("0.1.0"));
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

#[tokio::test(flavor = "multi_thread")]
async fn themes_install_and_uninstall_and_plugins_keep_or_drop_data() {
    let world = World::new().await;
    world
        .publish(vec![
            Release {
                kind: "theme",
                id: "dev.example.paper",
                version: "1.0.0",
                file: "paper.zip",
                bytes: theme("1.0.0"),
                sha256: None,
            },
            plugin_release("0.1.0", "stats.zip", plugin("0.1.0", json!({}), false)),
        ])
        .await;
    let done = world.install("dev.example.paper", false, false).await;
    assert!(is_installed(&done), "{done:?}");
    let theme = world
        .themes
        .list()
        .into_iter()
        .find(|t| t.id == "dev.example.paper")
        .unwrap();
    assert!(theme.problems.is_empty());
    world.uninstall("dev.example.paper", false).await.unwrap();
    assert!(
        !world
            .themes
            .list()
            .iter()
            .any(|t| t.id == "dev.example.paper")
    );
    // Built-in themes can't be removed.
    assert!(world.uninstall("caskade.default", false).await.is_err());

    assert!(is_installed(
        &world.install("dev.example.stats", true, false).await
    ));
    let data = world.temp.path().join("plugin-data/dev.example.stats");
    std::fs::create_dir_all(&data).unwrap();
    world.uninstall("dev.example.stats", true).await.unwrap();
    assert!(data.is_dir());
    assert!(!world.temp.path().join("plugins/dev.example.stats").exists());
    // A reinstall is reviewed again: the grant went.
    assert!(needs_review(
        &world.install("dev.example.stats", false, false).await
    ));
    assert!(is_installed(
        &world.install("dev.example.stats", true, false).await
    ));
    world.uninstall("dev.example.stats", false).await.unwrap();
    assert!(!data.exists());
}
