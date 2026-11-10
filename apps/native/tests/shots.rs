//! Every theme on the reference screens, rendered with vello_cpu (#90):
//! `caskade-native --shots` renders each screen in each valid theme (the
//! built-in ones and the catalogue's Ember, installed for the test), and
//! each must match its committed image in `tests/shots/` within a small
//! tolerance. A change to how a theme looks updates the images:
//!
//! `CASKADE_WRITE_SHOTS=1 cargo test -p caskade-native --features shots --test shots`
//!
//! `apps/native/scripts/webkit-shots.py` and `compare-shots.py` compare the
//! same pages in WebKitGTK; what still differs is listed in
//! `docs/theming/blitz.md` (#85). Headless: no display or bus is used.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A pixel differs when a channel moves by more than this (anti-aliasing
/// and rounding differ a little between CPUs).
const TOLERANCE: u8 = 24;
/// The share of an image's pixels that may differ.
const MAX_DIFFERING: f64 = 0.01;

fn golden() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/shots")
}

fn pngs(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name.ends_with(".png"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The share of pixels that differ by more than [`TOLERANCE`].
fn differing(a: &Path, b: &Path) -> f64 {
    let a = image::open(a).unwrap().into_rgba8();
    let b = image::open(b).unwrap().into_rgba8();
    assert_eq!(a.dimensions(), b.dimensions());
    let count = a
        .pixels()
        .zip(b.pixels())
        .filter(|(x, y)| {
            x.0.iter()
                .zip(y.0.iter())
                .any(|(p, q)| p.abs_diff(*q) > TOLERANCE)
        })
        .count();
    count as f64 / f64::from(a.width() * a.height())
}

#[test]
fn every_theme_renders_the_reference_screens_as_before() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    // Ember is the catalogue's: installed in the user's theme folder.
    let themes = temp.path().join("themes");
    let ember = themes.join("ember");
    std::fs::create_dir_all(&ember).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/ember");
    for file in std::fs::read_dir(&source).unwrap() {
        let file = file.unwrap();
        std::fs::copy(file.path(), ember.join(file.file_name())).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_caskade-native"))
        .arg("--shots")
        .arg(&out)
        .arg("--themes")
        .arg(&themes)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        // The gallery's cards show a time of day.
        .env("TZ", "UTC")
        .output()
        .expect("the native binary runs");
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    let rendered = pngs(&out);
    for theme in ["caskade.default", "caskade.paper", "dev.caskade.ember"] {
        assert!(
            rendered
                .iter()
                .any(|name| name.ends_with(&format!("-{theme}.png"))),
            "{theme} wasn't rendered: {rendered:?}"
        );
    }
    if std::env::var_os("CASKADE_WRITE_SHOTS").is_some() {
        std::fs::create_dir_all(golden()).unwrap();
        for name in pngs(&golden()) {
            std::fs::remove_file(golden().join(name)).unwrap();
        }
        for name in &rendered {
            std::fs::copy(out.join(name), golden().join(name)).unwrap();
        }
        return;
    }
    assert_eq!(
        rendered,
        pngs(&golden()),
        "screens or themes changed: CASKADE_WRITE_SHOTS=1 writes the images"
    );
    let mut problems = Vec::new();
    for name in &rendered {
        let share = differing(&out.join(name), &golden().join(name));
        if share > MAX_DIFFERING {
            problems.push(format!("{name}: {:.2}% of pixels differ", share * 100.0));
        }
    }
    assert!(
        problems.is_empty(),
        "{}\n(rendered in {}; CASKADE_WRITE_SHOTS=1 writes them if the change is meant)",
        problems.join("\n"),
        out.display()
    );
}
