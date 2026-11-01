//! Binary CLI routing with private command state; no real Flatpak user data.
#![cfg(all(
    feature = "cli",
    any(
        all(feature = "linux-formats", target_os = "linux"),
        all(feature = "macos-formats", not(target_os = "macos"))
    )
))]

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
use caskade_installer::manifest::{InstallManifest, RelocationVerdict};
use caskade_installer::testing::TestRoot;
use serde_json::Value;
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
use std::path::PathBuf;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

#[cfg(all(feature = "macos-formats", not(target_os = "macos")))]
fn binary(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_caskade-install"))
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_storage_clean(root: &TestRoot) {
    // A storage root retains only its identity; remove that known metadata
    // after validating it, then use TestRoot's strict sentinel/empty assertion.
    let metadata = root.path().join(".caskade");
    if metadata.exists() {
        fn inspect(path: &Path, base: &Path) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                assert!(!kind.is_symlink());
                if kind.is_dir() {
                    inspect(&entry.path(), base);
                } else {
                    assert_eq!(entry.path(), base.join("root.json"));
                }
            }
        }
        inspect(&metadata, &metadata);
        caskade_installer::manifest::RootManifest::read(root.path()).unwrap();
        fs::remove_dir_all(metadata).unwrap();
    }
    root.assert_clean();
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
struct FlatpakCommand {
    _home: tempfile::TempDir,
    bin: PathBuf,
    state: PathBuf,
    log: PathBuf,
}
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
impl FlatpakCommand {
    fn new(fail: bool) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        let state = home.path().join("private-flatpak");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&state).unwrap();
        let log = home.path().join("argv.log");
        let script = bin.join("flatpak");
        fs::write(
            &script,
            format!(
                r##"#!/bin/sh
set -eu
for arg in "$@"; do printf '%s\n' "$arg" >> "$CASKADE_FLATPAK_LOG"; done
printf 'END\n' >> "$CASKADE_FLATPAK_LOG"
case "$1" in
install)
    if [ "{fail}" = true ]; then exit 17; fi
    : > "$FLATPAK_USER_DIR/installed"
    ;;
uninstall) rm -f "$FLATPAK_USER_DIR/installed" ;;
info) [ -e "$FLATPAK_USER_DIR/installed" ] ;;
*) exit 99 ;;
esac
"##
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _home: home,
            bin,
            state,
            log,
        }
    }
    fn run(&self, args: &[&str]) -> Output {
        let path = std::env::join_paths(std::iter::once(self.bin.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .arg("--json")
            .args(args)
            .env("PATH", path)
            .env("FLATPAK_USER_DIR", &self.state)
            .env("CASKADE_FLATPAK_LOG", &self.log)
            .output()
            .unwrap()
    }
    fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .split("END\n")
            .filter(|part| !part.is_empty())
            .map(|part| part.lines().map(str::to_owned).collect())
            .collect()
    }
}
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
fn bundle() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/flatpak/flatpak-bundle.flatpak")
}
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
fn installed_dir(root: &Path) -> PathBuf {
    let dirs: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_name() != ".caskade" && entry.file_type().unwrap().is_dir())
        .collect();
    assert_eq!(dirs.len(), 1);
    dirs[0].path()
}
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
fn assert_flatpak(root: &Path, outcome: &Value) -> PathBuf {
    assert_eq!(outcome["type"], "hand_off");
    assert_eq!(outcome["value"]["type"], "delegated");
    assert_eq!(
        outcome["value"]["value"]["launch_uri"],
        "flatpak:io.caskade.FixtureGame"
    );
    let game = installed_dir(root);
    let manifest = InstallManifest::read(&game).unwrap();
    assert_eq!(manifest.record().plan.candidate.handler.0, "flatpak");
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Pinned
    );
    assert!(manifest.record().files.is_empty());
    assert!(manifest.record().launch_targets.is_empty());
    assert_eq!(manifest.record().uninstall_steps.len(), 1);
    assert!(manifest.verify(&game).unwrap().changed.is_empty());
    game
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
#[test]
fn flatpak_cli_delegates_records_pinned_metadata_and_runs_recorded_uninstall() {
    let fake = FlatpakCommand::new(false);
    let root = TestRoot::new().unwrap();
    let input = bundle();
    let output = fake.run(&[
        "install",
        input.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    let outcome = value(&output);
    assert!(
        output.status.success(),
        "{outcome} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let game = assert_flatpak(root.path(), &outcome);
    assert!(fake.state.join("installed").exists());
    assert_eq!(
        fake.calls(),
        vec![
            vec![
                "install",
                "--user",
                "--noninteractive",
                "--",
                input.to_str().unwrap()
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        ]
    );
    let output = fake.run(&["uninstall", game.to_str().unwrap()]);
    assert!(output.status.success(), "{}", value(&output));
    assert_eq!(
        fake.calls()[1],
        [
            "info",
            "--user",
            "--",
            "app/io.caskade.FixtureGame/x86_64/1"
        ]
    );
    assert_eq!(
        fake.calls()[2],
        [
            "uninstall",
            "--user",
            "--noninteractive",
            "--",
            "app/io.caskade.FixtureGame/x86_64/1"
        ]
    );
    assert!(!fake.state.join("installed").exists());
    assert_storage_clean(&root);
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
#[test]
fn flatpak_extract_policy_and_delegate_failure_never_commit_a_game() {
    for fail in [false, true] {
        let fake = FlatpakCommand::new(fail);
        let root = TestRoot::new().unwrap();
        let input = bundle();
        let mut args = vec![
            "install",
            input.to_str().unwrap(),
            "--root",
            root.path().to_str().unwrap(),
        ];
        if !fail {
            args.push("--extract");
        }
        let output = fake.run(&args);
        let report = value(&output);
        if fail {
            assert_eq!(output.status.code(), Some(1), "{report}");
        } else {
            assert_eq!(report["type"], "unsupported");
            assert!(fake.calls().is_empty());
        }
        assert!(!fake.state.join("installed").exists());
        assert_storage_clean(&root);
    }
}

/// `apt`, `dpkg-query` and `pkexec` on a private PATH; apt's installs are a
/// state file, `dpkg-query --status` finds it and `dpkg-query -L` lists
/// `/usr/bin/env` as the package's file.
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
struct AptCommand {
    _home: tempfile::TempDir,
    bin: PathBuf,
    state: PathBuf,
    log: PathBuf,
}
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
impl AptCommand {
    fn new(fail: bool) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let state = home.path().join("installed");
        let log = home.path().join("argv.log");
        let scripts = [
            ("pkexec", "#!/bin/sh\nexec \"$@\"\n".to_owned()),
            (
                "apt",
                format!(
                    r##"#!/bin/sh
set -eu
for arg in "$@"; do printf '%s\n' "$arg" >> "$CASKADE_APT_LOG"; done
printf 'END\n' >> "$CASKADE_APT_LOG"
case "$1" in
install)
    if [ "{fail}" = true ]; then exit 100; fi
    : > "$CASKADE_APT_STATE"
    ;;
remove) rm -f "$CASKADE_APT_STATE" ;;
*) exit 99 ;;
esac
"##
                ),
            ),
            (
                "dpkg-query",
                r##"#!/bin/sh
[ -e "$CASKADE_APT_STATE" ] || exit 1
case "$1" in
--status) ;;
-L) printf '/usr/share/doc/caskade-deb-absolute-path\n/usr/bin/env\n' ;;
*) exit 99 ;;
esac
"##
                .to_owned(),
            ),
        ];
        for (name, script) in scripts {
            let path = bin.join(name);
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self {
            _home: home,
            bin,
            state,
            log,
        }
    }
    fn run(&self, args: &[&str]) -> Output {
        let path = std::env::join_paths(std::iter::once(self.bin.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .arg("--json")
            .args(args)
            .env("PATH", path)
            .env("CASKADE_APT_STATE", &self.state)
            .env("CASKADE_APT_LOG", &self.log)
            .output()
            .unwrap()
    }
    fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .split("END\n")
            .filter(|part| !part.is_empty())
            .map(|part| part.lines().map(str::to_owned).collect())
            .collect()
    }
}
#[cfg(all(feature = "linux-formats", target_os = "linux"))]
fn system_deb() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/packages/deb-absolute-path.deb")
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
#[test]
fn apt_installed_deb_hands_off_its_command_and_uninstalls_through_apt() {
    let fake = AptCommand::new(false);
    let root = TestRoot::new().unwrap();
    let input = system_deb();
    let output = fake.run(&[
        "install",
        input.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    let outcome = value(&output);
    assert!(
        output.status.success(),
        "{outcome} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(outcome["type"], "hand_off");
    assert_eq!(outcome["value"]["type"], "package");
    let package = &outcome["value"]["value"];
    assert_eq!(package["package"], "caskade-deb-absolute-path");
    assert_eq!(
        package["title"],
        caskade_installer::discover::clean_title("caskade-deb-absolute-path")
    );
    assert_eq!(package["program"], "/usr/bin/env");
    assert_eq!(package["uninstall_steps"].as_array().unwrap().len(), 1);
    assert!(fake.state.exists());
    let game = installed_dir(root.path());
    let manifest = InstallManifest::read(&game).unwrap();
    assert_eq!(manifest.record().plan.candidate.handler.0, "deb");
    assert_eq!(
        manifest.record().relocation.verdict,
        RelocationVerdict::Pinned
    );
    assert!(manifest.record().files.is_empty());
    let output = fake.run(&["uninstall", game.to_str().unwrap()]);
    assert!(output.status.success(), "{}", value(&output));
    assert_eq!(
        fake.calls(),
        [
            vec!["install", "-y", "--", input.to_str().unwrap()],
            vec!["remove", "-y", "--", "caskade-deb-absolute-path"],
        ]
    );
    assert!(!fake.state.exists());
    assert_storage_clean(&root);
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
#[test]
fn a_deb_removed_by_hand_uninstalls_without_asking_apt() {
    let fake = AptCommand::new(false);
    let root = TestRoot::new().unwrap();
    let input = system_deb();
    let output = fake.run(&[
        "install",
        input.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", value(&output));
    fs::remove_file(&fake.state).unwrap();
    let game = installed_dir(root.path());
    let output = fake.run(&["uninstall", game.to_str().unwrap()]);
    assert!(output.status.success(), "{}", value(&output));
    // apt fails on a package it can't find; it is never asked.
    assert_eq!(
        fake.calls(),
        [vec!["install", "-y", "--", input.to_str().unwrap()]]
    );
    assert_storage_clean(&root);
}

#[cfg(all(feature = "linux-formats", target_os = "linux"))]
#[test]
fn a_failed_apt_install_removes_nothing() {
    let fake = AptCommand::new(true);
    let root = TestRoot::new().unwrap();
    let input = system_deb();
    let output = fake.run(&[
        "install",
        input.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", value(&output));
    // An older version may be installed; a failed install leaves it alone.
    assert_eq!(
        fake.calls(),
        [vec!["install", "-y", "--", input.to_str().unwrap()]]
    );
    assert_storage_clean(&root);
}

#[cfg(all(feature = "fetch", feature = "linux-formats", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloaded_flatpak_uses_its_finalizer_and_removes_completed_downloads() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .insert_header(
                    "content-disposition",
                    "attachment; filename=fixture.flatpak",
                )
                .set_body_bytes(fs::read(bundle()).unwrap()),
        )
        .mount(&server)
        .await;
    let fake = FlatpakCommand::new(false);
    let root = TestRoot::new().unwrap();
    let output = fake.run(&[
        "install",
        &format!("{}/fixture.flatpak", server.uri()),
        "--root",
        root.path().to_str().unwrap(),
        "--job-id",
        "flatpak-download",
    ]);
    let outcome = value(&output);
    assert!(
        output.status.success(),
        "{outcome} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let game = assert_flatpak(root.path(), &outcome);
    assert!(
        !root
            .path()
            .join(".caskade/downloads/flatpak-download")
            .exists()
    );
    assert_eq!(
        InstallManifest::read(&game).unwrap().record().source.input,
        caskade_installer::Input::Urls(vec![
            format!("{}/fixture.flatpak", server.uri()).parse().unwrap()
        ])
    );
    let output = fake.run(&["uninstall", game.to_str().unwrap()]);
    assert!(output.status.success(), "{}", value(&output));
    assert_storage_clean(&root);
}

#[cfg(all(feature = "macos-formats", not(target_os = "macos")))]
#[test]
fn dmg_binary_detects_mac_payload_and_reports_unsupported_without_mounting() {
    // Valid generated UDIF trailer/plist metadata only; no fake native image.
    let source = tempfile::tempdir().unwrap();
    let image = source.path().join("metadata-only.dmg");
    let xml = b"<plist><dict/></plist>";
    let mut bytes = vec![0; 100 + xml.len() + 512];
    bytes[100..100 + xml.len()].copy_from_slice(xml);
    let tail = &mut bytes[100 + xml.len()..];
    tail[..4].copy_from_slice(b"koly");
    tail[4..8].copy_from_slice(&4_u32.to_be_bytes());
    tail[8..12].copy_from_slice(&512_u32.to_be_bytes());
    tail[32..40].copy_from_slice(&100_u64.to_be_bytes());
    tail[216..224].copy_from_slice(&100_u64.to_be_bytes());
    tail[224..232].copy_from_slice(&(xml.len() as u64).to_be_bytes());
    fs::write(&image, bytes).unwrap();
    let probed = binary(&["probe", image.to_str().unwrap()]);
    assert!(probed.status.success(), "{}", value(&probed));
    assert!(String::from_utf8_lossy(&probed.stdout).contains("macos.dmg"));
    let root = TestRoot::new().unwrap();
    let output = binary(&[
        "install",
        image.to_str().unwrap(),
        "--root",
        root.path().to_str().unwrap(),
    ]);
    assert_eq!(value(&output)["type"], "unsupported");
    assert_eq!(output.status.code(), Some(3));
    assert_storage_clean(&root);
}
