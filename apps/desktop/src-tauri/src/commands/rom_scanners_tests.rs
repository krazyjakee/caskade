//! Typed IPC -> ScannerService -> SQLite, plus the scan events, on Tauri's
//! mock runtime.
use super::*;
use crate::commands::test_ipc::{assert_report, invoke};
use caskade_core::{bus::Bus, emulation::Catalog};
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

const PLATFORMS: &str = r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd"]

[[region]]
id = "usa"
name = "USA"
codes = ["USA"]
"#;

/// Polls `rom_scan_jobs` until the scan leaves `running`.
fn settled(window: &tauri::WebviewWindow<tauri::test::MockRuntime>, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let jobs = invoke(window, "rom_scan_jobs", json!({})).unwrap();
        let job = jobs
            .as_array()
            .and_then(|jobs| jobs.iter().find(|job| job["id"] == id))
            .cloned()
            .expect("the scan is retained");
        if job["status"] != "running" {
            return job;
        }
        assert!(Instant::now() < deadline, "the scan didn't finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn scanners_scan_review_and_import_through_typed_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let roms = temp.path().join("PS1");
    std::fs::create_dir_all(&roms).unwrap();
    for name in [
        "Tekken 3 (USA).chd",
        "Game (USA) (Disc 1).chd",
        "Game (USA) (Disc 2).chd",
        "Demo (USA).chd",
    ] {
        std::fs::write(roms.join(name), b"ROM").unwrap();
    }
    let emu = temp.path().join("emu");
    std::fs::create_dir_all(&emu).unwrap();
    std::fs::write(emu.join("emu"), b"").unwrap();

    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let bus = Bus::default();
    let service = EmulationService::new(
        store.clone(),
        bus.clone(),
        Catalog::load([("platforms.toml", PLATFORMS)]).unwrap(),
    );
    tauri::async_runtime::block_on(
        service.save_emulator(
            serde_json::from_value(json!({
                "id": "emu", "name": "Emu",
                "install": {"type": "folder", "dir": emu, "executable": "emu"},
                "profiles": [{"type": "custom", "id": "psx", "name": "PSX", "args": ["{RomPath}"],
                    "platforms": ["sony_playstation"], "extensions": ["chd"], "env": [],
                    "tracking": {"type": "default"}, "hooks": {}}],
            }))
            .unwrap(),
        ),
    )
    .unwrap();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            rom_scanners_list,
            rom_scanner_save,
            rom_scanner_delete,
            rom_scan_start,
            rom_scan_jobs,
            rom_scan_cancel,
            rom_scan_submit,
            rom_scan_all,
            rom_exclusions,
            rom_exclusions_set
        ])
        .events(tauri_specta::collect_events![
            crate::events::EmulationChanged,
            crate::events::EmulationScanChanged,
            crate::events::LibraryChanged
        ]);
    let app = mock_builder()
        .manage(service)
        .manage(store.clone())
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    crate::events::bridge(app.handle().clone(), &bus);
    let statuses = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = statuses.clone();
    app.listen(crate::events::EmulationScanChanged::NAME, move |event| {
        let value: Value = serde_json::from_str(event.payload()).unwrap();
        seen.lock()
            .unwrap()
            .push(value["status"].as_str().unwrap().to_owned());
    });
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    // A new scanner gets its id from the backend.
    let scanner = invoke(
        &window,
        "rom_scanner_save",
        json!({"scanner": {
            "id": "", "name": "PS1", "folder": roms, "platform": null,
            "profile": {"emulatorId": "emu", "profileId": "psx"},
            "launchWith": {"type": "platform_default"},
            "recursive": true, "scanArchives": false, "skipCloudPlaceholders": true,
            "exclusions": [], "inLibraryUpdate": true,
        }}),
    )
    .unwrap();
    let id = scanner["id"].as_str().unwrap().to_owned();
    assert!(!id.is_empty());
    let mut invalid = scanner.clone();
    invalid["exclusions"] = json!(["?("]);
    let bad = invoke(&window, "rom_scanner_save", json!({"scanner": invalid}));
    assert!(bad.is_err(), "an invalid exclusion regex is refused");

    // A manual scan stops at the review list.
    let job = invoke(
        &window,
        "rom_scan_start",
        json!({"scannerId": id, "mode": "review"}),
    )
    .unwrap();
    let review = settled(&window, job["id"].as_str().unwrap());
    assert_eq!(review["status"], "review");
    let games = review["review"].as_array().unwrap();
    assert_eq!(games.len(), 3);
    let choices: Vec<Value> = games
        .iter()
        .map(|game| {
            json!({
                "roms": game["roms"].as_array().unwrap().iter().map(|r| r["path"].clone()).collect::<Vec<_>>(),
                "include": game["title"] != "Demo",
                "title": game["title"], "platform": game["platform"], "regions": game["regions"],
            })
        })
        .collect();
    let done = invoke(
        &window,
        "rom_scan_submit",
        json!({"id": review["id"], "revision": review["revision"], "choices": choices}),
    )
    .unwrap();
    assert_eq!(done["status"], "done");
    assert_eq!(done["summary"]["added"], 2);

    let listed = invoke(&window, "rom_scanners_list", json!({})).unwrap();
    assert_eq!(listed[0]["games"], 2);
    assert_eq!(listed[0]["platforms"], json!(["sony_playstation"]));
    assert_eq!(
        listed[0]["scanner"]["exclusions"],
        json!(["Demo (USA).chd"])
    );
    assert!(listed[0]["lastScanMs"].is_number());
    assert_eq!(listed[0]["flatpakHint"], Value::Null);

    // Scan all agrees with the review.
    let all = invoke(&window, "rom_scan_all", json!({})).unwrap();
    assert_eq!(all[0]["added"], 0);
    assert_eq!(all[0]["games"].as_array().unwrap().len(), 2);
    assert!(invoke(&window, "rom_scan_jobs", json!({})).unwrap()[0]["id"] == review["id"]);

    // #135: a global exclusion drops Tekken from the next scan.
    assert_eq!(
        invoke(&window, "rom_exclusions", json!({})).unwrap(),
        json!([])
    );
    let set = invoke(
        &window,
        "rom_exclusions_set",
        json!({"patterns": [">Tekken*", " "]}),
    )
    .unwrap();
    assert_eq!(set, json!([">Tekken*"]));
    assert!(invoke(&window, "rom_exclusions_set", json!({"patterns": ["?("]})).is_err());
    assert_eq!(invoke(&window, "rom_exclusions", json!({})).unwrap(), set);
    let all = invoke(&window, "rom_scan_all", json!({})).unwrap();
    assert_eq!(all[0]["missing"], 1);
    assert_eq!(all[0]["games"].as_array().unwrap().len(), 1);

    invoke(&window, "rom_scanner_delete", json!({"id": id})).unwrap();
    let gone = invoke(&window, "rom_scanner_delete", json!({"id": id})).unwrap_err();
    assert_report(&gone, "conflict", &[id.as_str()]);
    let gone = invoke(&window, "rom_scan_cancel", json!({"id": "scan-1"})).unwrap_err();
    assert_report(&gone, "conflict", &["scan-1"]);
    let gone = invoke(
        &window,
        "rom_scan_start",
        json!({"scannerId": "scanner-1", "mode": "review"}),
    )
    .unwrap_err();
    assert_report(&gone, "invalid", &["scanner-1"]);
    assert!(invoke(&window, "rom_scanners_list", json!({})).unwrap() == json!([]));

    let deadline = Instant::now() + Duration::from_secs(5);
    while !statuses.lock().unwrap().iter().any(|s| s == "done") {
        assert!(Instant::now() < deadline, "no emulationScanChanged events");
        std::thread::sleep(Duration::from_millis(10));
    }
    let statuses = statuses.lock().unwrap();
    assert_eq!(statuses.first().map(String::as_str), Some("running"));
    assert!(statuses.iter().any(|s| s == "review"));
}
