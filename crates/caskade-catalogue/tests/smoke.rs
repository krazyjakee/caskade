//! The catalogue end to end (#184): packages built and the catalogue
//! signed with the maintainer tooling, checked by `caskade-catalogue
//! check`, served by a local server and installed into a clean profile;
//! then an update, and a broken release that the check catches and an
//! install rolls back, for a plugin and a theme.
use caskade_addons::{AppMode, CatalogueService, author, package::AddonFailure};
use caskade_app::commands::addon_packages::{
    AddonFolders, AddonJobs, AddonOutcome, AddonServices, addons_install, addons_records,
};
use caskade_app::events::{Emit, Event, Events};
use caskade_core::{db::LibraryStore, themes::ThemeService};
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices};
use serde_json::json;
use std::path::{Path, PathBuf};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate, matchers::method};

/// Serves a folder, as a static host serving release assets would.
struct Folder(PathBuf);

impl Respond for Folder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let relative = request.url.path().trim_start_matches('/');
        match std::fs::read(self.0.join(relative)) {
            Ok(bytes) if !relative.contains("..") => {
                ResponseTemplate::new(200).set_body_bytes(bytes)
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

/// Where install progress goes: nowhere, here.
struct Discard;

impl Emit for Discard {
    fn emit(&self, _event: Event) -> Result<(), String> {
        Ok(())
    }
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// The plugin's source at `version`; `broken` ships a backend that
/// isn't a component, so it can't start.
fn plugin_source(root: &Path, version: &str, broken: bool) {
    let mut manifest = json!({
        "id": "dev.example.smoke", "name": "Smoke", "version": version, "api": "^0.1",
        "description": "A page.", "author": "Example", "ui": "dist/ui.js",
        "permissions": {"library": "read"},
        "contributes": {"views": [{"id": "smoke", "title": "Smoke"}]}
    });
    let _ = std::fs::remove_dir_all(root.join("dist"));
    write(
        root,
        &[(
            "dist/ui.js",
            &format!("export default {{v: \"{version}\"}}"),
        )],
    );
    if broken {
        manifest["backend"] = json!("dist/backend.wasm");
        write(root, &[("dist/backend.wasm", "not a component")]);
    }
    write(root, &[("plugin.json", &manifest.to_string())]);
}

fn theme_source(root: &Path, version: &str, accent: &str) {
    write(
        root,
        &[
            (
                "theme.json",
                &json!({"id": "dev.example.ember", "name": "Ember", "version": version,
                    "author": "Example", "extends": "caskade.default"})
                .to_string(),
            ),
            (
                "tokens.json",
                &json!({"color": {"accent": {"$root": {"$value": accent}}}}).to_string(),
            ),
        ],
    );
}

struct Smoke {
    temp: tempfile::TempDir,
    server: MockServer,
    key: minisign::KeyPair,
    public_key: String,
    store: LibraryStore,
    host: PluginHost,
    themes: ThemeService,
    catalogue: CatalogueService,
    bus: caskade_core::bus::Bus,
    folders: AddonFolders,
    jobs: AddonJobs,
}

impl Smoke {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(Folder(temp.path().join("site")))
            .mount(&server)
            .await;
        let key = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public_key = key.pk.to_box().unwrap().to_string();
        // A clean profile.
        let profile = temp.path().join("profile");
        let store = LibraryStore::in_memory().await.unwrap();
        let host = PluginHost::new(
            PluginServices {
                store: store.clone(),
                bus: None,
                settings: None,
            },
            HostOptions {
                folders: vec![profile.join("plugins")],
                data: profile.join("plugin-data"),
                limits: Limits::default(),
            },
        )
        .unwrap();
        let catalogue = CatalogueService::new(
            profile.join("cache"),
            &format!("{}/catalogue.json", server.uri()),
            Some(&public_key),
        )
        .unwrap();
        Self {
            themes: ThemeService::new(profile.join("themes")),
            bus: caskade_core::bus::Bus::default(),
            folders: AddonFolders {
                plugins: profile.join("plugins"),
                themes: profile.join("themes"),
                work: profile.join("addons"),
            },
            jobs: AddonJobs::default(),
            temp,
            server,
            key,
            public_key,
            store,
            host,
            catalogue,
        }
    }

    /// Builds the source into the served folder, keeping the releases
    /// already published, and signs the catalogue.
    fn publish(&self) {
        let src = self.temp.path().join("src");
        std::fs::write(
            src.join("catalogue.json"),
            json!({"format": 1, "generated": "2026-10-30T00:00:00Z", "packages": [
                {"path": "smoke", "kind": "plugin", "summary": "A page.", "license": "MIT"},
                {"path": "ember", "kind": "theme", "summary": "Warm.", "license": "MIT"}
            ]})
            .to_string(),
        )
        .unwrap();
        let site = self.temp.path().join("site");
        let published = self.temp.path().join("published.json");
        let previous = site.join("catalogue.json").is_file().then(|| {
            std::fs::copy(site.join("catalogue.json"), &published).unwrap();
            published.as_path()
        });
        author::build(&src.join("catalogue.json"), &site, previous).unwrap();
        let bytes = std::fs::read(site.join("catalogue.json")).unwrap();
        let signature = minisign::sign(
            Some(&self.key.pk),
            &self.key.sk,
            bytes.as_slice(),
            Some("caskade catalogue"),
            None,
        )
        .unwrap();
        std::fs::write(site.join("catalogue.json.sig"), signature.to_string()).unwrap();
    }

    /// `caskade-catalogue check` against the served catalogue.
    async fn check(&self, name: &str) -> author::Checked {
        caskade_catalogue::check(
            &format!("{}/catalogue.json", self.server.uri()),
            Some(&self.public_key),
            &self.temp.path().join(name),
        )
        .await
        .unwrap()
    }

    /// The services as the shell holds them, with the bus the UI's reloads
    /// follow.
    fn services(&self) -> AddonServices<'_> {
        AddonServices {
            events: Events::new(Discard),
            catalogue: Some(&self.catalogue),
            plugins: Some(&self.host),
            themes: Some(&self.themes),
            bus: Some(&self.bus),
            settings: None,
        }
    }

    async fn install(&self, id: &str, accept: bool) -> AddonOutcome {
        addons_install(
            &self.services(),
            &self.folders,
            &self.jobs,
            &self.store,
            id.to_owned(),
            AppMode::Desktop,
            accept,
            false,
        )
        .await
        .unwrap()
    }

    fn installed_plugin(&self) -> Option<String> {
        self.host
            .manifest("dev.example.smoke")
            .map(|manifest| manifest.version.to_string())
    }

    fn installed_theme(&self) -> Option<String> {
        self.themes
            .list()
            .into_iter()
            .find(|theme| theme.id == "dev.example.ember")
            .map(|theme| theme.version)
    }
}

fn is_installed(outcome: &AddonOutcome) -> bool {
    matches!(outcome, AddonOutcome::Installed { .. })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_built_catalogue_installs_activates_updates_and_rolls_back() {
    let smoke = Smoke::new().await;
    let src = smoke.temp.path().join("src");
    plugin_source(&src.join("smoke"), "0.1.0", false);
    theme_source(&src.join("ember"), "1.0.0", "#cc4a0e");
    smoke.publish();

    // The maintainer's check passes, and the app takes the signed catalogue.
    let checked = smoke.check("check-1").await;
    assert!(checked.problems.is_empty(), "{:?}", checked.problems);
    assert_eq!(checked.releases.len(), 2);
    let status = smoke.catalogue.refresh().await;
    assert!(status.verified && status.error.is_none(), "{status:?}");

    // Install: the plugin after its review, then it's on; the theme compiles
    // over the default it extends.
    assert!(matches!(
        smoke.install("dev.example.smoke", false).await,
        AddonOutcome::NeedsReview { .. }
    ));
    let done = smoke.install("dev.example.smoke", true).await;
    assert!(is_installed(&done), "{done:?}");
    assert!(smoke.host.enabled("dev.example.smoke"));
    let plugin = smoke
        .host
        .list()
        .await
        .into_iter()
        .find(|plugin| plugin.id == "dev.example.smoke")
        .unwrap();
    assert!(plugin.errors.is_empty() && plugin.has_ui, "{plugin:?}");
    let done = smoke.install("dev.example.ember", false).await;
    assert!(is_installed(&done), "{done:?}");
    let css = smoke.themes.stylesheet("dev.example.ember").unwrap();
    assert!(css.contains("#cc4a0e"));

    // Update both; the catalogue keeps the first releases.
    plugin_source(&src.join("smoke"), "0.2.0", false);
    theme_source(&src.join("ember"), "1.1.0", "#d04a0a");
    smoke.publish();
    assert!(smoke.check("check-2").await.problems.is_empty());
    smoke.catalogue.refresh().await;
    let entry = smoke.catalogue.catalogue();
    assert_eq!(entry.entry("dev.example.smoke").unwrap().releases.len(), 2);
    let mut events = smoke.bus.stream();
    assert!(is_installed(
        &smoke.install("dev.example.smoke", false).await
    ));
    assert!(is_installed(
        &smoke.install("dev.example.ember", false).await
    ));
    // The UI reloads the plugin's UI half on this.
    assert!(matches!(
        events.try_recv(),
        Ok(caskade_core::bus::Event::PluginsChanged { plugin }) if plugin == "dev.example.smoke"
    ));
    assert_eq!(smoke.installed_plugin().as_deref(), Some("0.2.0"));
    assert_eq!(smoke.installed_theme().as_deref(), Some("1.1.0"));
    assert!(smoke.host.enabled("dev.example.smoke"));
    assert!(
        smoke
            .themes
            .stylesheet("dev.example.ember")
            .unwrap()
            .contains("#d04a0a")
    );

    // A plugin release that won't start and a theme that doesn't check:
    // the maintainer's check names both before publishing ...
    plugin_source(&src.join("smoke"), "0.3.0", true);
    theme_source(&src.join("ember"), "1.2.0", "not a colour");
    smoke.publish();
    let problems = smoke.check("check-3").await.problems.join("\n");
    assert!(
        problems.contains("dev.example.smoke 0.3.0: it didn't start"),
        "{problems}"
    );
    assert!(problems.contains("dev.example.ember 1.2.0: "), "{problems}");
    // ... and published anyway, installs put the previous versions back.
    smoke.catalogue.refresh().await;
    let mut events = smoke.bus.stream();
    let failed = smoke.install("dev.example.smoke", false).await;
    match &failed {
        AddonOutcome::Failed {
            kind, rolled_back, ..
        } => {
            assert_eq!(*kind, AddonFailure::Activation);
            assert!(*rolled_back);
        }
        other => panic!("not refused: {other:?}"),
    }
    // A rollback is announced too: the UI half to load is the previous one's.
    assert!(matches!(
        events.try_recv(),
        Ok(caskade_core::bus::Event::PluginsChanged { plugin }) if plugin == "dev.example.smoke"
    ));
    let failed = smoke.install("dev.example.ember", false).await;
    assert!(matches!(failed, AddonOutcome::Failed { .. }), "{failed:?}");
    assert_eq!(smoke.installed_plugin().as_deref(), Some("0.2.0"));
    assert_eq!(smoke.installed_theme().as_deref(), Some("1.1.0"));
    assert!(smoke.themes.stylesheet("dev.example.ember").is_ok());
    let records = addons_records(&smoke.store).await.unwrap();
    let record = &records.addons["dev.example.smoke"];
    assert_eq!(record.version.as_deref(), Some("0.2.0"));
    assert_eq!(
        record
            .rejected
            .as_ref()
            .map(|rejected| rejected.version.as_str()),
        Some("0.3.0")
    );
}

/// The starter collection (`scripts/addons/starter.json`) as built and
/// published: every package installs into a clean profile from a local
/// server. Needs the examples built first; see `docs/plugins/publishing.md`:
/// `CASKADE_STARTER_DIR=<built folder> cargo test -p caskade-catalogue
/// --test smoke starter_collection -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the starter collection built (CASKADE_STARTER_DIR)"]
async fn starter_collection_installs_in_a_clean_profile() {
    let built = PathBuf::from(std::env::var("CASKADE_STARTER_DIR").expect("CASKADE_STARTER_DIR"));
    let smoke = Smoke::new().await;
    let site = smoke.temp.path().join("site");
    // Everything the build wrote: packages, media and the catalogue.
    copy_tree(&built, &site);
    let bytes = std::fs::read(site.join("catalogue.json")).unwrap();
    let signature = minisign::sign(
        Some(&smoke.key.pk),
        &smoke.key.sk,
        bytes.as_slice(),
        None,
        None,
    )
    .unwrap();
    std::fs::write(site.join("catalogue.json.sig"), signature.to_string()).unwrap();

    let checked = smoke.check("check").await;
    assert!(checked.problems.is_empty(), "{:?}", checked.problems);
    let status = smoke.catalogue.refresh().await;
    assert!(status.verified, "{status:?}");
    for entry in smoke.catalogue.catalogue().entries {
        let supported = entry.releases.iter().any(|release| {
            release.os.is_empty() || release.os.contains(&caskade_addons::AddonOs::current())
        });
        let first = smoke.install(&entry.id, true).await;
        if !supported {
            assert!(
                matches!(first, AddonOutcome::Failed { .. }),
                "{}: {first:?}",
                entry.id
            );
            continue;
        }
        match &first {
            AddonOutcome::Installed { version, .. } => println!("{} {version} installed", entry.id),
            other => panic!("{}: {other:?}", entry.id),
        }
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
