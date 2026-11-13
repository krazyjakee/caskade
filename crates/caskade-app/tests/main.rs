//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod acceptance_achievements;
mod acceptance_actions_scripts;
mod account_libraries;
mod backup_roundtrip;
mod deep_link;
mod discovery_controller;
mod igdb;
mod library;
mod permissions;
mod plugin_settings;
mod plugin_ui;
mod reconcile_scan;

use caskade_app::events::{Emit, Event, Events};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

/// What reached the UI, in order.
#[derive(Clone, Default)]
pub struct Sink(Arc<Mutex<Vec<Event>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

#[allow(dead_code)]
impl Sink {
    /// The app's sink over this one.
    pub fn events(&self) -> Events {
        Events::new(self.clone())
    }

    /// Everything sent so far.
    pub fn taken(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }

    /// Waits up to `limit` for a sent event `wanted` picks, and gives what
    /// it picked.
    pub fn wait_for<T>(&self, limit: Duration, wanted: impl Fn(&Event) -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(found) = self.0.lock().unwrap().iter().find_map(&wanted) {
                return Some(found);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The plugin host's test guest, built once for wasm32-wasip2.
#[allow(dead_code)]
fn guest() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../caskade-plugin-host/tests/guest/Cargo.toml");
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
