//! A launch from an AppImage build (#274): the game gets the environment
//! Caskade was started with, minus the AppImage runtime's additions; the
//! user's own entries and what the launch sets itself stay, and outside an
//! AppImage nothing changes. The environment is simulated by running this
//! test binary again with it (the workspace forbids `unsafe`, so the test
//! can't change its own environment).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use caskade_core::db::LibraryStore;
use caskade_core::launch::{LaunchConfig, LaunchService};
use caskade_core::model::{Game, GameEntry, InstallState, LaunchSpec, SourceId};

const OUT: &str = "CASKADE_TEST_APPIMAGE_OUT";
const MOUNT: &str = "/tmp/.mount_CaskadTEST";

/// Runs `launched_from_this_environment` in a fresh process with `vars` on
/// top of this one's (less any AppImage runtime this one has), and returns
/// the environment its game saw.
fn launch_in(vars: &[(&str, String)], dir: &Path, name: &str) -> BTreeMap<String, String> {
    let out = dir.join(name);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "appimage_env::launched_from_this_environment",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        // Whatever AppImage runs these tests, the child sees only `vars`'.
        .env_remove("APPDIR")
        .env_remove("APPIMAGE")
        .env_remove("ARGV0")
        .env_remove("OWD")
        .envs(vars.iter().map(|(key, value)| (*key, value.as_str())))
        .env(OUT, &out)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

#[test]
fn a_launch_from_an_appimage_gets_the_environment_caskade_was_started_with() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::env::var("PATH").unwrap();
    let inside = launch_in(
        &[
            ("APPDIR", MOUNT.to_owned()),
            ("APPIMAGE", "/home/player/Caskade.AppImage".to_owned()),
            ("ARGV0", "Caskade.AppImage".to_owned()),
            ("OWD", "/home/player".to_owned()),
            ("LD_LIBRARY_PATH", format!("{MOUNT}/usr/lib:/opt/mine/lib")),
            (
                "GDK_PIXBUF_MODULE_FILE",
                format!("{MOUNT}/usr/lib/loaders.cache"),
            ),
            ("XDG_DATA_DIRS", format!("{MOUNT}/usr/share:/usr/share")),
            ("PATH", format!("{MOUNT}/usr/bin:{path}")),
            ("GTK_PATH", format!("{MOUNT}/usr/lib/gtk-3.0")),
            ("MY_SETTING", "kept".to_owned()),
        ],
        dir.path(),
        "inside",
    );
    for gone in [
        "APPDIR",
        "APPIMAGE",
        "ARGV0",
        "OWD",
        "GDK_PIXBUF_MODULE_FILE",
    ] {
        assert!(!inside.contains_key(gone), "{gone} reached the game");
    }
    // The user's own entries stay.
    assert_eq!(inside["LD_LIBRARY_PATH"], "/opt/mine/lib");
    assert_eq!(inside["XDG_DATA_DIRS"], "/usr/share");
    assert_eq!(inside["PATH"], path);
    assert_eq!(inside["MY_SETTING"], "kept");
    // What the launch sets itself wins.
    assert_eq!(inside["GTK_PATH"], "/game/gtk");
    assert!(
        inside.values().all(|value| !value.contains(MOUNT)),
        "{inside:?}"
    );

    // Not an AppImage: what Caskade had is what the game gets.
    let outside = launch_in(
        &[
            ("LD_LIBRARY_PATH", "/opt/other/lib:/opt/mine/lib".to_owned()),
            ("OWD", "/home/player".to_owned()),
            ("MY_SETTING", "kept".to_owned()),
        ],
        dir.path(),
        "outside",
    );
    assert_eq!(outside["LD_LIBRARY_PATH"], "/opt/other/lib:/opt/mine/lib");
    assert_eq!(outside["OWD"], "/home/player");
    assert_eq!(outside["PATH"], path);
    assert_eq!(outside["GTK_PATH"], "/game/gtk");
}

/// The child half: launches `sh`, which writes the environment it got.
#[tokio::test]
#[ignore = "run by a_launch_from_an_appimage_gets_the_environment_caskade_was_started_with"]
async fn launched_from_this_environment() {
    let Some(out) = std::env::var_os(OUT) else {
        return;
    };
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Env Fixture")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), "env");
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Executable {
        path: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "env > \"$0\"".into(),
            out.to_string_lossy().into_owned(),
        ],
        cwd: None,
        env: BTreeMap::from([("GTK_PATH".to_owned(), "/game/gtk".to_owned())]),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    let launches = LaunchService::new(store).with_config(LaunchConfig {
        poll_interval: Duration::from_millis(20),
        launch_timeout: Duration::from_secs(10),
        exit_grace: Duration::from_millis(100),
        heartbeat_interval: Duration::from_millis(200),
    });
    launches
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
}
