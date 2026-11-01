//! M4 exit (#134, TECH-PLAN §8): the three example plugins installed from
//! their folders work together, and a deliberately broken one (`crashy`)
//! doesn't affect the app: its calls fail alone, its launch.before step is
//! skipped, the circuit breaker turns it off with a notice, and launches,
//! playtime and the other plugins carry on. Its UI half's placeholders are
//! tested in Vitest (`apps/desktop/src/plugins/examples.test.tsx`).
//! Needs Node with the workspace's packages and the wasm32-wasip2 target.
#![cfg(unix)]

use crate::ipc::invoke;
use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
    launch::LaunchService,
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
    time::Duration,
};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

const EXAMPLES: [&str; 4] = ["gamemode", "howlongtobeat", "playtime-stats", "crashy"];

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugins/examples")
        .canonicalize()
        .unwrap()
}

fn build() {
    let status = Command::new("node")
        .arg(examples().join("build.mjs"))
        .args(EXAMPLES)
        .env(
            "CARGO_TARGET_DIR",
            Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-guest"),
        )
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("run node plugins/examples/build.mjs");
    assert!(status.success(), "building the examples failed");
}

/// What a user downloads: the manifest and the built halves.
fn download(example: &str, into: &Path, change: impl FnOnce(&mut Value)) -> PathBuf {
    let (from, to) = (examples().join(example), into.join(example));
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

/// A HowLongToBeat search endpoint answering every title with fixed times.
fn mock_hltb() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
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
                }
            } else {
                let mut body = vec![0; length];
                reader.read_exact(&mut body).ok();
            }
            let answer = json!({"data": [
                {"game_name": "Fixture", "comp_main": 7200, "comp_plus": 10800, "comp_100": 18000}
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

#[test]
fn three_example_plugins_work_and_a_crashing_one_does_not_affect_the_app() {
    build();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let plugins = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let port = mock_hltb();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let settings = SettingsService::new(store.clone());
    let bus = Bus::default();
    let host = PluginHost::new(
        PluginServices {
            store: store.clone(),
            bus: Some(bus.clone()),
            settings: Some(settings.clone()),
        },
        HostOptions {
            folders: vec![plugins.clone()],
            data: temp.path().join("plugin-data"),
            limits: Limits {
                memory: 64 << 20,
                call_timeout: Duration::from_millis(300),
                ..Limits::default()
            },
        },
    )
    .unwrap();

    tauri::async_runtime::block_on(async {
        host.discover().await.unwrap();
        // Install each from its folder: the review shows exactly what the
        // manifest asks for, and that is what is granted.
        for example in EXAMPLES {
            let folder = download(example, &downloads, |manifest| {
                if example == "howlongtobeat" {
                    // The local mock server stands in for howlongtobeat.com;
                    // a local address needs its port in the grant (CSK-03).
                    manifest["permissions"]["network"] =
                        json!(["howlongtobeat.com", format!("127.0.0.1:{port}")]);
                }
            });
            let wanted = Manifest::read(&folder).unwrap().permissions;
            let preview = host.preview_folder(&folder).await.unwrap();
            assert_eq!(
                preview.plugin.permissions.as_ref(),
                Some(&wanted),
                "{example}"
            );
            let installed = host.install_folder(&folder).await.unwrap();
            assert_eq!(host.grants(&installed.id).await, Some(wanted), "{example}");
        }
    });
    let hooks = PluginHooks::new(host.clone(), bus.clone());
    tauri::async_runtime::block_on(hooks.sync());
    assert!(hooks.attached("dev.caskade.crashy"));

    // GameMode runs games through a fake gamemoderun that records them.
    let record = temp.path().join("gamemoderun.log");
    let fake = temp.path().join("bin/gamemoderun");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\nexec \"$@\"\n",
            record.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    tauri::async_runtime::block_on(async {
        settings
            .set(
                &settings_namespace("dev.caskade.gamemode"),
                json!({"enabled": true, "command": fake.to_string_lossy()}),
                None,
            )
            .await
            .unwrap();
        settings
            .set(
                &settings_namespace("dev.caskade.howlongtobeat"),
                json!({"endpoint": format!("http://127.0.0.1:{port}/api/search")}),
                None,
            )
            .await
            .unwrap();
    });

    // The UI halves and backend calls, over typed IPC.
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            caskade_desktop::commands::plugin_ui::plugins_ui,
            caskade_desktop::commands::plugin_ui::plugin_request,
        ]);
    let app = mock_builder()
        .manage(host.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let listed = invoke(&window, "plugins_ui", json!({})).unwrap();
    let mut ui: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|plugin| plugin["id"].as_str().unwrap())
        .collect();
    ui.sort();
    assert_eq!(
        ui,
        [
            "dev.caskade.crashy",
            "dev.caskade.howlongtobeat",
            "dev.caskade.playtime-stats"
        ]
    );
    let lookup = |window: &tauri::WebviewWindow<MockRuntime>| {
        invoke(
            window,
            "plugin_request",
            json!({"pluginId": "dev.caskade.howlongtobeat", "method": "lookup", "payload": {"title": "Fixture"}}),
        )
    };
    assert_eq!(lookup(&window).unwrap()["mainHours"], 2.0);
    // Crashy's memory hog fails its own call only.
    let grow = invoke(
        &window,
        "plugin_request",
        json!({"pluginId": "dev.caskade.crashy", "method": "grow", "payload": null}),
    );
    assert!(grow.is_err(), "{grow:?}");
    assert_eq!(lookup(&window).unwrap()["completionistHours"], 5.0);

    // Launches: crashy's launch.before never returns and its game.exited
    // handler traps; GameMode still wraps every launch and playtime is kept.
    let game_dir = temp.path().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(game_dir.join("game"), "#!/bin/sh\nsleep 0.3\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            game_dir.join("game"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let game = Game::new("Fixture");
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: game_dir.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: game_dir.join("game"),
        args: vec![],
        cwd: Some(game_dir.clone()),
        env: BTreeMap::new(),
    });
    let mut stream = bus.stream();
    tauri::async_runtime::block_on(async {
        store.upsert_game(&game).await.unwrap();
        store.upsert_entry(&entry).await.unwrap();
        let launch = LaunchService::new(store.clone())
            .with_bus(bus.clone())
            .with_aliases(None);
        for _ in 0..3 {
            let session = launch
                .launch(&entry.id)
                .await
                .unwrap()
                .wait()
                .await
                .unwrap();
            assert!(session.duration_ms >= 200, "{}", session.duration_ms);
        }
        let saved = store.sessions_for_game(&game.id, 10).await.unwrap();
        assert_eq!(saved.len(), 3);
        assert!(saved.iter().all(|session| session.ended_at_ms.is_some()));
        // The circuit breaker turned crashy off and said why.
        let mut disabled = None;
        for _ in 0..200 {
            while let Ok(event) = stream.try_recv() {
                if let Event::PluginDisabled { plugin, reason } = event {
                    disabled = Some((plugin, reason));
                }
            }
            if disabled.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let (plugin, reason) = disabled.expect("crashy disabled");
        assert_eq!(plugin, "dev.caskade.crashy");
        assert!(reason.contains("3 failures"), "{reason}");
        assert!(!host.enabled("dev.caskade.crashy"));
        assert!(!hooks.attached("dev.caskade.crashy"));
        // The others are still enabled and on the bus.
        for id in [
            "dev.caskade.gamemode",
            "dev.caskade.howlongtobeat",
            "dev.caskade.playtime-stats",
        ] {
            assert!(host.enabled(id), "{id}");
        }
        assert!(hooks.attached("dev.caskade.gamemode"));
    });
    let wrapped = std::fs::read_to_string(&record).unwrap();
    assert_eq!(wrapped.lines().count(), 3, "{wrapped}");
    assert!(wrapped.lines().all(|line| line.ends_with("/game")));
    // After crashy is off, its UI half is no longer loaded either.
    let listed = invoke(&window, "plugins_ui", json!({})).unwrap();
    assert!(
        !listed
            .as_array()
            .unwrap()
            .iter()
            .any(|plugin| plugin["id"] == "dev.caskade.crashy")
    );
    assert_eq!(lookup(&window).unwrap()["title"], "Fixture");
}
