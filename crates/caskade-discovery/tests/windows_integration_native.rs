//! Trusted Windows discovery-live job: write only our unique HKCU keys.
#![cfg(all(windows, feature = "windows-sources"))]
use caskade_discovery::{sources::uninstall::UninstallSource, *};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
use winreg::{RegKey, enums::*};
struct Cleanup(Vec<String>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let parent = RegKey::predef(HKEY_CURRENT_USER);
        for path in &self.0 {
            for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
                if let Ok(key) = parent.open_subkey_with_flags(path, KEY_READ | KEY_WRITE | view) {
                    let _ = key.delete_subkey_all("");
                }
            }
            let _ = parent.delete_subkey_all(path);
        }
    }
}
#[test]
#[ignore = "writes isolated HKCU test keys; trusted Windows discovery-live job"]
fn live_registry_fixture_games_are_found_once_and_test_keys_are_removed() {
    assert!(std::env::var_os("CASKADE_DISCOVERY_LIVE").is_some());
    assert!(std::env::var_os("CASKADE_TEST_NATIVE").is_some());
    let id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp = crate::fixtures::tempdir();
    let mut cleanup = Cleanup(Vec::new());
    let mut titles = Vec::new();
    for (index, view) in [KEY_WOW64_32KEY, KEY_WOW64_64KEY].into_iter().enumerate() {
        let title = format!("Caskade D2 Fixture {id} {index}");
        let path =
            format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CaskadeD2-{id}-{index}");
        cleanup.0.push(path.clone());
        let game = temp.path().join(index.to_string());
        fs::create_dir_all(game.join("Game_Data")).unwrap();
        for name in [
            "game.exe",
            "UnityPlayer.dll",
            "Game_Data/globalgamemanagers",
        ] {
            fs::write(
                game.join(name),
                b"original inert discovery marker; never executed",
            )
            .unwrap();
        }
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags(&path, KEY_READ | KEY_WRITE | view)
            .expect("create owned fixture key");
        for (name, value) in [
            ("DisplayName", title.clone()),
            ("Publisher", "Caskade Fixture Studio".into()),
            ("InstallLocation", game.to_string_lossy().into_owned()),
            (
                "DisplayIcon",
                game.join("game.exe").to_string_lossy().into_owned(),
            ),
            (
                "UninstallString",
                format!("\"{}\"", game.join("unins000.exe").display()),
            ),
        ] {
            key.set_value(name, &value)
                .expect("write owned fixture value");
        }
        titles.push(title);
    }
    // Do not print records from the live registry: only our aggregate result.
    let games = UninstallSource::new()
        .scan(&HostEnv::live(), &ScanCtx::default())
        .expect("scan live uninstall keys");
    for title in &titles {
        assert_eq!(
            games.iter().filter(|g| g.title == *title).count(),
            1,
            "owned fixture game must appear once"
        );
    }
    let keys = cleanup.0.clone();
    drop(cleanup);
    for path in keys {
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            assert!(
                RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey_with_flags(&path, KEY_READ | view)
                    .is_err(),
                "owned fixture key must be removed"
            );
        }
    }
    println!(
        "{}",
        serde_json::json!({"metric":"d2_live_registry","fixture_games":2,"found_once":2,"owned_keys_removed":2})
    );
}
