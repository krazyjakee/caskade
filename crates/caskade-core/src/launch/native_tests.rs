#![cfg(target_os = "linux")]
use super::*;
use crate::{
    db::LibraryStore,
    model::{Game, GameEntry, InstallLocation, LaunchSpec},
};
use caskade_discovery::{InstallState, SourceId, TrackingHint};
use std::{path::PathBuf, time::Duration};

/// Explicitly selected game only. The operator closes it normally; Caskade
/// never kills a real Steam game merely to make a test finish.
#[tokio::test]
#[ignore = "requires opt-in Steam account/game; src/launch/README.md"]
async fn native_steam_account_game_records_play_session() {
    if std::env::var("CASKADE_TEST_NATIVE").as_deref() != Ok("1") {
        return;
    }
    let Ok(app) = std::env::var("CASKADE_CORE_STEAM_APP_ID") else {
        eprintln!("SKIP real Steam lifecycle: CASKADE_CORE_STEAM_APP_ID is not configured");
        return;
    };
    let app_id = app.parse::<u32>().expect("positive Steam app id");
    assert!(app_id > 0);
    let path = PathBuf::from(
        std::env::var_os("CASKADE_CORE_STEAM_INSTALL_DIR")
            .expect("explicit Steam install directory"),
    );
    assert!(path.is_absolute() && path.is_dir());
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Native Steam verification");
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), app);
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path });
    entry.launch = Some(LaunchSpec::Uri {
        uri: format!("steam://rungameid/{app_id}"),
    });
    entry.tracking = Some(TrackingHint {
        steam_app_id: Some(app_id),
        ..Default::default()
    });
    store.upsert_entry(&entry).await.unwrap();
    let service = LaunchService::new(store.clone())
        .with_aliases(None)
        .with_config(LaunchConfig {
            launch_timeout: Duration::from_secs(120),
            ..Default::default()
        });
    let handle = service.launch(&entry.id).await.unwrap();
    eprintln!("Steam lifecycle started; close the selected game normally to finish this check");
    let result = tokio::time::timeout(Duration::from_secs(1200), handle.wait()).await;
    service.shutdown();
    let session = result
        .expect("operator did not close game within20minutes")
        .unwrap();
    assert!(session.ended_at_ms.is_some());
    assert!(session.duration_ms > 0);
    assert_eq!(
        store.sessions_for_game(&game.id, 10).await.unwrap().len(),
        1
    );
    eprintln!(
        "Steam lifecycle recorded: sessions=1 duration_ms={}",
        session.duration_ms
    );
}

#[tokio::test]
#[ignore = "requires local Wine; private prefix, no user account/state"]
async fn native_wine_fixture_records_session_with_runtime_only_runner_path() {
    if std::env::var("CASKADE_TEST_NATIVE").as_deref() != Ok("1") {
        return;
    }
    let wine = PathBuf::from(
        std::env::var_os("CASKADE_CORE_WINE").unwrap_or_else(|| "/usr/bin/wine".into()),
    );
    assert!(wine.is_file(), "configured Wine executable is missing");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("storage");
    let game_dir = root.join("Wine Café Game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-windows-x86_64.exe"
        ),
        game_dir.join("game.exe"),
    )
    .unwrap();
    caskade_installer::manifest::RootManifest::new("native-wine-root")
        .unwrap()
        .write(&root)
        .unwrap();
    let _cleanup = PrivateWineCleanup {
        prefix: game_dir.join(".caskade/prefix"),
        server: wine.parent().unwrap().join("wineserver"),
    };
    let store = LibraryStore::in_memory().await.unwrap();
    let game = Game::new("Native Wine fixture");
    store.upsert_game(&game).await.unwrap();
    store
        .upsert_root(&crate::model::StorageRoot {
            id: "native-wine-root".into(),
            path: root,
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "wine");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: "native-wine-root".into(),
        rel_dir: "Wine Café Game".into(),
    });
    entry.runtime = caskade_discovery::GameRuntime::Windows {
        prefix: Some(caskade_discovery::Prefix {
            path: ".caskade/prefix".into(),
            arch: None,
            owner: None,
        }),
        runner: Some(caskade_discovery::RunnerHint {
            kind: caskade_discovery::RunnerKind::Wine,
            name: None,
            path: None,
        }),
    };
    entry.launch = Some(LaunchSpec::Executable {
        path: "game.exe".into(),
        args: vec!["--token".into(), r"G:\Game\token".into()],
        cwd: Some(".".into()),
        env: Default::default(),
    });
    store.upsert_entry(&entry).await.unwrap();
    let service = LaunchService::new(store.clone())
        .with_aliases(None)
        .with_runner_paths(RunnerPaths {
            wine: wine.clone(),
            ..Default::default()
        })
        .with_config(LaunchConfig {
            launch_timeout: Duration::from_secs(30),
            exit_grace: Duration::from_millis(100),
            ..Default::default()
        });
    let session = service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(game_dir.join("token").is_file());
    assert!(session.ended_at_ms.is_some());
    assert_eq!(
        store.sessions_for_game(&game.id, 10).await.unwrap().len(),
        1
    );
    assert!(store.get_entry(&entry.id).await.unwrap().unwrap().runtime == entry.runtime);
    assert_eq!(
        std::fs::read_dir(game_dir.join(".caskade/prefix/dosdevices/z:"))
            .unwrap()
            .count(),
        0
    );
    eprintln!(
        "Private Wine fixture: sessions=1 duration_ms={}",
        session.duration_ms
    );
}

struct PrivateWineCleanup {
    prefix: PathBuf,
    server: PathBuf,
}
impl Drop for PrivateWineCleanup {
    fn drop(&mut self) {
        for arg in ["-k", "-w"] {
            let _ = std::process::Command::new(&self.server)
                .arg(arg)
                .env("WINEPREFIX", &self.prefix)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}
