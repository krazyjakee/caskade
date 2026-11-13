//! The ROM scanner commands -> ScannerService -> SQLite, plus the scan
//! events the UI hears.
use super::*;
use crate::{
    error::ErrorCode,
    events::{Emit, Event, Events},
};
use caskade_core::{
    bus::Bus,
    emulation::{Catalog, ScanStatus},
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

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
/// `internal`.
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

/// Polls `rom_scan_jobs` until the scan leaves `running`.
async fn settled(emulation: &EmulationService, id: &str) -> ScanJob {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let job = rom_scan_jobs(emulation)
            .into_iter()
            .find(|job| job.id == id)
            .expect("the scan is retained");
        if job.status != ScanStatus::Running {
            return job;
        }
        assert!(Instant::now() < deadline, "the scan didn't finish");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn scanners_scan_review_and_import() {
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

    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let service = EmulationService::new(
        store.clone(),
        bus.clone(),
        Catalog::load([("platforms.toml", PLATFORMS)]).unwrap(),
    );
    service
        .save_emulator(
            serde_json::from_value(json!({
                "id": "emu", "name": "Emu",
                "install": {"type": "folder", "dir": emu, "executable": "emu"},
                "profiles": [{"type": "custom", "id": "psx", "name": "PSX", "args": ["{RomPath}"],
                    "platforms": ["sony_playstation"], "extensions": ["chd"], "env": [],
                    "tracking": {"type": "default"}, "hooks": {}}],
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let sink = Sink::default();
    crate::events::bridge(Events::new(sink.clone()), &bus);

    // A new scanner gets its id from the backend.
    let scanner = rom_scanner_save(
        &service,
        serde_json::from_value(json!({
            "id": "", "name": "PS1", "folder": roms, "platform": null,
            "profile": {"emulatorId": "emu", "profileId": "psx"},
            "launchWith": {"type": "platform_default"},
            "recursive": true, "scanArchives": false, "skipCloudPlaceholders": true,
            "exclusions": [], "inLibraryUpdate": true,
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let id = scanner.id.clone();
    assert!(!id.is_empty());
    let mut invalid = scanner.clone();
    invalid.exclusions = vec!["?(".into()];
    assert!(
        rom_scanner_save(&service, invalid).await.is_err(),
        "an invalid exclusion regex is refused"
    );

    // A manual scan stops at the review list.
    let job = rom_scan_start(&service, &id, ScanMode::Review)
        .await
        .unwrap();
    let review = settled(&service, &job.id).await;
    assert_eq!(review.status, ScanStatus::Review);
    assert_eq!(review.review.len(), 3);
    let choices: Vec<ReviewChoice> = review
        .review
        .iter()
        .map(|game| ReviewChoice {
            roms: game.roms.iter().map(|rom| rom.path.clone()).collect(),
            include: game.title != "Demo",
            title: game.title.clone(),
            platform: game.platform.clone(),
            regions: game.regions.clone(),
        })
        .collect();
    let done = rom_scan_submit(&service, &review.id, review.revision, choices)
        .await
        .unwrap();
    assert_eq!(done.status, ScanStatus::Done);
    assert_eq!(done.summary.as_ref().unwrap().added, 2);

    let listed = rom_scanners_list(&service, &store).await.unwrap();
    assert_eq!(listed[0].games, 2);
    assert_eq!(listed[0].platforms, ["sony_playstation"]);
    assert_eq!(listed[0].scanner.exclusions, ["Demo (USA).chd"]);
    assert!(listed[0].last_scan_ms.is_some());
    assert_eq!(listed[0].flatpak_hint, None);

    // Scan all agrees with the review.
    let all = rom_scan_all(&service).await.unwrap();
    assert_eq!(all[0].added, 0);
    assert_eq!(all[0].games.len(), 2);
    assert_eq!(rom_scan_jobs(&service)[0].id, review.id);

    // #135: a global exclusion drops Tekken from the next scan.
    assert!(rom_exclusions(&service).await.unwrap().is_empty());
    let set = rom_exclusions_set(&service, vec![">Tekken*".into(), " ".into()])
        .await
        .unwrap();
    assert_eq!(set, [">Tekken*"]);
    assert!(
        rom_exclusions_set(&service, vec!["?(".into()])
            .await
            .is_err()
    );
    assert_eq!(rom_exclusions(&service).await.unwrap(), set);
    let all = rom_scan_all(&service).await.unwrap();
    assert_eq!(all[0].missing, 1);
    assert_eq!(all[0].games.len(), 1);

    rom_scanner_delete(&service, &id).await.unwrap();
    let gone = rom_scanner_delete(&service, &id).await.unwrap_err();
    assert_report(&gone, ErrorCode::Conflict, &[id.as_str()]);
    let gone = rom_scan_cancel(&service, "scan-1").unwrap_err();
    assert_report(&gone, ErrorCode::Conflict, &["scan-1"]);
    let gone = rom_scan_start(&service, "scanner-1", ScanMode::Review)
        .await
        .unwrap_err();
    assert_report(&gone, ErrorCode::Invalid, &["scanner-1"]);
    assert!(
        rom_scanners_list(&service, &store)
            .await
            .unwrap()
            .is_empty()
    );

    let statuses = || -> Vec<ScanStatus> {
        sink.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::EmulationScanChanged(changed) => Some(changed.0.status),
                _ => None,
            })
            .collect()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !statuses().contains(&ScanStatus::Done) {
        assert!(Instant::now() < deadline, "no emulationScanChanged events");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let statuses = statuses();
    assert_eq!(statuses.first(), Some(&ScanStatus::Running));
    assert!(statuses.contains(&ScanStatus::Review));
}
