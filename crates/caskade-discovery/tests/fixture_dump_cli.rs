#![cfg(feature = "cli")]
//! End-to-end fixture export never touches live launcher data.
use crate::fixtures as support;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_caskade-discovery"))
}
fn machine() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/machines/linux-mixed")
}
fn scan(root: &Path) -> serde_json::Value {
    let out = binary()
        .arg("--json")
        .args(["scan", "--source", "steam", "--root"])
        .arg(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!report["groups"].as_array().unwrap().is_empty(), "{report}");
    assert!(
        report["sources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["error"].is_null()),
        "{report}"
    );
    support::relative_value(&report["groups"], root).unwrap()
}
#[test]
fn cli_requires_anonymization_and_replays_nonempty_builtin_source_groups() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("dump");
    let denied = binary()
        .arg("--json")
        .arg("dump")
        .arg("--root")
        .arg(machine())
        .arg(&destination)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(!destination.exists());
    let error: serde_json::Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("--anonymize"));
    let before = scan(&machine());
    let out = binary()
        .arg("--json")
        .args(["dump", "--anonymize", "--root"])
        .arg(machine())
        .arg(&destination)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: caskade_discovery::dump::DumpReport = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!report.files.is_empty());
    assert_eq!(before, scan(&destination));
}
