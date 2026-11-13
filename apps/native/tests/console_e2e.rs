//! The console end-to-end harness against the native binary (#92): the
//! built app runs a script of pad presses (`src/input/e2e.rs`, a synthetic
//! pad through the real input path) on a profile of its own, reads its
//! window between presses, writes a report and quits. A pad-only run sees
//! no key or pointer event.
//!
//! Two runs. The frame's (#96): a fresh profile skips first-run setup, a
//! pad goes into the sidebar, opens Collections and goes back, and the
//! guide button opens nothing with no game running. Console mode's (#107,
//! the webview's `console_e2e.rs` script): started with `--console` on a
//! seeded library, Home → a shelf → a game → Play; guide → back to the
//! game; guide → stop; the Library → a ROM game → its emulator; Settings →
//! a switch; then out of console mode. The test then reads the core's
//! records. Not the Store: its catalogue is the metadata server's, and
//! tests never reach the internet.
//!
//! The app runs on a display and session bus of its own
//! (`caskade-test-desktop`), never the developer's (#122):
//! `cargo test -p caskade-native --test console_e2e -- --ignored`

// Unix only: Xvfb and dbus-daemon (caskade-test-desktop).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use caskade_core::{
    AppPaths,
    db::LibraryStore,
    emulation::{
        ArgsOverride, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, GameRom,
        PlatformSettings, ProfileHooks, ProfileRef,
    },
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, PlaySession, SourceId},
    settings::SettingsService,
};
use caskade_emulation_fixtures::{FakeEmulator, Invocation};
use caskade_test_desktop::PrivateDesktop;
use std::time::{Duration, Instant};

const SCRIPT: &str = "\
# A fresh profile: first-run setup (#97) first. Up to its top bar, and
# Skip setup, which opens the library.
wait-text Get started
press nav.up
seek nav.right Skip setup
press confirm
wait-text All Games
# Home, from the sidebar (#98).
seek-section nav.left sidebar
seek nav.up Home
press confirm
wait-text Welcome back
# A pad: into the sidebar, to its top, down to Collections, and A opens it.
press nav.down
seek-section nav.left sidebar
seek nav.up Home
seek nav.down Collections
press confirm
wait-text New Collection
# B goes back a page.
press back
wait-text Welcome back
# With no game running, guide opens no quick menu.
press guide
sleep 300
wait-text Welcome back
";

#[test]
#[ignore = "drives the built app on a private Xvfb display and bus: run with -- --ignored"]
fn a_scripted_pad_drives_the_native_app_with_no_key_or_pointer_event() {
    let desktop = PrivateDesktop::start();
    let temp = tempfile::tempdir().unwrap();
    run(&desktop, temp.path(), SCRIPT, &[], &[]);
}

/// A profile under `home`, as `directories` puts Caskade's folders on
/// Linux with the run's XDG variables.
fn paths(home: &Path) -> AppPaths {
    AppPaths {
        data: home.join("data/caskade"),
        config: home.join("config/caskade"),
        cache: home.join("cache/caskade"),
        logs: home.join("data/caskade/logs"),
        games: home.join("Games/Caskade"),
    }
}

/// Runs `script` in the native binary on `desktop` with a profile under
/// `home` and checks its report: every step ran, and no key or pointer
/// event reached the window.
fn run(
    desktop: &PrivateDesktop,
    home: &Path,
    script_text: &str,
    args: &[&str],
    env: &[(&str, &OsStr)],
) {
    let runtime: PathBuf = home.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let script = home.join("script.e2e");
    std::fs::write(&script, script_text).unwrap();
    let report = home.join("report.json");
    let log = std::fs::File::create(home.join("native.log")).unwrap();
    let mut command = desktop.command(env!("CARGO_BIN_EXE_caskade-native"));
    command
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("NO_AT_BRIDGE", "1")
        .env("GTK_A11Y", "none")
        .env("RUST_LOG", "info")
        .env("CASKADE_E2E_SCRIPT", &script)
        .env("CASKADE_E2E_REPORT", &report);
    for (name, value) in env {
        command.env(name, value);
    }
    let mut app = command
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("the native binary starts");
    // The app bounds its own run: 90 s for the window, 20 s for each step's
    // wait and 20 s to quit (`src/input/e2e.rs`). CI's Xvfb and lavapipe are
    // several times slower than a desktop, so the deadline covers all of it
    // rather than guessing a total.
    let steps = script_text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .count();
    let deadline = Instant::now() + Duration::from_secs(90 + 25 * steps as u64 + 30);
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
    // The step it was on, and the log without the renderer's per-frame
    // warnings and Dioxus's teardown advice, which would fill the tail.
    let last_step = output
        .lines()
        .rev()
        .find(|line| line.contains("e2e") && line.contains("step="))
        .unwrap_or("no step started: the window never answered");
    let tail: String = output
        .lines()
        .filter(|line| !line.contains("vello_common::flatten"))
        .filter(|line| !line.contains("dioxus_signals::warnings"))
        .filter(|line| !line.starts_with(' ') && !line.starts_with('}') && !line.starts_with("```"))
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let status = status.unwrap_or_else(|| {
        panic!("the run never ended; its last step: {last_step}\nits log:\n{tail}")
    });
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&report).unwrap_or_else(|_| panic!("no report; the log:\n{tail}")),
    )
    .unwrap();
    assert_eq!(
        report["ok"], true,
        "the run failed: {}; steps: {}; the log:\n{tail}",
        report["failure"], report["steps"]
    );
    assert!(status.success(), "{status}; the log:\n{tail}");
    assert_eq!(report["steps"].as_array().map(Vec::len), Some(steps));
    assert_eq!(report["keyboardEvents"], 0, "a pad-only run");
    assert_eq!(report["pointerEvents"], 0, "a pad-only run");
}

const RUNNER: &str = "Couch Runner";
const ROM_GAME: &str = "Pocket Quest";

/// Console mode's run (#107), the webview's script: one controller press
/// (or wait) per line.
const CONSOLE_SCRIPT: &str = "\
# Console mode from the start; Home opens on the hero.
wait-event console-on
wait-text Continue Playing
focus-is Resume · View details
# Down into Recently Played (its View all, then the card below it; the
# hero's game isn't in it), then into Recently Added, which has both games:
# LT and RT page the shelf, and focus follows.
press nav.down
focus-is View all →
seek nav.down Pocket Quest
seek-section nav.down shelf-added
press nav.down
seek nav.left Pocket Quest
press page.next
focus-has Couch Runner
press page.prev
focus-has Pocket Quest
press page.next
focus-has Couch Runner
# The game, and Play.
press confirm
wait-text Play from your library
focus-is Play
press confirm
wait-event game-started
wait-text Playing
# Guide: the quick menu, then back to the game.
press guide
wait-text Return to game
focus-is Return to game
press confirm
sleep 1500
# Guide again: stop the game.
press guide
wait-text Return to game
seek nav.down Stop game
press confirm
focus-is Stop Couch Runner
press confirm
wait-event game-exited
# The quick menu closes once the stop it ran returns, which can be after
# the exit: focus back on the page's Play first, then B.
focus-is Play
# Back to Home, into the sidebar, to the Library's emulated games.
press back
wait-text Welcome back
seek-section nav.left sidebar
seek nav.up Home
seek nav.down Library
press confirm
wait-text Save as collection
# The Library's filters are under its link in the sidebar.
seek-section nav.left sidebar
seek nav.down Emulated
press confirm
wait-text Pocket Quest
seek-section nav.right main
seek nav.down Pocket Quest
press confirm
wait-text Play from your library
# Play waits for the emulator check; focus goes to it once it can start.
focus-is Play
press confirm
wait-event game-started
wait-event game-exited
# Settings opens on General, its first tab: RB to Appearance, LB
# back, then a switch in the panel to the right of the tabs: into it, up
# to Reset to defaults at its top, down to Startup's first row, the switch.
seek-section nav.left sidebar
seek nav.up Home
seek nav.down Settings
press confirm
wait-text Launch on system startup
focus-has General
press tab.next
wait-text Interface scale
press tab.prev
wait-text Launch on system startup
focus-has General
press nav.right
seek nav.up Reset general settings
press nav.down
seek nav.left Launch on system startup
press confirm
wait-event settings-changed
# And out of console mode.
press view
wait-event console-off
";

struct Seeded {
    runner: Game,
    rom_game: Game,
    rom: PathBuf,
    record: PathBuf,
}

/// A library with a game that keeps running (so there's something to stop)
/// and a Game Boy Advance ROM played by a fake mGBA; both played before.
/// Setup is done.
async fn seed(paths: &AppPaths, root: &Path) -> Seeded {
    let store = LibraryStore::open_default(paths).await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(paths).unwrap();
    // These games are local fixtures. Matching them must not download
    // Hasheous dumps or query the metadata server during the pad run.
    settings
        .set(
            "core.metadata",
            serde_json::json!({ "online": false }),
            None,
        )
        .await
        .unwrap();
    let mut onboarding = settings.get("core.onboarding").await.unwrap().value;
    onboarding["completed"] = true.into();
    onboarding["sounds"] = false.into();
    settings
        .set("core.onboarding", onboarding, None)
        .await
        .unwrap();
    let day = 86_400_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let games = root.join("games");
    std::fs::create_dir_all(&games).unwrap();
    let runner_exe = games.join("couch-runner.sh");
    std::fs::write(&runner_exe, "#!/bin/sh\nexec sleep 120\n").unwrap();
    make_executable(&runner_exe);
    let runner = Game::new(RUNNER);
    store.upsert_game(&runner).await.unwrap();
    let mut entry = GameEntry::new(&runner.id, SourceId::new("e2e"), "runner");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: games.clone(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: runner_exe,
        args: vec![],
        cwd: None,
        env: BTreeMap::new(),
    });
    let runner_entry = store.upsert_entry(&entry).await.unwrap();

    let fake = FakeEmulator::new(root).unwrap();
    let mgba = root.join("mGBA");
    fake.copy_to(&mgba.join("mgba")).unwrap();
    store
        .upsert_emulator(&Emulator {
            id: "mgba".into(),
            name: "mGBA".into(),
            definition: Some("mgba".into()),
            install: EmulatorInstall::Folder {
                dir: mgba,
                executable: "mgba".into(),
            },
            profiles: vec![EmulatorProfile::BuiltIn {
                id: "gba".into(),
                definition_profile: "Nintendo Game Boy Advance".into(),
                args: ArgsOverride::Default,
                hooks: ProfileHooks::default(),
            }],
        })
        .await
        .unwrap();
    store
        .set_platform_settings(&PlatformSettings {
            platform: "nintendo_gba".into(),
            default: Some(ProfileRef {
                emulator_id: "mgba".into(),
                profile_id: "gba".into(),
            }),
            bios_dir: None,
        })
        .await
        .unwrap();
    let roms = root.join("ROMs/GBA");
    std::fs::create_dir_all(&roms).unwrap();
    let rom = roms.join("Pocket Quest (USA).gba");
    std::fs::write(&rom, b"").unwrap();
    let rom_game = Game::new(ROM_GAME);
    store.upsert_game(&rom_game).await.unwrap();
    let mut entry = GameEntry::new(&rom_game.id, SourceId::new("roms"), "pocket-quest");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: roms });
    entry.launch = Some(LaunchSpec::Emulator {
        platform_id: "nintendo_gba".into(),
        target: EmulatorTarget::PlatformDefault,
        roms: vec![GameRom::absolute(rom.clone())],
        args: ArgsOverride::Default,
    });
    let rom_entry = store.upsert_entry(&entry).await.unwrap();

    // Played before: the runner yesterday (so it leads Home), the ROM game
    // the day before.
    for (game, entry, ago) in [(&runner, &runner_entry, 1), (&rom_game, &rom_entry, 2)] {
        let mut session = PlaySession::new(&game.id, &entry.id, now - ago * day);
        session.ended_at_ms = Some(now - ago * day + 3_600_000);
        session.duration_ms = 3_600_000;
        store.upsert_session(&session).await.unwrap();
    }
    Seeded {
        runner,
        rom_game,
        rom,
        record: root.join("fake-emulator"),
    }
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[ignore = "drives the built app on a private Xvfb display and bus: run with -- --ignored"]
fn console_mode_end_to_end_with_a_pad_only() {
    let desktop = PrivateDesktop::start_sized("1600x1000x24");
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let paths = paths(home);
    paths.create().unwrap();
    let seeded = caskade_app::runtime::block_on(seed(&paths, home));
    run(
        &desktop,
        home,
        CONSOLE_SCRIPT,
        &["--console"],
        &[
            ("CASKADE_FAKE_RECORD", seeded.record.as_os_str()),
            ("CASKADE_FAKE_SLEEP_MS", OsStr::new("1500")),
        ],
    );

    caskade_app::runtime::block_on(async {
        let store = LibraryStore::open_default(&paths).await.unwrap();
        // The runner: played, returned to, stopped; its new session has
        // playtime.
        let runs = store
            .sessions_for_game(&seeded.runner.id, 10)
            .await
            .unwrap();
        assert_eq!(runs.len(), 2, "the old session and the new one");
        let new = runs
            .iter()
            .max_by_key(|session| session.started_at_ms)
            .unwrap();
        assert!(new.ended_at_ms.is_some(), "stop ended the session");
        assert!(new.duration_ms >= 1_000, "playtime: {} ms", new.duration_ms);
        // The ROM game ran in the fake mGBA, with its ROM.
        let rom_runs = store
            .sessions_for_game(&seeded.rom_game.id, 10)
            .await
            .unwrap();
        assert_eq!(rom_runs.len(), 2);
        let invocation = Invocation::read(&seeded.record).expect("the emulator ran");
        assert_eq!(invocation.argv, [seeded.rom.to_string_lossy().into_owned()]);
        // The switch in Settings, and console mode left (and remembered).
        let settings = SettingsService::new(store.clone());
        settings.register_core(&paths).unwrap();
        let general = settings.get("core.general").await.unwrap().value;
        assert_eq!(general["launchOnLogin"], true);
        let console = settings.get("core.console").await.unwrap().value;
        assert_eq!(console["active"], false);
    });
}
