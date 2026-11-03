//! Genuine runner-generated disk images through the built development CLI.
#![cfg(all(feature = "cli", feature = "macos-formats", target_os = "macos"))]
use crate::fixtures;
use caskade_installer::{
    manifest::{InstallManifest, RootManifest},
    testing::TestRoot,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn attached() -> BTreeSet<PathBuf> {
    let output = Command::new("/usr/bin/hdiutil")
        .args(["info", "-plist"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let plist = plist::Value::from_reader_xml(output.stdout.as_slice()).unwrap();
    plist
        .as_dictionary()
        .unwrap()
        .get("images")
        .and_then(plist::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_dictionary()?.get("image-path")?.as_string())
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.into()))
        .collect()
}
fn cli(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_caskade-install"))
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
#[ignore = "native macOS #22: genuine CASKADE_MACOS_FIXTURES UDZO/ULFO/SLA via CLI"]
fn native_cli_udzo_ulfo_sla_install_launch_uninstall_and_detach() {
    let artifact_root = std::env::var_os("CASKADE_MACOS_FIXTURES")
        .map(PathBuf::from)
        .expect(
            "generate native images with xtask/macos-fixtures.py and set CASKADE_MACOS_FIXTURES",
        );
    let corpus = fixtures::load_macos_native(&artifact_root).unwrap();
    for id in ["mac-dmg-udzo", "mac-dmg-ulfo", "mac-dmg-sla"] {
        let fixture = corpus
            .iter()
            .find(|fixture| fixture.expected.id == id)
            .unwrap();
        let source = fixture.input().canonicalize().unwrap();
        assert!(!attached().contains(&source));
        let root = TestRoot::new().unwrap();
        let outcome = cli(&[
            "install",
            source.to_str().unwrap(),
            "--extract",
            "--root",
            root.path().to_str().unwrap(),
        ]);
        assert_eq!(outcome["type"], "installed");
        let game = root.path().join(outcome["value"]["dir"].as_str().unwrap());
        let manifest = InstallManifest::read(&game).unwrap();
        assert_eq!(manifest.record().plan.candidate.handler.0, "macos-dmg");
        assert!(manifest.verify(&game).unwrap().is_ok());
        let tree: Vec<_> = fixtures::snapshot_tree(&game)
            .unwrap()
            .into_iter()
            .filter(|entry| !Path::new(&entry.path).starts_with(".caskade"))
            .collect();
        assert_eq!(tree, fixture.tree);
        assert!(!attached().contains(&source), "CLI left {id} attached");
        let token = root.path().join("launch-token");
        let output = Command::new(game.join(&fixture.expected.launch_target))
            .args(["--token", token.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(token.is_file());
        fs::remove_file(token).unwrap();
        assert_eq!(
            cli(&["uninstall", game.to_str().unwrap()])["type"],
            "uninstalled"
        );
        assert!(!game.exists());
        assert!(!attached().contains(&source));
        RootManifest::read(root.path()).unwrap();
        for entry in fs::read_dir(root.path()).unwrap() {
            assert_eq!(entry.unwrap().file_name(), ".caskade");
        }
        fs::remove_dir_all(root.path().join(".caskade")).unwrap();
        root.assert_clean();
    }
}
