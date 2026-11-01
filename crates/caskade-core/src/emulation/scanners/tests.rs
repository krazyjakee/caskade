use super::*;
use crate::{
    bus::Bus,
    emulation::{
        Catalog, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, ProfileHooks,
        ProfileRef, Tracking,
    },
    model::{InstallLocation, PlaySession},
};
use std::{fs, path::PathBuf, time::Duration};

const PLATFORMS: &str = r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd", "bin", "m3u"]

[[platform]]
id = "sony_playstation2"
name = "Sony PlayStation 2"
short = "PS2"
extensions = ["chd", "iso"]

[[region]]
id = "usa"
name = "USA"
codes = ["USA", "U"]
"#;

struct Fixture {
    temp: tempfile::TempDir,
    store: LibraryStore,
    scanners: ScannerService,
    scanner: RomScanner,
}

impl Fixture {
    fn roms(&self) -> PathBuf {
        self.scanner.folder.clone()
    }
    fn touch(&self, name: &str) {
        let path = self.roms().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"ROM").unwrap();
    }
    async fn entries(&self) -> Vec<GameEntry> {
        self.store
            .entries_for_scanner(&self.scanner.id)
            .await
            .unwrap()
    }
    async fn entry(&self, rom: &str) -> GameEntry {
        self.entries()
            .await
            .into_iter()
            .find(|entry| entry_paths(entry).iter().any(|path| path == rom))
            .unwrap_or_else(|| panic!("no entry with {rom}"))
    }
    async fn reviewed(&self) -> ScanJob {
        let job = self
            .scanners
            .start(&self.scanner.id, ScanMode::Review)
            .await
            .unwrap();
        for _ in 0..200 {
            let job = self.scanners.get(&job.id).unwrap();
            if job.status != ScanStatus::Running {
                assert_eq!(job.status, ScanStatus::Review, "{:?}", job.error);
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the review scan didn't finish");
    }
}

/// A portable PS1/PS2 emulator and one scanner over `ROMs/`.
async fn fixture(files: &[&str]) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let catalog = Catalog::load([("platforms.toml", PLATFORMS)]).unwrap();
    let emulation = EmulationService::new(store.clone(), Bus::default(), catalog);
    let dir = temp.path().join("emu");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("emu"), b"").unwrap();
    emulation
        .save_emulator(Emulator {
            id: "emu".into(),
            name: "Emu".into(),
            definition: None,
            install: EmulatorInstall::Folder {
                dir,
                executable: "emu".into(),
            },
            profiles: vec![EmulatorProfile::Custom {
                id: "psx".into(),
                name: "PSX".into(),
                executable: None,
                args: vec!["{RomPath}".into()],
                working_dir: None,
                platforms: vec!["sony_playstation".into()],
                extensions: vec!["chd".into(), "cue".into(), "bin".into()],
                env: vec![],
                tracking: Tracking::Default,
                hooks: ProfileHooks::default(),
            }],
        })
        .await
        .unwrap();
    let scanners = emulation.scanners();
    let scanner = scanners
        .save(RomScanner {
            id: String::new(),
            name: "PS1".into(),
            folder: temp.path().join("ROMs"),
            platform: None,
            profile: ProfileRef {
                emulator_id: "emu".into(),
                profile_id: "psx".into(),
            },
            launch_with: EmulatorTarget::PlatformDefault,
            recursive: true,
            scan_archives: false,
            skip_cloud_placeholders: true,
            exclusions: vec![],
            in_library_update: true,
        })
        .await
        .unwrap();
    let fixture = Fixture {
        temp,
        store,
        scanners,
        scanner,
    };
    fs::create_dir_all(fixture.roms()).unwrap();
    for file in files {
        fixture.touch(file);
    }
    fixture
}

#[tokio::test]
async fn moving_a_collection_keeps_games_sessions_and_launches() {
    let mut fx = fixture(&[
        "Final Fantasy VII (USA) (Disc 1).chd",
        "Final Fantasy VII (USA) (Disc 2).chd",
        "Crash Bandicoot (USA).chd",
    ])
    .await;
    let first = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!((first.added, first.updated, first.missing), (2, 0, 0));
    let ff7 = fx.entry("Final Fantasy VII (USA) (Disc 1).chd").await;
    let Some(LaunchSpec::Emulator { roms, .. }) = &ff7.launch else {
        panic!("not an emulated entry")
    };
    assert_eq!(roms.len(), 2);
    assert!(roms.iter().all(|rom| matches!(
        &rom.location,
        RomLocation::Scanner { scanner_id, path } if *scanner_id == fx.scanner.id && path.is_relative()
    )));
    let mut session = PlaySession::new(&ff7.game_id, &ff7.id, 1_000);
    session.ended_at_ms = Some(61_000);
    session.duration_ms = 60_000;
    fx.store.upsert_session(&session).await.unwrap();

    // The collection moves to another drive; only the scanner folder changes.
    let moved = fx.temp.path().join("Other drive/PS1");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(fx.roms(), &moved).unwrap();
    fx.scanner.folder = moved.clone();
    fx.scanner = fx.scanners.save(fx.scanner.clone()).await.unwrap();
    let after = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!((after.added, after.missing), (0, 0));

    let again = fx.entry("Final Fantasy VII (USA) (Disc 1).chd").await;
    assert_eq!((&again.id, &again.game_id), (&ff7.id, &ff7.game_id));
    assert_eq!(again.state, InstallState::Installed);
    assert_eq!(again.launch, ff7.launch, "relative ROMs don't change");
    assert_eq!(
        again.location,
        Some(InstallLocation::External { path: moved })
    );
    let sessions = fx.store.sessions_for_game(&ff7.game_id, 10).await.unwrap();
    assert_eq!(
        sessions.iter().map(|s| &s.id).collect::<Vec<_>>(),
        [&session.id]
    );
    assert_eq!(fx.entries().await.len(), 2);
}

#[tokio::test]
async fn removing_rom_files_marks_entries_not_installed() {
    let fx = fixture(&["Crash Bandicoot (USA).chd", "Spyro the Dragon (USA).chd"]).await;
    fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    let crash = fx.entry("Crash Bandicoot (USA).chd").await;
    fs::remove_file(fx.roms().join("Crash Bandicoot (USA).chd")).unwrap();

    let summary = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!((summary.added, summary.missing), (0, 1));
    let gone = fx.entry("Crash Bandicoot (USA).chd").await;
    assert_eq!(gone.id, crash.id);
    assert_eq!(gone.state, InstallState::Partial);
    assert!(fx.store.game(&crash.game_id).await.unwrap().is_some());

    // The file comes back: the same entry is installed again.
    fx.touch("Crash Bandicoot (USA).chd");
    let back = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!((back.added, back.updated), (0, 1));
    assert_eq!(
        fx.entry("Crash Bandicoot (USA).chd").await.state,
        InstallState::Installed
    );
}

#[tokio::test]
async fn review_untick_and_merge_choices_survive_the_next_automatic_scan() {
    let fx = fixture(&[
        "Metal Gear Solid (USA).chd",
        "Metal Gear Solid Integral VR Disc (USA).chd",
        "Spyro the Dragon (USA).cue",
        "Spyro the Dragon (USA).bin",
        "Tekken 3 (USA).chd",
    ])
    .await;
    fs::write(
        fx.roms().join("Spyro the Dragon (USA).cue"),
        "FILE \"Spyro the Dragon (USA).bin\" BINARY\n  TRACK 01 MODE2/2352\n",
    )
    .unwrap();
    let job = fx.reviewed().await;
    let titles: Vec<&str> = job.review.iter().map(|game| game.title.as_str()).collect();
    assert_eq!(titles.len(), 4, "{titles:?}");
    let spyro = job
        .review
        .iter()
        .find(|game| game.title.starts_with("Spyro"))
        .unwrap();
    assert_eq!(spyro.files.len(), 2, "the cue claims its bin");

    let choice = |game: &ReviewGame, include: bool| ReviewChoice {
        roms: game.roms.iter().map(|rom| rom.path.clone()).collect(),
        include,
        title: game.title.clone(),
        platform: game.platform.clone(),
        regions: game.regions.clone(),
    };
    let by_title = |title: &str| job.review.iter().find(|game| game.title == title).unwrap();
    let mgs = by_title("Metal Gear Solid");
    let vr = by_title("Metal Gear Solid Integral VR Disc");
    let mut merged = choice(mgs, true);
    merged
        .roms
        .extend(vr.roms.iter().map(|rom| rom.path.clone()));
    merged.title = "Metal Gear Solid: Integral".into();
    let choices = vec![
        merged,
        choice(spyro, false),
        choice(by_title("Tekken 3"), true),
    ];

    let done = fx
        .scanners
        .submit(&job.id, job.revision, choices)
        .await
        .unwrap();
    assert_eq!(done.status, ScanStatus::Done, "{:?}", done.error);
    assert_eq!(done.summary.as_ref().unwrap().added, 2);
    let scanner = fx.store.get_scanner(&fx.scanner.id).await.unwrap().unwrap();
    assert!(
        scanner
            .exclusions
            .contains(&"Spyro the Dragon (USA).cue".into())
    );
    assert!(
        scanner
            .exclusions
            .contains(&"Spyro the Dragon (USA).bin".into())
    );

    // The next automatic scan finds the same two games and changes nothing.
    let next = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!((next.added, next.updated, next.missing), (0, 0, 0));
    assert_eq!(next.games.len(), 2);
    let entries = fx.entries().await;
    assert_eq!(entries.len(), 2);
    let mgs = fx.entry("Metal Gear Solid (USA).chd").await;
    assert_eq!(
        entry_paths(&mgs),
        [
            "Metal Gear Solid (USA).chd",
            "Metal Gear Solid Integral VR Disc (USA).chd"
        ]
    );
    let game = fx.store.game(&mgs.game_id).await.unwrap().unwrap();
    assert_eq!(game.title, "Metal Gear Solid: Integral");
    assert!(entries.iter().all(|entry| {
        entry_paths(entry)
            .iter()
            .all(|path| !path.starts_with("Spyro"))
    }));

    // A later review shows the stored choices: one merged game, no Spyro.
    let again = fx.reviewed().await;
    assert_eq!(again.review.len(), 2);
    assert!(again.review.iter().all(|game| game.game_id.is_some()));
}

#[tokio::test]
async fn a_review_split_keeps_discs_apart_and_a_stale_submit_is_refused() {
    let fx = fixture(&["Game (USA) (Disc 1).chd", "Game (USA) (Disc 2).chd"]).await;
    let job = fx.reviewed().await;
    assert_eq!(job.review.len(), 1);
    let game = &job.review[0];
    let split = game
        .roms
        .iter()
        .map(|rom| ReviewChoice {
            roms: vec![rom.path.clone()],
            include: true,
            title: game.title.clone(),
            platform: game.platform.clone(),
            regions: game.regions.clone(),
        })
        .collect::<Vec<_>>();
    let stale = fx
        .scanners
        .submit(&job.id, job.revision + 1, split.clone())
        .await;
    assert!(stale.is_err());
    fx.scanners
        .submit(&job.id, job.revision, split)
        .await
        .unwrap();
    assert_eq!(fx.entries().await.len(), 2);
    let next = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!((next.added, next.updated, next.missing), (0, 0, 0));
}

#[tokio::test]
async fn scanners_are_found_by_platform_and_list_their_state() {
    let fx = fixture(&["Tekken 3 (USA).chd"]).await;
    let found = fx.scanners.for_platform("sony_playstation").await.unwrap();
    assert_eq!(
        found.iter().map(|s| &s.id).collect::<Vec<_>>(),
        [&fx.scanner.id]
    );
    assert!(
        fx.scanners
            .for_platform("sony_playstation2")
            .await
            .unwrap()
            .is_empty()
    );

    fx.scanners.scan_all().await.unwrap();
    let views = fx.scanners.list(&|_, _| true).await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].games, 1);
    assert!(views[0].last_scan_ms.is_some());
    assert_eq!(views[0].platforms, ["sony_playstation"]);
    assert_eq!(views[0].flatpak_hint, None);
}

#[tokio::test]
async fn a_platform_the_profile_doesnt_run_is_refused() {
    let fx = fixture(&[]).await;
    let mut scanner = fx.scanner.clone();
    scanner.platform = Some("sony_playstation2".into());
    assert!(fx.scanners.save(scanner).await.is_err());
}

#[tokio::test]
async fn deleting_a_scanner_keeps_its_games_not_installed() {
    let fx = fixture(&["Tekken 3 (USA).chd"]).await;
    fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    let entry = fx.entry("Tekken 3 (USA).chd").await;
    assert!(fx.scanners.delete(&fx.scanner.id).await.unwrap());
    let kept = fx.store.get_entry(&entry.id).await.unwrap().unwrap();
    assert_eq!(kept.state, InstallState::Partial);
}

#[tokio::test]
async fn a_folder_the_flatpak_emulator_cant_see_gets_a_hint() {
    let fx = fixture(&[]).await;
    let emulation = EmulationService::new(
        fx.store.clone(),
        Bus::default(),
        Catalog::load([("platforms.toml", PLATFORMS)]).unwrap(),
    );
    emulation
        .save_emulator(Emulator {
            id: "flat".into(),
            name: "Flat".into(),
            definition: None,
            install: EmulatorInstall::Flatpak {
                app_id: "org.example.Flat".into(),
            },
            profiles: vec![EmulatorProfile::Custom {
                id: "flat-psx".into(),
                name: "PSX".into(),
                executable: None,
                args: vec!["{RomPath}".into()],
                working_dir: None,
                platforms: vec!["sony_playstation".into()],
                extensions: vec!["chd".into()],
                env: vec![],
                tracking: Tracking::Default,
                hooks: ProfileHooks::default(),
            }],
        })
        .await
        .unwrap();
    let mut scanner = fx.scanner.clone();
    scanner.profile.emulator_id = "flat".into();
    scanner.profile.profile_id = "flat-psx".into();
    fx.scanners.save(scanner).await.unwrap();
    let asked = std::sync::Mutex::new(Vec::new());
    let hidden = fx
        .scanners
        .list(&|app_id, dir| {
            asked
                .lock()
                .unwrap()
                .push((app_id.to_owned(), dir.to_path_buf()));
            false
        })
        .await
        .unwrap();
    assert!(
        hidden[0]
            .flatpak_hint
            .as_deref()
            .unwrap()
            .contains("Flat runs as a Flatpak")
    );
    assert_eq!(
        asked.into_inner().unwrap(),
        [("org.example.Flat".to_owned(), fx.roms())]
    );
    let seen = fx.scanners.list(&|_, _| true).await.unwrap();
    assert_eq!(seen[0].flatpak_hint, None);
}

/// #135: global exclusions (§6.3) apply to every scanner from the next scan.
#[tokio::test]
async fn global_exclusions_apply_from_the_next_scan() {
    let fx = fixture(&[
        "Crash Bandicoot (USA).chd",
        "Spyro the Dragon (USA).chd",
        "bios/scph1001.bin",
    ])
    .await;
    assert!(fx.scanners.global_exclusions().await.unwrap().is_empty());
    let first = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!(first.added, 3);

    let saved = fx
        .scanners
        .set_global_exclusions(vec![
            " bios/ ".into(),
            ">Spyro*".into(),
            "bios/".into(),
            String::new(),
        ])
        .await
        .unwrap();
    assert_eq!(saved, ["bios/", ">Spyro*"]);
    assert_eq!(fx.scanners.global_exclusions().await.unwrap(), saved);
    let next = fx.scanners.scan_import(&fx.scanner.id).await.unwrap();
    assert_eq!(next.missing, 2);
    let installed: Vec<_> = fx
        .entries()
        .await
        .into_iter()
        .filter(|entry| entry.state == InstallState::Installed)
        .collect();
    assert_eq!(installed.len(), 1);
    assert_eq!(entry_paths(&installed[0]), ["Crash Bandicoot (USA).chd"]);

    // A pattern that doesn't compile is refused and nothing changes.
    assert!(
        fx.scanners
            .set_global_exclusions(vec!["?(".into()])
            .await
            .is_err()
    );
    assert_eq!(fx.scanners.global_exclusions().await.unwrap(), saved);
}
