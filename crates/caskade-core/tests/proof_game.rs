//! `fixtures/saves/proof_game.rs`, a real Windows program with saves, and
//! the Ludusavi manifest that says where it keeps them, for the tests that
//! run it: under Proton (`proton_real`) and on Windows (`windows_saves`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use caskade_core::bus::Bus;
use caskade_core::install::InstallerRunnerResolver;
use caskade_core::launch::{LaunchConfig, LaunchService, SessionStatus};
use caskade_core::ludusavi::Ludusavi;
use caskade_core::model::{
    Game, GameEntry, GameRuntime, InstallLocation, InstallState, LaunchSpec, PlaySession,
};
use caskade_core::saves::Saves;
use caskade_core::saves::versions::{SaveBlobs, SaveVersion, SaveVersions, SyncRefresh};
use caskade_core::sync::SyncService;
use caskade_discovery::HostEnv;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::cores::{Core, within};

pub const TITLE: &str = "Caskade Proof";

/// The game's saves, one entry per kind of Windows folder a real game uses.
pub async fn manifest(folder: &str) -> MockServer {
    let server = MockServer::start().await;
    let manifest = format!(
        "{TITLE}:
  files:
    <winAppData>/{folder}:
      tags: [save]
      when: [{{os: windows}}]
    <winLocalAppData>/{folder}/*.ini:
      tags: [config]
      when: [{{os: windows}}]
    <winDocuments>/{folder}/*.sav:
      tags: [save]
      when: [{{os: windows}}]
    <home>/Saved Games/{folder}:
      tags: [save]
      when: [{{os: windows}}]
"
    );
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(manifest))
        .mount(&server)
        .await;
    server
}

/// The program, as `CASKADE_PROOF_GAME` names it, or built into `dir`.
pub fn program(dir: &Path) -> PathBuf {
    if let Some(built) = std::env::var_os("CASKADE_PROOF_GAME") {
        return built.into();
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/saves/proof_game.rs");
    let program = dir.join("proof_game.exe");
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args(["--edition", "2021", "-O", "-o"]);
    rustc.arg(&program).arg(source);
    if !cfg!(windows) {
        rustc.args(["--target", "x86_64-pc-windows-gnu"]);
    }
    let status = rustc.status().expect("rustc runs");
    assert!(status.success(), "the proof game builds");
    program
}

pub struct Device {
    pub core: Core,
    pub sync: SyncService,
    pub versions: SaveVersions,
    pub launches: LaunchService,
}

/// Saves, sync and launches over `core`, wired as the app wires them.
pub async fn device(core: Core, env: HostEnv, manifest: &MockServer) -> Device {
    let ludusavi = Ludusavi::with_url(core.dir.path().join("ludusavi"), manifest.uri());
    ludusavi.refresh(true).await.unwrap();
    let saves = Saves::with_environment(core.store.clone(), ludusavi, env);
    let versions = SaveVersions::new(saves, core.dir.path().join("save-blobs"));
    versions.set_settings(core.settings.clone());
    // The same household account on both (accounts sync, #228).
    versions.set_account(|| Some("player".into()));
    let sync = SyncService::new(core.store.clone(), core.devices.clone());
    sync.set_settings(core.settings.clone());
    sync.register_blobs(Arc::new(SaveBlobs(versions.clone())));
    let engine = sync.clone();
    versions.set_device(move || engine.engine().map(|engine| engine.device().to_owned()));
    versions.set_remote(SyncRefresh::new(sync.clone(), core.devices.clone()));
    versions.set_wait(Duration::from_secs(20));
    let bus = Bus::default();
    versions.attach(&bus);
    let launches = LaunchService::new(core.store.clone())
        .with_bus(bus)
        .with_runner_resolver(Arc::new(InstallerRunnerResolver(core.settings.clone())))
        .with_config(LaunchConfig {
            launch_timeout: Duration::from_secs(300),
            ..Default::default()
        });
    Device {
        core,
        sync,
        versions,
        launches,
    }
}

/// The game added by hand, in a folder of its own.
pub async fn add_game(device: &Device, saves: &str) -> (String, PathBuf) {
    let core = &device.core;
    let folder = core.dir.path().join("My Games").join(TITLE);
    std::fs::create_dir_all(folder.join("Content")).unwrap();
    let scratch = core.dir.path().join("program");
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::copy(program(&scratch), folder.join("Caskade Proof.exe")).unwrap();
    let pack: Vec<u8> = (0..6 * 1024 * 1024).map(|i| (i % 253) as u8).collect();
    std::fs::write(folder.join("Content/level1.pak"), pack).unwrap();
    let game = core.store.upsert_game(&Game::new(TITLE)).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "proof");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: folder.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: folder.join("Caskade Proof.exe"),
        args: Vec::new(),
        cwd: None,
        env: [("CASKADE_PROOF_FOLDER".to_owned(), saves.to_owned())].into(),
    });
    entry.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    let entry = core.store.upsert_entry(&entry).await.unwrap();
    (entry.id, folder)
}

/// Play on `device`: the launch pipeline with Proton, to the game's exit.
pub async fn play(device: &Device, entry: &str) -> PlaySession {
    let handle = device.launches.launch(entry).await.unwrap();
    let (session, status) = tokio::time::timeout(Duration::from_secs(300), handle.wait_status())
        .await
        .expect("the game exits within 5 minutes")
        .unwrap();
    assert_eq!(status, SessionStatus::Exited, "{session:?}");
    session
}

/// The version made after `session`, by the `launch.after` interceptor.
pub async fn version_after(device: &Device, game: &str, session: &PlaySession) -> SaveVersion {
    within("a version after the session", 30, || async {
        device
            .versions
            .versions(game)
            .await
            .unwrap()
            .iter()
            .any(|version| version.session_id.as_deref() == Some(&session.id))
    })
    .await;
    device
        .versions
        .versions(game)
        .await
        .unwrap()
        .into_iter()
        .find(|version| version.session_id.as_deref() == Some(&session.id))
        .unwrap()
}

/// The last run's lines of the game's log.
pub fn last_run(folder: &Path) -> String {
    let log = read(folder.join("ran.log"));
    let start = log.rfind("run wine=").unwrap_or(0);
    log[start..].to_owned()
}

pub fn write(path: impl AsRef<Path>, text: &str) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn read(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}
