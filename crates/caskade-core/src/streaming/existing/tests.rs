//! Finding a user's own Sunshine where each install puts it.

use super::*;

fn touch(path: &std::path::Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "").unwrap();
}

#[test]
fn the_msi_upgrade_code_is_packed_as_windows_files_it() {
    assert_eq!(
        packed_guid(UPGRADE_CODE).as_deref(),
        Some("B1D3A21561EBB1040A1D95BB3A49F28B")
    );
    assert_eq!(packed_guid("not a guid"), None);
}

#[test]
fn each_install_is_found_with_its_certificate() {
    let temp = tempfile::tempdir().unwrap();
    let (root, home) = (temp.path().join("root"), temp.path().join("home"));
    let never = |_: &str, _: &[&str]| false;
    let probe = |os, runs: &dyn Fn(&str, &[&str]) -> bool| {
        detect(&Probe {
            os,
            root: root.clone(),
            home: home.clone(),
            runs,
        })
    };
    // Nothing installed: nothing found, on any system.
    for os in [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::Macos,
    ] {
        assert_eq!(probe(os, &never), None);
    }

    // Linux: a package, then the Flatpak (whose certificate is in its data).
    touch(&root.join("usr/bin/sunshine"));
    let found = probe(OperatingSystem::Linux, &never).unwrap();
    assert_eq!(found.found, [Found::Package]);
    assert_eq!(found.cert, None, "no certificate until it first ran");
    touch(&home.join(".config/sunshine/credentials/cacert.pem"));
    let found = probe(OperatingSystem::Linux, &never).unwrap();
    assert_eq!(
        found.cert,
        Some(home.join(".config/sunshine/credentials/cacert.pem"))
    );
    std::fs::create_dir_all(home.join(".local/share/flatpak/app/dev.lizardbyte.app.Sunshine"))
        .unwrap();
    touch(
        &home.join(".var/app/dev.lizardbyte.app.Sunshine/config/sunshine/credentials/cacert.pem"),
    );
    touch(&home.join(".config/systemd/user/app-dev.lizardbyte.app.Sunshine.service"));
    let found = probe(OperatingSystem::Linux, &never).unwrap();
    assert_eq!(
        found.found,
        [Found::Package, Found::Flatpak, Found::UserUnit]
    );
    assert_eq!(
        found.cert,
        Some(
            home.join(
                ".var/app/dev.lizardbyte.app.Sunshine/config/sunshine/credentials/cacert.pem"
            )
        )
    );

    // Windows: the MSI's upgrade code and the service, asked of Windows.
    let asked = std::sync::Mutex::new(Vec::new());
    let windows = |program: &str, args: &[&str]| {
        asked
            .lock()
            .unwrap()
            .push(format!("{program} {}", args.join(" ")));
        true
    };
    let found = probe(OperatingSystem::Windows, &windows).unwrap();
    assert_eq!(found.found, [Found::Msi, Found::Service]);
    let asked = asked.lock().unwrap();
    assert!(
        asked[0].ends_with(r"UpgradeCodes\B1D3A21561EBB1040A1D95BB3A49F28B"),
        "{asked:?}"
    );
    assert_eq!(asked[1], "sc.exe query SunshineService");

    // macOS: the app, or Homebrew's.
    std::fs::create_dir_all(root.join("Applications/Sunshine.app")).unwrap();
    touch(&root.join("opt/homebrew/bin/sunshine"));
    let found = probe(OperatingSystem::Macos, &never).unwrap();
    assert_eq!(found.found, [Found::App, Found::Homebrew]);
}
