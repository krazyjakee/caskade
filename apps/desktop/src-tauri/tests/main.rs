//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod acceptance_achievements;
mod acceptance_actions_scripts;
mod account_libraries;
mod backup_roundtrip;
mod bridge;
// Needs X and the built frontend: `--features e2e`, then `-- --ignored`.
#[cfg(feature = "e2e")]
mod console_e2e;
mod discovery_controller;
mod hello;
mod igdb;
#[path = "../src/commands/test_ipc.rs"]
mod ipc;
mod library;
mod m4_exit;
mod plugin_settings;
mod plugin_ui;
mod reconcile_scan;

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

/// The plugin host's test guest, built once for wasm32-wasip2.
fn guest() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/caskade-plugin-host/tests/guest/Cargo.toml");
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-guest");
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "--release",
                "--locked",
                "--target",
                "wasm32-wasip2",
            ])
            .arg("--manifest-path")
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", &target)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("run cargo for the test guest");
        assert!(
            status.success(),
            "building the test guest failed; is the target installed? \
             `rustup target add wasm32-wasip2`"
        );
        target.join("wasm32-wasip2/release/caskade_test_plugin.wasm")
    })
}
