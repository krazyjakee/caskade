//! #153, the M5 exit: Caskade in console mode, driven by a pad only. The
//! built app (frontend embedded, the `e2e` feature's scripted pad) runs
//! against a fresh library: Home → a shelf → a game → Play; guide → back to
//! the game; guide → stop; the Library → a ROM game → its emulator; Settings
//! → a switch; then out of console mode. Input arrives only through
//! `InputService::inject`, and the page must see no keyboard or pointer
//! event. The test then reads the core's records: sessions with playtime,
//! the emulator's invocation and the changed settings.
//!
//! It needs the built frontend, so it is ignored by default:
//!
//! ```sh
//! pnpm build
//! cargo test -p caskade-desktop --features e2e --test it console_e2e:: -- --ignored
//! ```
//!
//! The app runs on an Xvfb display and a session bus of its own
//! (`caskade-test-desktop`), never the developer's (#122): its tray icon
//! and single-instance check stay away from the desktop's.

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
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const RUNNER: &str = "Couch Runner";
const ROM_GAME: &str = "Pocket Quest";

/// The player's run, one controller press (or wait) per line.
const SCRIPT: &str = r#"

# Console mode from the start; Home opens on the hero.
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
# Play waits for the emulator check; go to it once it can start.
sleep 1000
seek nav.down Play
press confirm
wait-event game-started
wait-event game-exited
# Settings opens on General, its first tab: RB to Appearance, LB
# back, then a switch in the panel to the right of the tabs. The panel has
# one column at this size (two from a 72rem panel, #41): into it, up to
# Reset to defaults at its top, down to Startup's first row, the switch.
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
"#;

struct Seeded {
    runner: Game,
    rom_game: Game,
    rom: PathBuf,
    record: PathBuf,
}

/// A library with a game that keeps running (so there's something to stop)
/// and a Game Boy Advance ROM played by a fake mGBA; both played before.
async fn seed(paths: &AppPaths, root: &Path) -> Seeded {
    let store = LibraryStore::open_default(paths).await.unwrap();
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

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[ignore = "drives the built app under X with the e2e feature; see the module docs"]
fn console_mode_end_to_end_with_a_pad_only() {
    let desktop = caskade_test_desktop::PrivateDesktop::start_sized("1600x1000x24");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let home = root.join("home");
    let (data, config, cache, state) = (
        root.join("xdg/data"),
        root.join("xdg/config"),
        root.join("xdg/cache"),
        root.join("xdg/state"),
    );
    // Where `directories` puts Caskade's folders on Linux.
    let paths = AppPaths {
        data: data.join("caskade"),
        config: config.join("caskade"),
        cache: cache.join("caskade"),
        logs: data.join("caskade/logs"),
        games: home.join("Games/Caskade"),
    };
    paths.create().unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let seeded = tauri::async_runtime::block_on(seed(&paths, root));
    let script = root.join("script.txt");
    std::fs::write(&script, SCRIPT).unwrap();
    let report_path = root.join("report.json");

    let mut app = desktop
        .command(env!("CARGO_BIN_EXE_caskade"))
        .arg("--console")
        .current_dir(root)
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_STATE_HOME", &state)
        .env("CASKADE_E2E_SCRIPT", &script)
        .env("CASKADE_E2E_REPORT", &report_path)
        .env("CASKADE_FAKE_RECORD", &seeded.record)
        .env("CASKADE_FAKE_SLEEP_MS", "1500")
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        )
        .spawn()
        .expect("the caskade binary");
    let deadline = Instant::now() + Duration::from_secs(300);
    let status = loop {
        if let Some(status) = app.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = app.kill();
            panic!("the run didn't finish in time");
        }
        std::thread::sleep(Duration::from_millis(250));
    };

    let report: caskade_desktop::e2e::Report =
        serde_json::from_slice(&std::fs::read(&report_path).expect("a report")).unwrap();
    assert!(
        report.ok,
        "the run failed: {:?} after {} steps",
        report.failure,
        report.steps.len()
    );
    assert!(status.success());
    assert_eq!(
        (report.keyboard_events, report.pointer_events),
        (0, 0),
        "a pad-only run sends no keys and no pointer"
    );

    tauri::async_runtime::block_on(async {
        let store = LibraryStore::open_default(&paths).await.unwrap();
        // The runner: played, returned to, stopped; its new session has playtime.
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
