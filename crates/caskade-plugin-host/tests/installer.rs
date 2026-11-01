//! #55 acceptance: a WASM plugin teaches Caskade a new format and a new URL
//! host without filesystem write access. The example plugin
//! (`plugins/examples/caskade-pack`) resolves a game page to its `.cpk`
//! download and plans the unpacking; the installer downloads and writes.
#![cfg(target_os = "linux")]

use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    install::{InstallOptions, InstallService, JobSnapshot, JobStatus},
    model::StorageRoot,
};
use caskade_installer::{
    ArtifactId, ArtifactNode, Format, PayloadPlatform, Role, manifest::RootManifest,
};
use caskade_plugin_host::{HostError, HostOptions, Limits, PluginHost, PluginServices};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const ID: &str = "dev.caskade.example.pack";

fn example() -> PathBuf {
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/examples/caskade-pack");
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("example-pack");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            "wasm32-wasip2",
        ])
        .arg("--manifest-path")
        .arg(plugin.join("backend/Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("run cargo for the example plugin");
    assert!(status.success(), "building the example plugin failed");
    target.join("wasm32-wasip2/release/caskade_pack_plugin.wasm")
}

/// A `.cpk`: `CPAK`, then (name length u16 LE, name, flags u8, size u64 LE,
/// bytes) per entry.
fn pack(entries: &[(&str, bool, &[u8])]) -> Vec<u8> {
    let mut pack = b"CPAK".to_vec();
    for (name, executable, bytes) in entries {
        pack.extend((name.len() as u16).to_le_bytes());
        pack.extend(name.as_bytes());
        pack.push(u8::from(*executable));
        pack.extend((bytes.len() as u64).to_le_bytes());
        pack.extend(*bytes);
    }
    pack
}

/// Serves a game page that links its pack, and the pack.
fn serve(pack: Vec<u8>) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let (kind, body) = if request.starts_with("GET /games/42 ") {
                (
                    "text/html",
                    br#"<html><body><h1>Pack Runner</h1>
                    <a rel="caskade-download" href="/files/pack-runner.cpk">Download</a>
                    </body></html>"#
                        .to_vec(),
                )
            } else if request.starts_with("GET /files/pack-runner.cpk ") {
                ("application/octet-stream", pack.clone())
            } else {
                ("text/plain", b"not found".to_vec())
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    port
}

async fn finished(service: &InstallService, id: &str) -> JobSnapshot {
    // Up to two minutes: the job compiles the plugin's backend in the
    // resource queue every test in this binary shares, which can take long.
    for _ in 0..2400 {
        let job = service.job(id).unwrap();
        if job.status.is_finished() || job.status == JobStatus::NeedsUser {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} did not finish: {:?}", service.job(id));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_teaches_a_new_format_and_a_new_url_host_without_write_access() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let game = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
    ))
    .unwrap();
    let port = serve(pack(&[
        ("Pack Runner.x86_64", true, &game),
        ("data/level1.dat", false, b"level one"),
    ]));

    // The example plugin, granted the test server's host and port and
    // nothing else.
    let folder = temp.path().join("plugins/caskade-pack");
    std::fs::create_dir_all(folder.join("dist")).unwrap();
    std::fs::copy(example(), folder.join("dist/backend.wasm")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../plugins/examples/caskade-pack/plugin.json"),
        )
        .unwrap(),
    )
    .unwrap();
    manifest["permissions"] = serde_json::json!({"network": [format!("127.0.0.1:{port}")]});
    // Only the backend is built here; the UI half has its own test.
    manifest.as_object_mut().unwrap().remove("ui");
    std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
    // Its translations (#219), as installed.
    std::fs::create_dir_all(folder.join("locales")).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/examples/caskade-pack/locales/sv.po"),
        folder.join("locales/sv.po"),
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
    let grants = host.grants(ID).await.unwrap();
    assert!(grants.fs.is_empty() && !grants.process);

    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = RootManifest::new("root-pack").unwrap();
    identity.write(&root_path).unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();

    let mut events = bus.stream();
    let service = InstallService::new(
        store.clone(),
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let queued = service
        .install(format!("http://127.0.0.1:{port}/games/42"), None)
        .await
        .unwrap();
    let job = finished(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    assert_eq!(
        job.candidates[0].handler.0,
        format!("plugin:{ID}"),
        "the plugin's plan ran"
    );

    // The installer wrote what the plugin described.
    let dir = job.target_dir.clone().unwrap();
    let exe = dir.join("Pack Runner.x86_64");
    assert_eq!(std::fs::read(&exe).unwrap(), game);
    assert_ne!(
        std::fs::metadata(&exe).unwrap().permissions().mode() & 0o111,
        0
    );
    assert_eq!(
        std::fs::read(dir.join("data/level1.dat")).unwrap(),
        b"level one"
    );

    // The game is in the library and launches the pack's program.
    let game_id = job.game_id.clone().unwrap();
    let entries = store.entries_for_game(&game_id).await.unwrap();
    let launch = format!("{:?}", entries[0].launch);
    assert!(launch.contains("Pack Runner.x86_64"), "{launch}");

    let mut names = Vec::new();
    while let Ok(event) = events.try_recv() {
        names.push(event.name());
    }
    for name in ["install.queued", "install.downloaded", "install.completed"] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }

    // #219: in Swedish, its name, settings and the install handler's
    // messages show in Swedish; the backend asks the host for the language.
    host.pin_locale(Some("sv"));
    let summary = host
        .list()
        .await
        .into_iter()
        .find(|plugin| plugin.id == ID)
        .unwrap();
    assert_eq!(summary.name, "Caskade-paket (exempel)");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    let settings = summary.contributes.unwrap().settings.unwrap();
    assert_eq!(
        settings["properties"]["preferPacks"]["title"],
        "Föredra paket"
    );
    let empty = temp.path().join("empty.cpk");
    std::fs::write(&empty, b"CPAK").unwrap();
    let input = caskade_core::bus::InstallInput {
        path: empty,
        artifact: ArtifactNode {
            id: ArtifactId("input".into()),
            path: "empty.cpk".into(),
            format: Format::Unknown,
            platform: PayloadPlatform::Unknown,
            role: Role::Unknown,
            size: Some(4),
            facts: Default::default(),
            children: vec![],
            children_complete: true,
        },
    };
    match host.install_plan(ID, &input).await {
        Err(HostError::Plugin { message, .. }) => assert_eq!(message, "paketet är tomt"),
        other => panic!("expected the pack's error, got {other:?}"),
    }
    host.pin_locale(Some("en"));
    match host.install_plan(ID, &input).await {
        Err(HostError::Plugin { message, .. }) => assert_eq!(message, "the pack is empty"),
        other => panic!("expected the pack's error, got {other:?}"),
    }
}
