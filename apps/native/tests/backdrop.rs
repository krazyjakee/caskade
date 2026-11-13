//! #149: the backdrop moves on the GPU on its own clock. Once Home has
//! settled, frames of the backdrop alone keep being presented while the
//! document isn't resolved or painted again. A window hidden to the tray
//! draws nothing at all, the document's own animations included, and its
//! renderer's GPU resources are released until it is shown.
//! The app starts on a display and session bus of its own
//! (`caskade-test-desktop`) and draws through wgpu even on a software
//! adapter (`CASKADE_RENDERER=gpu`: lavapipe in CI).
//!
//! `cargo test -p caskade-native --test backdrop -- --ignored`

// Unix only: Xvfb and dbus-daemon (caskade-test-desktop).
#![cfg(unix)]

use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use caskade_test_desktop::PrivateDesktop;

struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Frames {
    document: u64,
    backdrop: u64,
    gpu: bool,
    released: bool,
}

/// The app's frame counts (`CASKADE_NATIVE_SMOKE_FRAMES`), once it wrote
/// them.
fn frames(file: &Path) -> Option<Frames> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    Some(Frames {
        document: json["document"].as_u64()?,
        backdrop: json["backdrop"].as_u64()?,
        gpu: json["gpu"].as_bool()?,
        released: json["released"].as_bool()?,
    })
}

/// Waits up to `limit` for counts that satisfy `done`.
fn until(file: &Path, limit: Duration, done: impl Fn(&Frames) -> bool) -> Option<Frames> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(frames) = frames(file).filter(|frames| done(frames)) {
            return Some(frames);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn an_idle_window_moves_its_backdrop_without_painting_the_document() {
    let desktop = PrivateDesktop::start_sized("1280x800x24");
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let counts = home.join("frames.json");
    // Home's hero has ambient loops of its own: paused, so only the
    // backdrop moves.
    let appearance = serde_json::json!({
        "customCssEnabled": true,
        "customCss": "[data-cs=\"hero\"] [data-part=\"fx\"]::after, [data-cs=\"hero\"] [data-part=\"glow\"] { animation-play-state: paused; }",
    });
    let hide_after = 30.0;
    let log = std::fs::File::create(home.join("native.log")).unwrap();
    let started = Instant::now();
    let _app = App(desktop
        .command(env!("CARGO_BIN_EXE_caskade-native"))
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("CASKADE_FIXTURES", "1")
        .env("CASKADE_NATIVE_NO_TRAY", "1")
        .env("CASKADE_RENDERER", "gpu")
        // Xvfb copies every frame on the CPU: frames at the backdrop's
        // own rate here, not its 1%-of-a-core share.
        .env("ANYRENDER_BACKDROP_CPU_SHARE", "1")
        .env("CASKADE_NATIVE_SMOKE_FRAMES", &counts)
        .env("CASKADE_NATIVE_SMOKE_APPEARANCE", appearance.to_string())
        .env("CASKADE_NATIVE_SMOKE_HIDE", hide_after.to_string())
        .env("RUST_LOG", "info")
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap());
    let log_text = || std::fs::read_to_string(home.join("native.log")).unwrap_or_default();
    let first = until(&counts, Duration::from_secs(25), |frames| {
        frames.gpu && frames.backdrop > 0
    })
    .unwrap_or_else(|| {
        panic!(
            "the GPU never drew the backdrop on its own: {:?}\n{}",
            frames(&counts),
            log_text()
        )
    });
    eprintln!("first backdrop frame: {first:?} at {:?}", started.elapsed());
    // Home settles: its data arrives and its entrances end.
    std::thread::sleep(Duration::from_secs(6));
    let settled = frames(&counts).unwrap();
    std::thread::sleep(Duration::from_secs(4));
    let later = frames(&counts).unwrap();
    eprintln!("settled {settled:?}, 4 s later {later:?}");
    assert!(
        started.elapsed() < Duration::from_secs_f64(hide_after),
        "the app started too slowly to measure before it hides"
    );
    assert!(later.gpu);
    assert_eq!(
        later.document, settled.document,
        "the document was painted again while only the backdrop moved"
    );
    // Nexus asks for 6 a second.
    assert!(
        later.backdrop - settled.backdrop >= 10,
        "too few backdrop frames: {settled:?} → {later:?}"
    );
    assert!(!later.released);
    // Hidden, as closing to the tray does: released, and no more frames.
    let wait = Duration::from_secs_f64(hide_after).saturating_sub(started.elapsed());
    std::thread::sleep(wait);
    let hidden = until(&counts, Duration::from_secs(5), |frames| frames.released)
        .unwrap_or_else(|| panic!("the hidden window kept its GPU resources\n{}", log_text()));
    std::thread::sleep(Duration::from_secs(3));
    let still_hidden = frames(&counts).unwrap();
    assert_eq!(still_hidden, hidden, "a hidden window kept drawing");
}

/// The document's own animations stop too while the window is hidden: the
/// status bar's spinner turns (`CASKADE_NATIVE_SMOKE_MOTION`) and paints
/// the document every frame until then.
#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn a_hidden_window_stops_its_document_animations() {
    let desktop = PrivateDesktop::start_sized("1280x800x24");
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let counts = home.join("frames.json");
    let log = std::fs::File::create(home.join("native.log")).unwrap();
    let _app = App(desktop
        .command(env!("CARGO_BIN_EXE_caskade-native"))
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("CASKADE_FIXTURES", "1")
        .env("CASKADE_NATIVE_NO_TRAY", "1")
        .env("CASKADE_RENDERER", "gpu")
        // Xvfb copies every frame on the CPU: frames at the backdrop's
        // own rate here, not its 1%-of-a-core share.
        .env("ANYRENDER_BACKDROP_CPU_SHARE", "1")
        .env("CASKADE_NATIVE_SMOKE_FRAMES", &counts)
        .env("CASKADE_NATIVE_SMOKE_MOTION", "spinner")
        .env("CASKADE_NATIVE_SMOKE_HIDE", "12")
        .env("RUST_LOG", "info")
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap());
    let log_text = || std::fs::read_to_string(home.join("native.log")).unwrap_or_default();
    let shown = until(&counts, Duration::from_secs(25), |frames| {
        frames.document > 20
    })
    .unwrap_or_else(|| panic!("the spinner didn't animate\n{}", log_text()));
    eprintln!("animating: {shown:?}");
    let hidden = until(&counts, Duration::from_secs(25), |frames| frames.released)
        .unwrap_or_else(|| panic!("the hidden window kept its GPU resources\n{}", log_text()));
    std::thread::sleep(Duration::from_secs(3));
    let later = frames(&counts).unwrap();
    eprintln!("hidden: {hidden:?}, 3 s later {later:?}");
    assert!(
        later.document <= hidden.document + 1,
        "a hidden window kept painting its document: {hidden:?} → {later:?}"
    );
    assert_eq!(later.backdrop, hidden.backdrop);
}

/// A theme installed in the user's folder whose backdrop is its own shader.
fn shader_theme(home: &Path) {
    let theme = home.join("data/caskade/themes/waves");
    std::fs::create_dir_all(theme.join("assets")).unwrap();
    std::fs::write(
        theme.join("theme.json"),
        r#"{"id": "dev.example.waves", "name": "Waves", "version": "0.1.0", "extends": "caskade.default"}"#,
    )
    .unwrap();
    std::fs::write(
        theme.join("backdrop.json"),
        r#"{"frameRate": 20, "layers": [
          {"type": "shader", "src": "assets/waves.wgsl", "colors": ["{color.accent}"],
           "numbers": ["{effect.glow.strength}"], "loop": "60s"}
        ]}"#,
    )
    .unwrap();
    std::fs::write(
        theme.join("assets/waves.wgsl"),
        "fn layer(input: Input) -> vec4<f32> {
    let x = input.pixel.x / max(input.size.x, 1.0);
    let wave = 0.5 + 0.5 * sin(x * 12.0 + input.time);
    return vec4<f32>(input.colors[0].rgb, wave * 0.4 * input.numbers.x);
}
",
    )
    .unwrap();
}

/// A theme's shader layer compiles into the backdrop pass and moves on the
/// backdrop's clock; over its GPU budget (here set to nothing) it stops.
#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn a_theme_shader_moves_and_stops_over_its_gpu_budget() {
    for budget in [None, Some("0")] {
        let desktop = PrivateDesktop::start_sized("1280x800x24");
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        shader_theme(home);
        let counts = home.join("frames.json");
        let log = std::fs::File::create(home.join("native.log")).unwrap();
        let mut command = desktop.command(env!("CARGO_BIN_EXE_caskade-native"));
        command
            .env("HOME", home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("CASKADE_FIXTURES", "1")
            .env("CASKADE_NATIVE_NO_TRAY", "1")
            .env("CASKADE_RENDERER", "gpu")
            // Xvfb copies every frame on the CPU: frames at the backdrop's
            // own rate here, not its 1%-of-a-core share.
            .env("ANYRENDER_BACKDROP_CPU_SHARE", "1")
            .env("CASKADE_NATIVE_SMOKE_FRAMES", &counts)
            .env(
                "CASKADE_NATIVE_SMOKE_APPEARANCE",
                r#"{"theme": "dev.example.waves", "customCssEnabled": true, "customCss": "[data-cs=\"hero\"] [data-part=\"fx\"]::after, [data-cs=\"hero\"] [data-part=\"glow\"] { animation-play-state: paused; }"}"#,
            )
            .env("RUST_LOG", "info")
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        // Without a test budget the shader gets a generous one: lavapipe runs
        // shaders on the CPU, and a loaded CI runner can take longer than
        // the default budget a real GPU is held to.
        command.env(
            "ANYRENDER_BACKDROP_SHADER_BUDGET_MS",
            budget.unwrap_or("10000"),
        );
        let _app = App(command.spawn().unwrap());
        let log_text = || std::fs::read_to_string(home.join("native.log")).unwrap_or_default();
        until(&counts, Duration::from_secs(25), |frames| {
            frames.gpu && frames.backdrop > 0
        })
        .unwrap_or_else(|| panic!("the GPU never drew the backdrop: {}", log_text()));
        assert!(
            log_text().contains("\"dev.example.waves\""),
            "the shader theme wasn't applied\n{}",
            log_text()
        );
        std::thread::sleep(Duration::from_secs(5));
        let settled = frames(&counts).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        let later = frames(&counts).unwrap();
        eprintln!("budget {budget:?}: {settled:?} → {later:?}");
        assert!(!log_text().contains("didn't compile"), "{}", log_text());
        match budget {
            // A software GPU (CI's lavapipe: ~44 ms a frame) runs the shader
            // over its fixed per-frame budget, so the watch rightly stops it
            // there: only the stop can be checked on such a machine.
            None if log_text().contains("they stop moving") => {
                eprintln!(
                    "this GPU runs the shader over its frame budget: only its stop is checked"
                );
                assert_eq!(later.backdrop, settled.backdrop);
            }
            None => {
                // Wait for a few more frames rather than a rate: a loaded
                // machine can take a while over each.
                let moved = until(&counts, Duration::from_secs(20), |frames| {
                    frames.backdrop >= settled.backdrop + 3
                })
                .unwrap_or_else(|| {
                    panic!(
                        "the shader didn't move: {settled:?} → {:?}\n{}",
                        frames(&counts),
                        log_text()
                    )
                });
                assert!(!log_text().contains("stop moving"));
                assert_eq!(moved.document, settled.document);
            }
            Some(_) => {
                assert!(
                    log_text().contains("they stop moving"),
                    "the watch didn't stop the shader\n{}",
                    log_text()
                );
                assert_eq!(
                    later.backdrop, settled.backdrop,
                    "a stopped shader kept drawing frames"
                );
            }
        }
        assert_eq!(later.document, settled.document);
    }
}

/// Presenting a frame on Xvfb costs tens of milliseconds of CPU (it copies
/// it): the backdrop's frames slow to keep within 1% of a core.
#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn costly_backdrop_frames_slow_to_their_share_of_a_core() {
    let desktop = PrivateDesktop::start_sized("1280x800x24");
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let counts = home.join("frames.json");
    let log = std::fs::File::create(home.join("native.log")).unwrap();
    let _app = App(desktop
        .command(env!("CARGO_BIN_EXE_caskade-native"))
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("CASKADE_FIXTURES", "1")
        .env("CASKADE_NATIVE_NO_TRAY", "1")
        .env("CASKADE_RENDERER", "gpu")
        .env("CASKADE_NATIVE_SMOKE_FRAMES", &counts)
        .env("RUST_LOG", "info")
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap());
    let log_text = || std::fs::read_to_string(home.join("native.log")).unwrap_or_default();
    until(&counts, Duration::from_secs(25), |frames| {
        frames.gpu && frames.backdrop > 0
    })
    .unwrap_or_else(|| panic!("the GPU never drew the backdrop: {}", log_text()));
    std::thread::sleep(Duration::from_secs(4));
    let settled = frames(&counts).unwrap();
    std::thread::sleep(Duration::from_secs(5));
    let later = frames(&counts).unwrap();
    eprintln!("{settled:?} → {later:?}");
    assert!(
        log_text().contains("backdrop frames cost"),
        "the frames weren't slowed\n{}",
        log_text()
    );
    // Nexus asks for 6 a second: 30 in 5 s; at 1% of a core, a couple.
    assert!(
        later.backdrop - settled.backdrop <= 6,
        "too many costly frames: {settled:?} → {later:?}"
    );
}
