//! The console end-to-end harness against the native binary (#92): the
//! built app runs a script of pad presses (`src/input/e2e.rs`, a synthetic
//! pad through the real input path) on a profile of its own, reads its
//! window between presses, writes a report and quits. A pad-only run sees
//! no key or pointer event.
//!
//! Until the views arrive (#96) the script only checks the plumbing: the
//! window answers, presses go through, the guide button opens nothing with
//! no game running. The webview's full console script runs here once the
//! native views exist.
//!
//! `xvfb-run -a cargo test -p caskade-native --test console_e2e -- --ignored`

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SCRIPT: &str = "\
# The window is up, and shows the app.
wait-text Caskade
# Pad presses reach the input root; with nothing to focus, focus stays put.
press nav.down
press nav.right 2
focus-is
# With no game running, guide opens no quick menu.
press guide
sleep 300
focus-is
press back
";

#[test]
#[ignore = "drives the built app under X: run with -- --ignored under xvfb-run"]
fn a_scripted_pad_drives_the_native_app_with_no_key_or_pointer_event() {
    assert!(
        std::env::var_os("DISPLAY").is_some(),
        "run under Xvfb: xvfb-run -a cargo test …"
    );
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_path_buf();
    let runtime: PathBuf = home.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let script = home.join("script.e2e");
    std::fs::write(&script, SCRIPT).unwrap();
    let report = home.join("report.json");
    let log = std::fs::File::create(home.join("native.log")).unwrap();
    let mut app = Command::new(env!("CARGO_BIN_EXE_caskade-native"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("DISPLAY", std::env::var_os("DISPLAY").unwrap())
        // xvfb-run's display needs its cookie.
        .envs(std::env::var_os("XAUTHORITY").map(|file| ("XAUTHORITY", file)))
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("NO_AT_BRIDGE", "1")
        .env("GTK_A11Y", "none")
        .env("RUST_LOG", "info")
        .env("CASKADE_E2E_SCRIPT", &script)
        .env("CASKADE_E2E_REPORT", &report)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("the native binary starts");
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = app.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() > deadline {
            let _ = app.kill();
            let _ = app.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let output = std::fs::read_to_string(home.join("native.log")).unwrap_or_default();
    let tail: String = output
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let status = status.unwrap_or_else(|| panic!("the run never ended; its log:\n{tail}"));
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&report).unwrap_or_else(|_| panic!("no report; the log:\n{tail}")),
    )
    .unwrap();
    assert_eq!(
        report["ok"], true,
        "the run failed: {}; the log:\n{tail}",
        report["failure"]
    );
    assert!(status.success(), "{status}; the log:\n{tail}");
    let steps = SCRIPT
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .count();
    assert_eq!(report["steps"].as_array().map(Vec::len), Some(steps));
    assert_eq!(report["keyboardEvents"], 0, "a pad-only run");
    assert_eq!(report["pointerEvents"], 0, "a pad-only run");
}
