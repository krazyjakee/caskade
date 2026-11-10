//! Guest providers behind the core traits (#126): library sources import
//! through the same path as built-ins, runners launch `LaunchSpec::Plugin`
//! entries, metadata fills games, install handlers and URL resolvers answer
//! through the #109 traits.

use crate::guest;
use caskade_core::{
    bus::{Bus, InstallInput},
    db::LibraryStore,
    model::{Game, InstallState},
};
use caskade_installer::{ArtifactId, ArtifactNode, Format, PayloadPlatform, Role, Step};
use caskade_plugin_host::{HostError, HostOptions, Limits, PluginHost, PluginServices};

const ID: &str = "dev.caskade.providers";

struct Setup {
    _temp: tempfile::TempDir,
    store: LibraryStore,
    bus: Bus,
    host: PluginHost,
}

async fn setup(permissions: serde_json::Value) -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("plugins/providers");
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(guest(), dir.join("dist/backend.wasm")).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::json!({
            "id": ID,
            "name": "Providers",
            "version": "0.1.0",
            "api": "^0.1",
            "backend": "dist/backend.wasm",
            "permissions": permissions,
            "contributes": {"providers": [
                "library-source", "metadata-provider", "runner", "install-handler", "url-resolver"
            ]}
        })
        .to_string(),
    )
    .unwrap();
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
    host.set_enabled(ID, true).await.unwrap();
    host.register_providers(&bus);
    Setup {
        _temp: temp,
        store,
        bus,
        host,
    }
}

fn everything() -> serde_json::Value {
    serde_json::json!({"network": ["example.com"], "process": true, "library": "write"})
}

async fn library_scan(s: &Setup) -> caskade_sources::ScanOutcome {
    // No built-in sources: only the plugin's games are scanned.
    let discovery = caskade_sources::DiscoveryService::with_sources(
        s.store.clone(),
        s.bus.clone(),
        caskade_discovery::HostEnv::live(),
        Vec::new(),
    );
    discovery
        .scan(
            &caskade_discovery::Config::default(),
            &caskade_sources::ImportOptions::default(),
            &caskade_discovery::Cancel::new(),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_library_source_imports_through_the_built_in_path() {
    let s = setup(everything()).await;
    let mut game = caskade_discovery::DetectedGame::new(
        caskade_discovery::SourceId::new("pretending-to-be-steam"),
        "celeste",
        "Celeste",
    );
    game.state = InstallState::Installed;
    game.confidence = caskade_discovery::Confidence::Certain;
    let scan = serde_json::to_string(&[game]).unwrap();
    s.host.request(ID, "store:scan", &scan).await.unwrap();

    let outcome = library_scan(&s).await;
    let report = outcome
        .scan
        .sources
        .iter()
        .find(|report| report.source.as_str() == "plugin:dev.caskade.providers")
        .unwrap();
    assert_eq!(report.count, 1);
    assert!(report.error.is_none());
    let page = s
        .store
        .list_games(&caskade_core::model::ListGames::default())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1, "{page:?}");
    let entries = s.store.entries_for_game(&page.items[0].id).await.unwrap();
    // A plugin speaks only for itself.
    assert_eq!(
        entries[0].source_id.as_str(),
        "plugin:dev.caskade.providers"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_library_source_needs_library_write() {
    let s = setup(serde_json::json!({"library": "read"})).await;
    let outcome = library_scan(&s).await;
    let report = outcome
        .scan
        .sources
        .iter()
        .find(|report| report.source.as_str() == "plugin:dev.caskade.providers")
        .unwrap();
    assert!(
        format!("{:?}", report.error).contains("library: write"),
        "{report:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_metadata_provider_fills_a_game() {
    let s = setup(everything()).await;
    let game = s
        .store
        .upsert_game(&Game::new("Hollow Knight"))
        .await
        .unwrap();
    s.host
        .request(
            ID,
            "store:metadata",
            r#"{"description": "Bugs and nails.", "genres": ["Metroidvania"],
                "artwork": [{"kind": "cover", "url": "https://example.com/hk.png"}]}"#,
        )
        .await
        .unwrap();
    let (filled, artwork) = s
        .host
        .apply_metadata(ID, &game.id, &["description".into(), "genres".into()])
        .await
        .unwrap();
    assert_eq!(filled.description, "Bugs and nails.");
    assert_eq!(filled.genres, ["Metroidvania"]);
    assert_eq!(artwork[0].url, "https://example.com/hk.png");
    assert_eq!(
        s.store.game(&game.id).await.unwrap().unwrap().genres,
        ["Metroidvania"]
    );
    // It saw the game and the fields.
    let seen = s
        .store
        .plugin_kv_get(ID, "looked-up")
        .await
        .unwrap()
        .unwrap();
    let seen = String::from_utf8(seen).unwrap();
    assert!(
        seen.contains("Hollow Knight") && seen.ends_with("description,genres"),
        "{seen}"
    );

    let offline = setup(serde_json::json!({"library": "write"})).await;
    let game = offline.store.upsert_game(&Game::new("X")).await.unwrap();
    assert!(matches!(
        offline.host.apply_metadata(ID, &game.id, &[]).await,
        Err(HostError::Plugin { code, .. }) if code == "permission_denied"
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn install_handlers_and_url_resolvers_answer_through_the_core_traits() {
    let s = setup(everything()).await;
    let handlers = s.bus.install_handlers();
    let handler = handlers
        .iter()
        .find(|handler| handler.id() == "plugin:dev.caskade.providers")
        .unwrap();
    // The guest reads the input through the host (it has no `fs` grant)
    // and claims files that start with `CUSTOM`.
    let files = tempfile::tempdir().unwrap();
    let input = |name: &str, bytes: &[u8]| {
        let path = files.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        InstallInput {
            path,
            artifact: ArtifactNode {
                id: ArtifactId("input".into()),
                path: name.into(),
                format: Format::Unknown,
                platform: PayloadPlatform::Unknown,
                role: Role::Unknown,
                size: Some(bytes.len() as u64),
                facts: Default::default(),
                children: vec![],
                children_complete: true,
            },
        }
    };
    let custom = input("game.custom", b"CUSTOMpayload");
    let candidate = handler.propose(&custom).await.unwrap().unwrap();
    assert_eq!(candidate.handler.0, "plugin:dev.caskade.providers");
    assert!(!candidate.requires.executes_code);
    assert_eq!(
        candidate.steps,
        [Step::CopyRange {
            from: ArtifactId("input".into()),
            offset: 6,
            length: 7,
            path: "game.bin".into(),
        }]
    );
    assert!(
        handler
            .propose(&input("game.zip", b"PK\x03\x04"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        s.host.request(ID, "read-input", "").await,
        Err(HostError::Plugin { code, .. }) if code == "permission_denied"
    ));
    // Plans that would run a program, or read files other than the input,
    // are refused.
    for plan in [
        r#"{"steps": [{"type": "run", "value": {"program": {"type": "tool", "value": "sh"}, "args": [], "env": {}, "runtime": {"type": "native"}, "timeout_secs": 5, "expect": []}}]}"#,
        r#"{"steps": [{"type": "copy_range", "value": {"from": "other", "offset": 0, "length": 1, "path": "x"}}]}"#,
        r#"{"steps": []}"#,
    ] {
        s.host.request(ID, "store:plan", plan).await.unwrap();
        assert!(handler.propose(&custom).await.is_err(), "{plan}");
    }

    let resolvers = s.bus.url_resolvers();
    let resolver = resolvers
        .iter()
        .find(|resolver| resolver.id() == "plugin:dev.caskade.providers")
        .unwrap();
    let page = url::Url::parse("https://example.com/page/42").unwrap();
    assert!(resolver.matches(&page));
    // Hosts outside its network grant aren't its business.
    assert!(!resolver.matches(&url::Url::parse("https://other.org/page/1").unwrap()));
    let found = resolver.resolve(&page).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].url.as_str(), "https://example.com/download/42");
    assert_eq!(found[0].headers["x-token"], "secret");
    // Its redirects and DNS answers are held to the grant as it downloads.
    assert!(
        found[0]
            .allowed_hosts
            .as_ref()
            .is_some_and(|hosts| hosts.iter().any(|host| host == "example.com"))
    );
    // Downloads must be on a granted host too (#157): a page elsewhere
    // that resolves to a local service is refused.
    let local = url::Url::parse("http://127.0.0.1:8384/page/1").unwrap();
    assert!(s.host.url_resolve(ID, &local).await.is_err());
    // Disabled plugins answer nothing.
    s.host.set_enabled(ID, false).await.unwrap();
    assert!(handler.propose(&custom).await.unwrap().is_none());
    assert!(!resolver.matches(&page));
}

/// #126: a `LaunchSpec::Plugin` entry launches through the guest runner;
/// the core spawns and tracks it.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_runner_launches_a_game_and_its_session_is_recorded() {
    use caskade_core::{
        launch::{LaunchCommand, LaunchService},
        model::{GameEntry, LaunchSpec, SourceId},
    };
    use caskade_plugin_host::providers::PluginRunners;
    use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
    let s = setup(everything()).await;
    let temp = tempfile::tempdir().unwrap();
    let exe = temp.path().join("fixture-game");
    caskade_core::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    );
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let token = temp.path().join("token");
    let mut command = LaunchCommand::executable(&exe);
    command.args = vec!["--token".into(), token.to_string_lossy().into_owned()];
    s.host
        .request(
            ID,
            "store:runner",
            &serde_json::to_string(&command).unwrap(),
        )
        .await
        .unwrap();

    let game = s
        .store
        .upsert_game(&Game::new("Runner game"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(
        &game.id,
        SourceId::new("plugin:dev.caskade.providers"),
        "g1",
    );
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Plugin {
        plugin_id: ID.into(),
        runner_id: "native".into(),
        payload: serde_json::json!({"game": "g1"}),
    });
    let entry = s.store.upsert_entry(&entry).await.unwrap();
    let launcher = LaunchService::new(s.store.clone())
        .with_aliases(None)
        .with_plugins(Arc::new(PluginRunners::of(s.host.clone())));
    let handle = launcher.launch(&entry.id).await.unwrap();
    let session = tokio::time::timeout(Duration::from_secs(60), handle.wait())
        .await
        .expect("the game exits")
        .unwrap();
    launcher.shutdown();
    assert!(session.ended_at_ms.is_some());
    assert!(token.is_file(), "the runner's command ran");
    assert_eq!(
        s.store.sessions_for_game(&game.id, 10).await.unwrap().len(),
        1
    );
    let spec = s
        .store
        .plugin_kv_get(ID, "prepared")
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8(spec)
            .unwrap()
            .contains(r#""runnerId":"native""#)
    );

    // Without the process grant, the runner is refused.
    let unprocessed = setup(serde_json::json!({"library": "write"})).await;
    let runners = PluginRunners::of(unprocessed.host.clone());
    use caskade_core::launch::PluginRunner;
    assert!(
        runners
            .resolve(ID, "native", &serde_json::json!({}))
            .await
            .is_err()
    );
}
