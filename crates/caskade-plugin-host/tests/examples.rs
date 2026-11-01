//! #133: the example plugins (`plugins/examples/`) install from their
//! folders, ask for exactly what their manifests declare, and work: GameMode
//! wraps a real fixture launch in (a fake) `gamemoderun`, HowLongToBeat looks a
//! title up on a local mock server and caches it, Playtime stats installs as
//! a UI-only plugin (its view is tested in Vitest). Needs Node with the
//! workspace's packages and the wasm32-wasip2 target.

use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    launch::{LaunchBefore, LaunchCommand, LaunchService},
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
    settings::SettingsService,
};
use caskade_plugin_host::{
    HostOptions, Limits, Manifest, PluginHooks, PluginHost, PluginServices, settings_namespace,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/examples")
        .canonicalize()
        .unwrap()
}

/// Builds every example once into its own dist/.
fn built() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let status = Command::new("node")
            .arg(examples().join("build.mjs"))
            .args(["gamemode", "howlongtobeat", "playtime-stats"])
            .env(
                "CARGO_TARGET_DIR",
                Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-guest"),
            )
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("run node plugins/examples/build.mjs");
        assert!(status.success(), "building the examples failed");
        examples()
    })
}

/// A copy of an example to install from, optionally with manifest changes.
fn copy(example: &str, into: &Path, change: impl FnOnce(&mut Value)) -> PathBuf {
    let from = built().join(example);
    let to = into.join(example);
    for file in [
        "plugin.json",
        "dist/ui.js",
        "dist/ui.css",
        "dist/backend.wasm",
    ] {
        if from.join(file).is_file() {
            std::fs::create_dir_all(to.join(file).parent().unwrap()).unwrap();
            std::fs::copy(from.join(file), to.join(file)).unwrap();
        }
    }
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(to.join("plugin.json")).unwrap()).unwrap();
    change(&mut manifest);
    std::fs::write(to.join("plugin.json"), manifest.to_string()).unwrap();
    to
}

struct Setup {
    temp: tempfile::TempDir,
    store: LibraryStore,
    settings: SettingsService,
    bus: Bus,
    host: PluginHost,
}

async fn setup() -> Setup {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("plugins")).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    let bus = Bus::default();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
            settings: Some(settings.clone()),
        },
        HostOptions {
            folders: vec![temp.path().join("plugins")],
            data: temp.path().join("plugin-data"),
            limits: Limits::default(),
        },
    )
    .unwrap();
    host.discover().await.unwrap();
    Setup {
        temp,
        store,
        settings,
        bus,
        host,
    }
}

/// Reviews and installs a folder: the review shows exactly the manifest's
/// permissions, and that is what is granted.
async fn install(s: &Setup, folder: &Path) -> String {
    let manifest = Manifest::read(folder).unwrap();
    let preview = s.host.preview_folder(folder).await.unwrap();
    assert!(
        preview.plugin.errors.is_empty(),
        "{:?}",
        preview.plugin.errors
    );
    assert_eq!(
        preview.plugin.permissions.as_ref(),
        Some(&manifest.permissions)
    );
    let installed = s.host.install_folder(folder).await.unwrap();
    assert_eq!(
        s.host.grants(&installed.id).await,
        Some(manifest.permissions.clone())
    );
    installed.id
}

#[tokio::test]
async fn playtime_stats_installs_as_a_ui_only_plugin_asking_for_nothing() {
    let s = setup().await;
    let folder = copy("playtime-stats", s.temp.path(), |_| {});
    let id = install(&s, &folder).await;
    let summary = s
        .host
        .list()
        .await
        .into_iter()
        .find(|plugin| plugin.id == id)
        .unwrap();
    assert!(summary.has_ui && !summary.has_backend);
    assert_eq!(summary.permissions, Some(Default::default()));
}

#[tokio::test]
async fn gamemode_wraps_a_real_launch_in_gamemoderun() {
    let s = setup().await;
    let folder = copy("gamemode", s.temp.path(), |_| {});
    let id = install(&s, &folder).await;
    let hooks = PluginHooks::new(s.host.clone(), s.bus.clone());
    hooks.sync().await;
    let namespace = settings_namespace(&id);

    // A fake gamemoderun: records what it was asked to run, then runs it.
    let record = s.temp.path().join("gamemoderun.args");
    let fake = s.temp.path().join("bin/gamemoderun");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexec \"$@\"\n",
            record.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    s.settings
        .set(
            &namespace,
            json!({"enabled": true, "command": fake.to_string_lossy()}),
            None,
        )
        .await
        .unwrap();

    let game_dir = s.temp.path().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::copy("/bin/true", game_dir.join("game")).unwrap();
    let game = Game::new("Fixture");
    s.store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: game_dir.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: game_dir.join("game"),
        args: vec!["--level".into(), "1".into()],
        cwd: Some(game_dir.clone()),
        env: BTreeMap::new(),
    });
    s.store.upsert_entry(&entry).await.unwrap();
    let launch = LaunchService::new(s.store.clone())
        .with_bus(s.bus.clone())
        .with_aliases(None);
    let session = launch
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    let ran = std::fs::read_to_string(&record).expect("gamemoderun ran");
    assert_eq!(
        ran.lines().collect::<Vec<_>>(),
        [
            game_dir
                .join("game")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap(),
            "--level",
            "1"
        ]
    );

    // Turned off in its settings, it leaves launches alone.
    s.settings
        .set(
            &namespace,
            json!({"enabled": false, "command": fake.to_string_lossy()}),
            None,
        )
        .await
        .unwrap();
    let command = LaunchCommand::executable("/games/game");
    assert_eq!(
        s.bus.run::<LaunchBefore>(command.clone()).await.outcome,
        caskade_core::bus::Outcome::Continue(command)
    );
}

/// Answers HowLongToBeat searches with fixed times and counts requests.
fn mock_hltb(hits: Arc<AtomicUsize>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            hits.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut length, mut chunked) = (0, false);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                let header = line.to_ascii_lowercase();
                if let Some(value) = header.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                chunked |= header.starts_with("transfer-encoding:") && header.contains("chunked");
                line.clear();
            }
            let mut body = Vec::new();
            if chunked {
                loop {
                    line.clear();
                    reader.read_line(&mut line).ok();
                    let size = usize::from_str_radix(line.trim(), 16).unwrap_or(0);
                    let mut chunk = vec![0; size + 2];
                    reader.read_exact(&mut chunk).ok();
                    if size == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..size]);
                }
            } else {
                body.resize(length, 0);
                reader.read_exact(&mut body).ok();
            }
            let request: Value = serde_json::from_slice(&body).unwrap_or_default();
            assert_eq!(request["searchTerms"], json!(["Celeste"]));
            let answer = json!({"data": [
                {"game_name": "Celeste", "comp_main": 28800, "comp_plus": 41400, "comp_100": 147600}
            ]})
            .to_string();
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .ok();
        }
    });
    port
}

#[tokio::test]
async fn howlongtobeat_looks_titles_up_on_its_allowed_host_and_caches_them() {
    let s = setup().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let port = mock_hltb(hits.clone());
    // The real manifest allows howlongtobeat.com only; this copy also allows
    // the local mock server.
    let folder = copy("howlongtobeat", s.temp.path(), |manifest| {
        manifest["permissions"]["network"] =
            json!(["howlongtobeat.com", format!("127.0.0.1:{port}")]);
    });
    let id = install(&s, &folder).await;
    s.settings
        .set(
            &settings_namespace(&id),
            json!({"endpoint": format!("http://127.0.0.1:{port}/api/search")}),
            None,
        )
        .await
        .unwrap();
    let celeste = json!({"title": "Celeste"}).to_string();
    let lookup = || s.host.request(&id, "lookup", &celeste);
    let times: Value = serde_json::from_str(&lookup().await.unwrap()).unwrap();
    assert_eq!(
        times,
        json!({"title": "Celeste", "mainHours": 8.0, "extrasHours": 11.5, "completionistHours": 41.0})
    );
    // The second lookup is answered from the plugin's key-value store.
    assert_eq!(
        serde_json::from_str::<Value>(&lookup().await.unwrap()).unwrap(),
        times
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    // Another host is refused by the network permission.
    s.settings
        .set(
            &settings_namespace(&id),
            json!({"endpoint": "http://localhost:9/api/search"}),
            None,
        )
        .await
        .unwrap();
    assert!(
        s.host
            .request(&id, "lookup", &json!({"title": "Hades"}).to_string())
            .await
            .is_err()
    );
}
