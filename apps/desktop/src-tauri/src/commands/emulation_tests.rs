//! Typed IPC -> EmulationService -> SQLite, plus the `emulation.changed`
//! bridge, on Tauri's mock runtime.
use super::*;
use crate::commands::test_ipc::{assert_report, invoke};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    emulation::{Catalog, OperatingSystem, OsFolders},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

const DEFINITIONS: [(&str, &str); 2] = [
    (
        "platforms.toml",
        r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc"]

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd"]
bios = [{ file = "scph5501.bin", required = true }]
"#,
    ),
    (
        "retroarch.toml",
        r#"
id = "retroarch"
name = "RetroArch"

[vars]
cores = "{EmulatorDir}/cores"
lib = ".so"

[[profile]]
name = "Snes9x"
platforms = ["nintendo_snes"]
extensions = ["sfc", "smc"]
args = ["-L", "{cores}/snes9x_libretro{lib}", "{RomPath}"]
requires = ["{cores}/snes9x_libretro{lib}"]

[[profile]]
name = "Beetle PSX HW"
platforms = ["sony_playstation"]
extensions = ["cue", "chd"]
args = ["-L", "{cores}/mednafen_psx_hw_libretro{lib}", "{RomPath}"]
requires = ["{cores}/mednafen_psx_hw_libretro{lib}"]
"#,
    ),
];

#[test]
fn emulators_profiles_and_platform_defaults_round_trip_through_typed_ipc() {
    // A portable RetroArch with only the SNES core installed.
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("RetroArch");
    std::fs::create_dir_all(dir.join("cores")).unwrap();
    std::fs::write(dir.join("retroarch"), b"").unwrap();
    std::fs::write(dir.join("cores/snes9x_libretro.so"), b"").unwrap();
    let home = temp.path().join("home");

    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let service = EmulationService::new(store, bus.clone(), Catalog::load(DEFINITIONS).unwrap())
        .with_host(
            OperatingSystem::Linux,
            OsFolders {
                user_home: Some(home.clone()),
                ..OsFolders::default()
            },
        );
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            emulation_catalog,
            emulation_overview,
            emulators_list,
            emulator_save,
            emulator_delete,
            profile_preview,
            args_split,
            emulation_platforms,
            platform_settings_set
        ])
        .events(tauri_specta::collect_events![
            crate::events::EmulationChanged
        ]);
    let app = mock_builder()
        .manage(service)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    crate::events::bridge(app.handle().clone(), &bus);
    let scopes = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = scopes.clone();
    app.listen(crate::events::EmulationChanged::NAME, move |event| {
        let value: Value = serde_json::from_str(event.payload()).unwrap();
        seen.lock()
            .unwrap()
            .push(value["scope"].as_str().unwrap().to_owned());
    });
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let catalog = invoke(&window, "emulation_catalog", json!({})).unwrap();
    assert_eq!(catalog["platforms"].as_array().unwrap().len(), 2);
    assert_eq!(catalog["definitions"][0]["vars"], json!(["cores", "lib"]));
    assert_eq!(
        catalog["definitions"][0]["profiles"][0]["requires"],
        json!(["{cores}/snes9x_libretro{lib}"])
    );
    let rom_path = catalog["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variable| variable["name"] == "RomPath")
        .unwrap();
    assert_eq!(rom_path["aliases"], json!(["ImagePath"]));

    // New emulators and profiles get ids from the backend.
    let saved = invoke(
        &window,
        "emulator_save",
        json!({"emulator": {
            "id": "",
            "name": "RetroArch",
            "definition": "retroarch",
            "install": {"type": "folder", "dir": dir, "executable": "retroarch"},
            "profiles": [
                {"type": "built_in", "id": "", "definitionProfile": "Snes9x",
                 "args": {"type": "default"}, "hooks": {"before": null, "afterExit": null}},
                {"type": "built_in", "id": "", "definitionProfile": "Beetle PSX HW",
                 "args": {"type": "replace", "args": ["--psx", "{RomPath}"]},
                 "hooks": {"before": null, "afterExit": null}},
            ],
        }}),
    )
    .unwrap();
    let emulator_id = saved["emulator"]["id"].as_str().unwrap().to_owned();
    assert!(!emulator_id.is_empty());
    assert_eq!(saved["status"], "some_missing");
    let snes = &saved["profiles"][0];
    let psx = &saved["profiles"][1];
    assert_eq!(snes["availability"], json!({"type": "available"}));
    assert_eq!(snes["preview"]["program"], json!(dir.join("retroarch")));
    assert_eq!(
        snes["preview"]["args"],
        json!([
            "-L",
            dir.join("cores/snes9x_libretro.so"),
            home.join("ROMs/SNES/Sample Game (USA).sfc"),
        ])
    );
    assert_eq!(psx["availability"]["type"], "missing_files");
    assert_eq!(
        psx["bios"],
        // Checked where this RetroArch looks for BIOS files: not there.
        json!([{"file": "scph5501.bin", "required": true, "found": false, "state": "missing",
            "path": null, "platform": "sony_playstation"}])
    );
    // The Replace override remembers the definition args it replaced.
    assert_eq!(
        saved["emulator"]["profiles"][1]["args"]["replaced"],
        json!(["-L", "{cores}/mednafen_psx_hw_libretro{lib}", "{RomPath}"])
    );
    assert_eq!(psx["drift"], Value::Null);
    let snes_id = snes["id"].as_str().unwrap().to_owned();
    assert_eq!(
        invoke(&window, "emulators_list", json!({})).unwrap()[0]["emulator"]["id"],
        json!(emulator_id)
    );

    // Unknown variables fail when saved, with a readable message.
    let mut broken = saved["emulator"].clone();
    broken["profiles"][1]["args"]["args"] = json!(["{corez}"]);
    let error = invoke(&window, "emulator_save", json!({"emulator": broken})).unwrap_err();
    assert_report(&error, "invalid", &[]);
    assert!(
        error["summary"]
            .as_str()
            .unwrap()
            .contains("nknown variable `{corez}`"),
        "{error}"
    );

    // Drafts preview without saving; typed arguments split in the backend.
    let draft = json!({"draft": {
        "install": {"type": "flatpak", "appId": "org.libretro.RetroArch"},
        "definition": null,
        "profile": {"type": "custom", "id": "", "name": "Mine", "executable": null,
            "args": ["--rom", "{RomPath}"], "workingDir": "{RomDir}",
            "platforms": ["sony_playstation"], "extensions": ["chd"], "env": [],
            "tracking": {"type": "default"}, "hooks": {"before": null, "afterExit": null}},
    }});
    let preview = invoke(&window, "profile_preview", draft).unwrap();
    assert_eq!(preview["program"], "flatpak");
    assert_eq!(
        preview["args"],
        json!([
            "run",
            "org.libretro.RetroArch",
            "--rom",
            home.join("ROMs/PS1/Sample Game (USA).chd"),
        ])
    );
    assert_eq!(preview["cwd"], json!(home.join("ROMs/PS1")));
    assert_eq!(
        invoke(
            &window,
            "args_split",
            json!({"text": r#"-L "my core.so" {RomPath}"#})
        )
        .unwrap(),
        json!(["-L", "my core.so", "{RomPath}"])
    );
    assert!(invoke(&window, "args_split", json!({"text": "\"unbalanced"})).is_err());

    // Platform defaults accept compatible profiles only.
    let platforms = invoke(&window, "emulation_platforms", json!({})).unwrap();
    let snes_view = platforms
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["platform"]["id"] == "nintendo_snes")
        .unwrap();
    assert_eq!(snes_view["compatible"][0]["profileId"], json!(snes_id));
    assert_eq!(snes_view["compatible"][0]["available"], true);
    let default = json!({"emulatorId": emulator_id, "profileId": snes_id});
    let set = invoke(
        &window,
        "platform_settings_set",
        json!({"settings": {"platform": "nintendo_snes", "default": default, "biosDir": null}}),
    )
    .unwrap();
    assert_eq!(set["settings"]["default"], default);
    assert!(
        invoke(
            &window,
            "platform_settings_set",
            json!({"settings": {"platform": "sony_playstation", "default": default, "biosDir": null}}),
        )
        .is_err()
    );
    let overview = invoke(&window, "emulation_overview", json!({})).unwrap();
    assert_eq!(
        overview,
        json!({"emulators": 1, "platformsWithDefault": 1, "roms": 0, "biosIssues": 0})
    );

    invoke(&window, "emulator_delete", json!({"id": emulator_id})).unwrap();
    let gone = invoke(&window, "emulator_delete", json!({"id": emulator_id})).unwrap_err();
    assert_report(&gone, "conflict", &[emulator_id.as_str()]);
    assert_eq!(
        invoke(&window, "emulators_list", json!({})).unwrap(),
        json!([])
    );

    // Every write told the UI what to refetch.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !scopes.lock().unwrap().contains(&"emulators".to_owned()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let scopes = scopes.lock().unwrap();
    assert!(scopes.contains(&"emulators".to_owned()), "{scopes:?}");
    assert!(scopes.contains(&"platforms".to_owned()), "{scopes:?}");
}

/// Detect -> review -> import through typed IPC, against a fixture machine
/// with Flatpak RetroArch (both cores) and nothing else from these
/// definitions.
#[test]
fn detection_starts_reports_and_imports_through_typed_ipc() {
    use caskade_emulation_fixtures::{FixtureOs, MachineFixture};
    let fixture = MachineFixture::new(FixtureOs::Linux).unwrap();
    let env = caskade_discovery::HostEnv::fixture(fixture.path()).unwrap();
    let mut definitions = DEFINITIONS;
    definitions[1].1 = r#"
id = "retroarch"
name = "RetroArch"

[detect.linux]
flatpak = "org.libretro.RetroArch"

[vars]
cores = "{EmulatorDir}/cores"
lib = ".so"
[vars.linux.flatpak]
cores = "{UserHome}/.var/app/org.libretro.RetroArch/config/retroarch/cores"
[vars.linux]
cores = "{XdgConfig}/retroarch/cores"

[[profile]]
name = "Snes9x"
platforms = ["nintendo_snes"]
args = ["-L", "{cores}/snes9x_libretro{lib}", "{RomPath}"]
requires = ["{cores}/snes9x_libretro{lib}"]
"#;
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let service = EmulationService::new(store, bus.clone(), Catalog::load(definitions).unwrap())
        .with_host(
            OperatingSystem::Linux,
            caskade_core::emulation::detect_folders(&env),
        )
        .with_host_env(env);
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            emulation_detect_start,
            emulation_detect_get,
            emulation_detect_latest,
            emulation_detect_cancel,
            emulation_detect_import,
            emulation_platforms
        ])
        .events(tauri_specta::collect_events![
            crate::events::EmulationChanged,
            crate::events::EmulationDetectChanged
        ]);
    let app = mock_builder()
        .manage(service)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    crate::events::bridge(app.handle().clone(), &bus);
    let jobs = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = jobs.clone();
    app.listen(crate::events::EmulationDetectChanged::NAME, move |event| {
        seen.lock()
            .unwrap()
            .push(serde_json::from_str(event.payload()).unwrap());
    });
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    assert_eq!(
        invoke(&window, "emulation_detect_latest", json!({})).unwrap(),
        Value::Null
    );
    assert!(
        invoke(
            &window,
            "emulation_detect_start",
            json!({"folder": "relative"})
        )
        .is_err()
    );
    let started = invoke(&window, "emulation_detect_start", json!({"folder": null})).unwrap();
    assert_eq!(started["status"], "running");
    let id = started["id"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(10);
    let job = loop {
        let job = invoke(&window, "emulation_detect_get", json!({"id": id})).unwrap();
        if job["status"] != "running" || Instant::now() > deadline {
            break job;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(job["status"], "done", "{job}");
    let results = job["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{job}");
    assert_eq!(
        results[0]["install"],
        json!({"type": "flatpak", "appId": "org.libretro.RetroArch"})
    );
    assert_eq!(
        results[0]["profiles"],
        json!([{"name": "Snes9x", "available": true}])
    );
    assert_eq!(results[0]["configured"], false);
    assert_eq!(
        invoke(&window, "emulation_detect_latest", json!({})).unwrap()["id"],
        json!(id)
    );

    let report = invoke(
        &window,
        "emulation_detect_import",
        json!({"id": id, "revision": job["revision"], "keys": [results[0]["key"]]}),
    )
    .unwrap();
    assert_eq!(report["emulators"].as_array().unwrap().len(), 1);
    assert_eq!(report["defaultsSet"], json!(["nintendo_snes"]));
    let platforms = invoke(&window, "emulation_platforms", json!({})).unwrap();
    let snes = platforms
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["platform"]["id"] == "nintendo_snes")
        .unwrap();
    assert_eq!(
        snes["settings"]["default"]["emulatorId"],
        report["emulators"][0]
    );
    let gone = invoke(&window, "emulation_detect_cancel", json!({"id": "det-1"})).unwrap_err();
    assert_report(&gone, "invalid", &["det-1"]);
    let gone = invoke(&window, "emulation_detect_get", json!({"id": "det-1"})).unwrap_err();
    assert_report(&gone, "conflict", &["det-1"]);

    // The UI saw the job start, finish and get marked configured.
    let deadline = Instant::now() + Duration::from_secs(5);
    while jobs.lock().unwrap().len() < 3 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let jobs = jobs.lock().unwrap();
    assert_eq!(jobs.first().unwrap()["status"], "running");
    assert_eq!(jobs.last().unwrap()["results"][0]["configured"], true);
}

/// #122: a game's emulator settings and readiness through typed IPC.
#[test]
fn games_report_readiness_and_take_per_game_settings_through_typed_ipc() {
    use caskade_core::{
        emulation::{ArgsOverride, EmulatorTarget, GameRom},
        model::{Game, GameEntry, LaunchSpec},
    };
    let temp = tempfile::tempdir().unwrap();
    let rom = temp.path().join("Chrono Trigger (USA).sfc");
    std::fs::write(&rom, b"rom").unwrap();
    let dir = temp.path().join("RetroArch");
    std::fs::create_dir_all(dir.join("cores")).unwrap();
    std::fs::write(dir.join("retroarch"), b"").unwrap();
    std::fs::write(dir.join("cores/snes9x_libretro.so"), b"").unwrap();
    let (store, entry) = tauri::async_runtime::block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = store
            .upsert_game(&Game::new("Chrono Trigger"))
            .await
            .unwrap();
        let mut entry = GameEntry::new(&game.id, caskade_core::install::HANDOFF_SOURCE, "chrono");
        entry.launch = Some(LaunchSpec::Emulator {
            platform_id: "nintendo_snes".into(),
            target: EmulatorTarget::PlatformDefault,
            roms: vec![GameRom::absolute(&rom)],
            args: ArgsOverride::Default,
        });
        let entry = store.upsert_entry(&entry).await.unwrap();
        (store, entry)
    });
    let service = EmulationService::new(store, Bus::default(), Catalog::load(DEFINITIONS).unwrap());
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new().commands(
        tauri_specta::collect_commands![emulation_game, emulation_game_set, emulator_save],
    );
    let app = mock_builder()
        .manage(service)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let game = invoke(&window, "emulation_game", json!({"entryId": entry.id})).unwrap();
    assert_eq!(game["readiness"], json!({"type": "no_emulator"}));
    assert_eq!(game["message"], "No emulator for SNES — configure");
    assert_eq!(game["discs"][0]["exists"], true);

    let saved = invoke(
        &window,
        "emulator_save",
        json!({"emulator": {
            "id": "", "name": "RetroArch", "definition": "retroarch",
            "install": {"type": "folder", "dir": dir, "executable": "retroarch"},
            "profiles": [{"type": "built_in", "id": "", "definitionProfile": "Snes9x",
                "args": {"type": "default"}, "hooks": {"before": null, "afterExit": null}}],
        }}),
    )
    .unwrap();
    let pin = json!({
        "type": "profile",
        "emulatorId": saved["emulator"]["id"],
        "profileId": saved["profiles"][0]["id"],
    });
    let game = invoke(
        &window,
        "emulation_game_set",
        json!({"entryId": entry.id, "target": pin, "args": {"type": "append", "args": ["--verbose"]}}),
    )
    .unwrap();
    assert_eq!(game["readiness"], json!({"type": "ready"}));
    assert_eq!(game["message"], Value::Null);
    assert_eq!(game["profile"]["profileName"], "Snes9x");
    assert_eq!(
        game["args"],
        json!({"type": "append", "args": ["--verbose"]})
    );
    assert!(
        invoke(
            &window,
            "emulation_game_set",
            json!({"entryId": entry.id, "target": {"type": "profile", "emulatorId": "x", "profileId": "y"}, "args": {"type": "default"}}),
        )
        .is_err()
    );
}
