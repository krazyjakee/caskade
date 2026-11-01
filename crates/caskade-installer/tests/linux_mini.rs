//! Linux I2 catalogue coverage and real installed launch proofs.
#![cfg(feature = "linux-formats")]
use std::path::{Path, PathBuf};
use crate::fixtures;
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
#[test]
fn catalogue_contains_every_linux_acceptance_mini_without_duplicate_payloads() {
    let corpus = fixtures::load_linux_mini(&fixture_root()).unwrap();
    let ids: Vec<_> = corpus
        .iter()
        .map(|fixture| fixture.expected.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "appimage-indie",
            "deb-missing-dep",
            "flatpak-bundle",
            "gog-linux-mojosetup",
            "humble-linux-tar",
            "itch-zip-with-manifest",
            "rpm-indie"
        ]
    );
    assert!(
        corpus
            .iter()
            .all(|fixture| fixture.expected.platform == "linux")
    );
    assert_eq!(
        corpus
            .iter()
            .filter(|fixture| fixture.expected.input.starts_with("bin/linux-mini/"))
            .count(),
        1
    );
    let rpm = corpus
        .iter()
        .find(|fixture| fixture.expected.id == "rpm-indie")
        .unwrap();
    assert_eq!(
        rpm.input(),
        fixture_root().join("bin/packages/rpm-self-contained.rpm")
    );
}

#[cfg(all(feature = "cli", target_os = "linux"))]
#[test]
fn linux_minis_install_verify_launch_and_uninstall_through_the_binary() {
    use caskade_installer::{
        manifest::{InstallManifest, ROOT_MANIFEST_PATH},
        testing::TestRoot,
    };
    use std::{fs, process::Command};
    for fixture in fixtures::load_linux_mini(&fixture_root()).unwrap() {
        if matches!(
            fixture.expected.id.as_str(),
            "deb-missing-dep" | "flatpak-bundle"
        ) {
            // System-managed cases use isolated native integration tests instead.
            continue;
        }
        let root = TestRoot::new().unwrap();
        let input = fixture.input();
        let output = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .args(["--json", "install", "--extract", "--root"])
            .arg(root.path())
            .arg(&input)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: stdout={} stderr={}",
            fixture.expected.id,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let outcome: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(outcome["type"], "installed", "{}", fixture.expected.id);
        let game = root.path().join(outcome["value"]["dir"].as_str().unwrap());
        let manifest = InstallManifest::read(&game).unwrap();
        assert!(manifest.verify(&game).unwrap().is_ok());
        assert_eq!(
            manifest.record().launch_targets[0].path,
            Path::new(&fixture.expected.launch_target),
            "{}",
            fixture.expected.id
        );
        if fixture.expected.id == "itch-zip-with-manifest" {
            assert_eq!(outcome["value"]["title"], "Caskade Itch Fixture");
            assert!(
                manifest.record().launch_targets[0]
                    .reason
                    .to_ascii_lowercase()
                    .contains("itch")
            );
            let mut actual = fixtures::snapshot_tree(&game).unwrap();
            actual.retain(|entry| entry.path != ".caskade" && !entry.path.starts_with(".caskade/"));
            assert_eq!(actual, fixture.tree);
        }
        let scratch = tempfile::tempdir().unwrap();
        let token = scratch.path().join("launch-token");
        let target = &manifest.record().launch_targets[0];
        let mut command = Command::new(game.join(&target.path));
        command
            .current_dir(game.join(&target.working_dir))
            .envs(&target.env)
            .args(&target.args)
            .arg("--token")
            .arg(&token);
        if fixture.expected.id == "appimage-indie" {
            // Use the official runtime's FUSE-free launch path on ordinary Linux
            // and the Steam Deck. Still launches the real AppImage AppRun.
            command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        }
        let launched = command.output().unwrap();
        assert!(
            launched.status.success(),
            "{}: {}",
            fixture.expected.id,
            String::from_utf8_lossy(&launched.stderr)
        );
        let executable = fs::read_to_string(&token).unwrap();
        let suffix = if fixture.expected.id == "appimage-indie" {
            "/usr/bin/indie-game"
        } else {
            "/FixtureGame.x86_64"
        };
        assert!(
            executable.trim().ends_with(suffix),
            "{}: {executable}",
            fixture.expected.id
        );
        assert!(
            manifest.verify(&game).unwrap().is_ok(),
            "launch altered installed payload"
        );
        let removed = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
            .args(["--json", "uninstall"])
            .arg(&game)
            .output()
            .unwrap();
        assert!(
            removed.status.success(),
            "{}",
            String::from_utf8_lossy(&removed.stderr)
        );
        assert!(!game.exists());
        fs::remove_file(root.path().join(ROOT_MANIFEST_PATH)).unwrap();
        fs::remove_dir(root.path().join(".caskade")).unwrap();
        root.assert_clean();
        assert!(input.is_file());
    }
}
