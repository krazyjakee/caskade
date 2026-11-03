#![cfg(feature = "cli")]
use std::{fs, process::Command};
fn fixture() -> tempfile::TempDir {
    let dir = crate::fixtures::tempdir();
    fs::write(
        dir.path().join("machine.toml"),
        "schema_version=1\nos=\"linux\"\n",
    )
    .unwrap();
    let game = dir.path().join("root/games/Original");
    fs::create_dir_all(&game).unwrap();
    fs::write(game.join("data.win"), b"original game marker").unwrap();
    fs::write(game.join("start.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
    dir
}
#[test]
fn classify_binary_prints_score_evidence_and_launch_as_json() {
    let dir = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_caskade-discovery"))
        .args(["classify", "/games/Original", "--root"])
        .arg(dir.path())
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["verdict"], "likely");
    assert!(value["score"].as_i64().unwrap() >= 60);
    assert!(
        value["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["value"]["rule"] == "fingerprint.gamemaker")
    );
    assert!(
        value["launch"]["value"]["program"]
            .as_str()
            .unwrap()
            .ends_with("start.sh")
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("classifying"));
    assert!(!dir.path().join("root/games/Original/.caskade").exists());
}
#[test]
fn thresholds_are_configurable_and_invalid_input_is_a_clean_error() {
    let dir = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_caskade-discovery"))
        .args(["classify", "/games/Original", "--root"])
        .arg(dir.path())
        .args(["--likely-score", "90", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["verdict"], "maybe");
    let output = Command::new(env!("CARGO_BIN_EXE_caskade-discovery"))
        .args(["classify", "/games/Original", "--root"])
        .arg(dir.path())
        .args(["--maybe-score", "90", "--likely-score", "10", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["error"].as_str().unwrap().contains("requires"));
}
