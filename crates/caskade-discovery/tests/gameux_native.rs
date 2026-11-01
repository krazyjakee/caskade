//! Writes only one unique HKCU GameUX fixture key; opt-in discovery-live only.
#![cfg(all(windows, feature = "windows-sources"))]
use caskade_discovery::{
    sources::gameux::{GameUx, REGISTRY_PATH},
    *,
};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
use winreg::{RegKey, enums::*};
#[test]
#[ignore = "real Windows HKCU GameUX parity; discovery-live profile"]
fn live_gameux_reads_its_owned_temporary_registration() {
    assert!(
        std::env::var_os("CASKADE_TEST_NATIVE").is_some(),
        "set CASKADE_TEST_NATIVE=1"
    );
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let guid = format!(
        "{{{:08x}-4242-4242-4242-{:012x}}}",
        std::process::id(),
        time & 0xffffffffffff
    );
    let key_path = format!(r"{REGISTRY_PATH}\Games\{guid}");
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
        }
    }
    let _cleanup = Cleanup(key_path.clone());
    let root = tempfile::tempdir().unwrap();
    let exe = root.path().join("Fixture2009.exe");
    let gdf = root.path().join("FixtureGdf.dll");
    fs::write(&exe, b"inert fixture: never executed").unwrap();
    fs::write(&gdf, b"inert GDF fixture: never loaded").unwrap();
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(&key_path, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .unwrap();
    for (name, value) in [
        ("Title", "Original Retail-Era Fixture 2009".to_owned()),
        (
            "ConfigApplicationPath",
            root.path().to_str().unwrap().to_owned(),
        ),
        ("AppExePath", exe.to_str().unwrap().to_owned()),
        ("ConfigGDFBinaryPath", gdf.to_str().unwrap().to_owned()),
    ] {
        key.set_value(name, &value).unwrap();
    }
    drop(key);
    let env = HostEnv::live();
    let ctx = ScanCtx::new(Cancel::new());
    let games = GameUx.scan(&env, &ctx).unwrap();
    let owned: Vec<_> = games
        .iter()
        .filter(|game| game.key.contains(&guid.to_lowercase()))
        .collect();
    assert!(!owned.is_empty());
    for game in owned {
        assert_eq!(game.title, "Original Retail-Era Fixture 2009");
        assert_eq!(game.state, InstallState::Installed);
        assert_eq!(game.confidence, Confidence::Certain);
        assert!(matches!(&game.launch[0],LaunchTarget::Exec{program,..} if program==&exe));
        assert!(
            game.evidence
                .contains(&Evidence::File { path: gdf.clone() })
        );
    }
}
