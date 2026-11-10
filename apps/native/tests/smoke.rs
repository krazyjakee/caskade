//! #87's smoke test: the native binary starts on a library of its own,
//! restores its window's saved place and size, plays a web game and opens
//! a video (each a wry window in a process of its own), and quits cleanly
//! with its window state saved. Each test starts the app on a display and
//! session bus of its own (`caskade-test-desktop`: Xvfb and dbus-daemon,
//! no window manager), never the developer's (#122); `xwininfo` reads its
//! windows. Nothing reaches the internet: the game and the video's page are
//! served here, on loopback.
//!
//! `cargo test -p caskade-native --test smoke -- --ignored`

// Unix only: Xvfb and dbus-daemon (caskade-test-desktop).
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use caskade_test_desktop::PrivateDesktop;

use caskade_core::{
    AppPaths,
    db::LibraryStore,
    model::{Game, GameEntry, InstallState, LaunchSpec},
};

/// Serves `page(path)` for every request on loopback; the address.
fn serve(page: fn(&str) -> String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
            // The headers, up to the blank line.
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                line.clear();
            }
            let body = page(&path);
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    address
}

fn page(path: &str) -> String {
    let title = if path.starts_with("/embed/") {
        "Smoke Video"
    } else {
        "Smoke Game"
    };
    format!("<!doctype html><title>{title}</title><p>{title}</p>")
}

/// Every top-level window on the display: its title and geometry
/// (`WIDTHxHEIGHT+X+Y`), as `xwininfo` says.
fn windows(display: &str) -> Vec<(String, String)> {
    let output = Command::new("xwininfo")
        .args(["-display", display, "-root", "-tree"])
        .output()
        .expect("xwininfo runs");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.split_once(" \"")?.1;
            let (title, rest) = rest.split_once("\":")?;
            let geometry = rest.split_whitespace().find(|word| {
                word.contains('x')
                    && word.contains('+')
                    && word.chars().next().is_some_and(|c| c.is_ascii_digit())
            })?;
            Some((title.to_owned(), geometry.to_owned()))
        })
        .collect()
}

/// Waits up to `limit` for a window titled `title`; its geometry.
fn window(display: &str, title: &str, limit: Duration) -> Option<String> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some((_, geometry)) = windows(display).into_iter().find(|(name, _)| name == title) {
            return Some(geometry);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

/// The processes `parent` started, by their parent id in /proc.
fn children(parent: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    let after = stat.rsplit_once(')')?.1;
                    after.split_whitespace().nth(1)?.parse::<u32>().ok()
                })
                == Some(parent)
        })
        .collect()
}

fn wait(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// The profile's paths, as the binary finds them under these XDG folders.
fn profile(home: &Path) -> AppPaths {
    AppPaths {
        data: home.join("data/caskade"),
        config: home.join("config/caskade"),
        cache: home.join("cache/caskade"),
        logs: home.join("data/caskade/logs"),
        games: home.join("Games/Caskade"),
    }
}

#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn starts_restores_its_window_plays_a_web_game_and_a_video_and_quits() {
    let desktop = PrivateDesktop::start();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_path_buf();
    let paths = profile(&home);
    paths.create().unwrap();
    let site = serve(page);
    // A web game in the library, served above.
    let entry = caskade_app::runtime::block_on(async {
        let store = LibraryStore::open_default(&paths).await.unwrap();
        let game = store.upsert_game(&Game::new("Smoke Game")).await.unwrap();
        let mut entry = GameEntry::new(&game.id, caskade_core::web::WEB_SOURCE, &site);
        entry.state = InstallState::Installed;
        entry.launch = Some(LaunchSpec::Web {
            url: format!("{site}/"),
        });
        store.upsert_entry(&entry).await.unwrap();
        entry.id
    });
    // Where the window was last time.
    let state_file = paths.config.join("native-window.json");
    std::fs::write(
        &state_file,
        r#"{"x":40,"y":30,"width":1100,"height":700,"maximized":false}"#,
    )
    .unwrap();
    let runtime: PathBuf = home.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let log = std::fs::File::create(home.join("native.log")).unwrap();
    let mut app = desktop
        .command(env!("CARGO_BIN_EXE_caskade-native"))
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("NO_AT_BRIDGE", "1")
        .env("GTK_A11Y", "none")
        .env("RUST_LOG", "info")
        .env("CASKADE_VIDEO_EMBED", format!("{site}/embed/"))
        .env("CASKADE_NATIVE_SMOKE_LAUNCH", &entry)
        .env("CASKADE_NATIVE_SMOKE_VIDEO", "aaaaaaaaaaa")
        .env("CASKADE_NATIVE_SMOKE_QUIT", "25")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("the native binary starts");
    let pid = app.id();
    let checks = std::panic::catch_unwind(|| {
        // The game's window is titled "Caskade" too until its page loads:
        // the main window is the one at the restored place and size.
        // Without a window manager the window goes exactly where it asks.
        let restored = "1100x700+40+30";
        let deadline = Instant::now() + Duration::from_secs(30);
        while !windows(desktop.display())
            .iter()
            .any(|(title, geometry)| title == "Caskade" && geometry == restored)
        {
            assert!(
                Instant::now() < deadline,
                "no main window at {restored}: {:?}",
                windows(desktop.display())
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        let game = window(desktop.display(), "Smoke Game", Duration::from_secs(30))
            .expect("the web game's window");
        let video = window(desktop.display(), "Smoke test", Duration::from_secs(30))
            .expect("the video window");
        (game, video)
    });
    // Whatever happened: the app, and the windows it started, end here.
    let started = children(pid);
    let status = wait(&mut app, Duration::from_secs(40));
    for child in started {
        let _ = Command::new("kill").arg(child.to_string()).status();
    }
    if status.is_none() {
        let _ = app.kill();
    }
    let output = std::fs::read_to_string(home.join("native.log")).unwrap_or_default();
    let (game, video) = checks.unwrap_or_else(|panic| {
        eprintln!("{output}");
        std::panic::resume_unwind(panic)
    });
    eprintln!("game window {game}, video window {video}");
    let status = status.unwrap_or_else(|| panic!("the app didn't quit:\n{output}"));
    assert!(status.success(), "{status}:\n{output}");
    assert!(output.contains("Caskade (native) quit"), "{output}");
    assert!(!output.contains("panicked"), "{output}");
    let saved = std::fs::read_to_string(&state_file).unwrap();
    assert!(saved.contains("\"width\":1100"), "{saved}");
}

/// A panic in the window's UI restarts Caskade (one event loop is all a
/// process gets), within the restart budget the webviews share; past it
/// Caskade stops instead of looping.
#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn a_panicking_window_restarts_within_the_budget_then_stops() {
    let desktop = PrivateDesktop::start();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_path_buf();
    let runtime = home.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let log_path = home.join("native.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut app = desktop
        .command(env!("CARGO_BIN_EXE_caskade-native"))
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("RUST_LOG", "info")
        .env("CASKADE_NATIVE_SMOKE_PANIC", "1")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("the native binary starts");
    // The first process hands over to its restart and ends.
    wait(&mut app, Duration::from_secs(60)).expect("the panicked process ends");
    let deadline = Instant::now() + Duration::from_secs(120);
    let output = loop {
        let output = std::fs::read_to_string(&log_path).unwrap_or_default();
        if output.contains("won't restart it again") || Instant::now() > deadline {
            break output;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    assert!(output.contains("won't restart it again"), "{output}");
    // Each start got as far as the window's UI and panicked there, on
    // purpose: not a display it couldn't open.
    assert_eq!(
        output.matches("panic: smoke: a panic in the UI").count(),
        caskade_player::recovery::RESTARTS + 1,
        "{output}"
    );
    assert_eq!(
        output.matches("restarting after the window failed").count(),
        caskade_player::recovery::RESTARTS,
        "{output}"
    );
    assert_eq!(
        output.matches("Caskade (native) starting").count(),
        caskade_player::recovery::RESTARTS + 1,
        "{output}"
    );
}

/// A second start hands its `caskade://` link to the running Caskade and
/// ends; the running one queues the link's install for confirmation (#88).
#[test]
#[ignore = "starts the app on a private Xvfb display and bus: run with -- --ignored"]
fn a_second_start_hands_its_link_to_the_running_caskade() {
    let desktop = PrivateDesktop::start();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_path_buf();
    let runtime = home.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let start = |name: &str, args: &[&str], quit: Option<&str>| {
        let log = std::fs::File::create(home.join(name)).unwrap();
        let mut command = desktop.command(env!("CARGO_BIN_EXE_caskade-native"));
        command
            .args(args)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("RUST_LOG", "info")
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        if let Some(seconds) = quit {
            command.env("CASKADE_NATIVE_SMOKE_QUIT", seconds);
        }
        command.spawn().expect("the native binary starts")
    };
    let read = |name: &str| std::fs::read_to_string(home.join(name)).unwrap_or_default();
    let mut first = start("first.log", &[], Some("20"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !read("first.log").contains("services started") {
        assert!(Instant::now() < deadline, "{}", read("first.log"));
        std::thread::sleep(Duration::from_millis(200));
    }
    let link = "caskade://install?url=http%3A%2F%2F127.0.0.1%3A1%2Fgame.zip";
    let mut second = start("second.log", &[link], None);
    let status = wait(&mut second, Duration::from_secs(20));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !read("first.log").contains("deep link awaits confirmation") && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    let ended = wait(&mut first, Duration::from_secs(40));
    if ended.is_none() {
        let _ = first.kill();
    }
    let (first_log, second_log) = (read("first.log"), read("second.log"));
    let status = status.unwrap_or_else(|| panic!("the second start didn't end:\n{second_log}"));
    assert!(status.success(), "{status}\n{second_log}");
    assert!(second_log.contains("Caskade already runs"), "{second_log}");
    assert!(!second_log.contains("services started"), "{second_log}");
    assert!(
        first_log.contains("deep link awaits confirmation"),
        "{first_log}"
    );
    assert!(ended.is_some_and(|status| status.success()), "{first_log}");
}
