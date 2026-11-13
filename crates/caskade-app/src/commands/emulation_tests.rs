//! The emulation commands -> EmulationService -> SQLite, plus the
//! `emulation.changed` bridge. Arguments are built from the JSON the UI
//! sends and replies read as JSON, to keep the assertions on their fields.
use super::*;
use crate::events::{Emit, Event, Events};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    emulation::{Catalog, OperatingSystem, OsFolders},
    error::ErrorCode,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// What reached the UI.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<Event>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

/// Asserts that `error` is a report of `code` whose summary names none of
/// `internal` (ids, namespaces, type names).
fn assert_report(error: &CommandError, code: ErrorCode, internal: &[&str]) {
    assert_eq!(error.code, code, "{error:?}");
    for name in internal {
        assert!(
            !error.summary.contains(name),
            "{} names {name}",
            error.summary
        );
    }
}

fn to_json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

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

#[tokio::test(flavor = "multi_thread")]
async fn emulators_profiles_and_platform_defaults_round_trip() {
    // A portable RetroArch with only the SNES core installed.
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("RetroArch");
    std::fs::create_dir_all(dir.join("cores")).unwrap();
    std::fs::write(dir.join("retroarch"), b"").unwrap();
    std::fs::write(dir.join("cores/snes9x_libretro.so"), b"").unwrap();
    let home = temp.path().join("home");

    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let service = EmulationService::new(store, bus.clone(), Catalog::load(DEFINITIONS).unwrap())
        .with_host(
            OperatingSystem::Linux,
            OsFolders {
                user_home: Some(home.clone()),
                ..OsFolders::default()
            },
        );
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);

    let catalog = to_json(emulation_catalog(&service));
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
    let saved = emulator_save(
        &service,
        from_json(json!({
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
        })),
    )
    .await
    .unwrap();
    let saved = to_json(saved);
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
    let listed = emulators_list(&service).await.unwrap();
    assert_eq!(to_json(&listed)[0]["emulator"]["id"], json!(emulator_id));

    // Unknown variables fail when saved, with a readable message.
    let mut broken = saved["emulator"].clone();
    broken["profiles"][1]["args"]["args"] = json!(["{corez}"]);
    let error = emulator_save(&service, from_json(broken))
        .await
        .unwrap_err();
    assert_report(&error, ErrorCode::Invalid, &[]);
    assert!(
        error.summary.contains("nknown variable `{corez}`"),
        "{error:?}"
    );

    // Drafts preview without saving; typed arguments split in the backend.
    let draft = from_json(json!({
        "install": {"type": "flatpak", "appId": "org.libretro.RetroArch"},
        "definition": null,
        "profile": {"type": "custom", "id": "", "name": "Mine", "executable": null,
            "args": ["--rom", "{RomPath}"], "workingDir": "{RomDir}",
            "platforms": ["sony_playstation"], "extensions": ["chd"], "env": [],
            "tracking": {"type": "default"}, "hooks": {"before": null, "afterExit": null}},
    }));
    let preview = to_json(profile_preview(&service, &draft));
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
        args_split(r#"-L "my core.so" {RomPath}"#).unwrap(),
        ["-L", "my core.so", "{RomPath}"]
    );
    assert!(args_split("\"unbalanced").is_err());

    // Platform defaults accept compatible profiles only.
    let platforms = to_json(emulation_platforms(&service).await.unwrap());
    let snes_view = platforms
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["platform"]["id"] == "nintendo_snes")
        .unwrap();
    assert_eq!(snes_view["compatible"][0]["profileId"], json!(snes_id));
    assert_eq!(snes_view["compatible"][0]["available"], true);
    let default = json!({"emulatorId": emulator_id, "profileId": snes_id});
    let set = platform_settings_set(
        &service,
        from_json(json!({"platform": "nintendo_snes", "default": default, "biosDir": null})),
    )
    .await
    .unwrap();
    assert_eq!(to_json(set)["settings"]["default"], default);
    assert!(
        platform_settings_set(
            &service,
            from_json(json!({"platform": "sony_playstation", "default": default, "biosDir": null})),
        )
        .await
        .is_err()
    );
    let overview = emulation_overview(&service).await.unwrap();
    assert_eq!(
        to_json(overview),
        json!({"emulators": 1, "platformsWithDefault": 1, "roms": 0, "biosIssues": 0})
    );

    emulator_delete(&service, &emulator_id).await.unwrap();
    let gone = emulator_delete(&service, &emulator_id).await.unwrap_err();
    assert_report(&gone, ErrorCode::Conflict, &[emulator_id.as_str()]);
    assert!(emulators_list(&service).await.unwrap().is_empty());

    // Every write told the UI what to refetch.
    let scopes = || {
        sink.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::EmulationChanged(changed) => Some(changed.scope.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !scopes().contains(&"emulators".to_owned()) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let scopes = scopes();
    assert!(scopes.contains(&"emulators".to_owned()), "{scopes:?}");
    assert!(scopes.contains(&"platforms".to_owned()), "{scopes:?}");
}

/// Detect -> review -> import, against a fixture machine with Flatpak
/// RetroArch (both cores) and nothing else from these definitions.
#[tokio::test(flavor = "multi_thread")]
async fn detection_starts_reports_and_imports() {
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
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let service = EmulationService::new(store, bus.clone(), Catalog::load(definitions).unwrap())
        .with_host(
            OperatingSystem::Linux,
            caskade_core::emulation::detect_folders(&env),
        )
        .with_host_env(env);
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);

    assert_eq!(emulation_detect_latest(&service), None);
    assert!(
        emulation_detect_start(&service, Some("relative".into()))
            .await
            .is_err()
    );
    let started = to_json(emulation_detect_start(&service, None).await.unwrap());
    assert_eq!(started["status"], "running");
    let id = started["id"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(10);
    let job = loop {
        let job = to_json(emulation_detect_get(&service, &id).unwrap());
        if job["status"] != "running" || Instant::now() > deadline {
            break job;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
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
        emulation_detect_latest(&service).map(|job| job.id),
        Some(id.clone())
    );

    let revision = u32::try_from(job["revision"].as_u64().unwrap()).unwrap();
    let keys = vec![results[0]["key"].as_str().unwrap().to_owned()];
    let report = to_json(
        emulation_detect_import(&service, &id, revision, &keys)
            .await
            .unwrap(),
    );
    assert_eq!(report["emulators"].as_array().unwrap().len(), 1);
    assert_eq!(report["defaultsSet"], json!(["nintendo_snes"]));
    let platforms = to_json(emulation_platforms(&service).await.unwrap());
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
    let gone = emulation_detect_cancel(&service, "det-1").unwrap_err();
    assert_report(&gone, ErrorCode::Invalid, &["det-1"]);
    let gone = emulation_detect_get(&service, "det-1").unwrap_err();
    assert_report(&gone, ErrorCode::Conflict, &["det-1"]);

    // The UI saw the job start, finish and get marked configured.
    let jobs = || {
        sink.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::EmulationDetectChanged(changed) => Some(to_json(&changed.0)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while jobs().len() < 3 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let jobs = jobs();
    assert_eq!(jobs.first().unwrap()["status"], "running");
    assert_eq!(jobs.last().unwrap()["results"][0]["configured"], true);
}

/// #122: a game's emulator settings and readiness.
#[tokio::test(flavor = "multi_thread")]
async fn games_report_readiness_and_take_per_game_settings() {
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
    let service = EmulationService::new(store, Bus::default(), Catalog::load(DEFINITIONS).unwrap());

    let game = to_json(emulation_game(&service, &entry.id).await.unwrap());
    assert_eq!(game["readiness"], json!({"type": "no_emulator"}));
    assert_eq!(game["message"], "No emulator for SNES — configure");
    assert_eq!(game["discs"][0]["exists"], true);

    let saved = to_json(
        emulator_save(
            &service,
            from_json(json!({
                "id": "", "name": "RetroArch", "definition": "retroarch",
                "install": {"type": "folder", "dir": dir, "executable": "retroarch"},
                "profiles": [{"type": "built_in", "id": "", "definitionProfile": "Snes9x",
                    "args": {"type": "default"}, "hooks": {"before": null, "afterExit": null}}],
            })),
        )
        .await
        .unwrap(),
    );
    let pin = from_json(json!({
        "type": "profile",
        "emulatorId": saved["emulator"]["id"],
        "profileId": saved["profiles"][0]["id"],
    }));
    let game = to_json(
        emulation_game_set(
            &service,
            &entry.id,
            pin,
            from_json(json!({"type": "append", "args": ["--verbose"]})),
        )
        .await
        .unwrap(),
    );
    assert_eq!(game["readiness"], json!({"type": "ready"}));
    assert_eq!(game["message"], Value::Null);
    assert_eq!(game["profile"]["profileName"], "Snes9x");
    assert_eq!(
        game["args"],
        json!({"type": "append", "args": ["--verbose"]})
    );
    assert!(
        emulation_game_set(
            &service,
            &entry.id,
            from_json(json!({"type": "profile", "emulatorId": "x", "profileId": "y"})),
            ArgsOverride::Default,
        )
        .await
        .is_err()
    );
}
