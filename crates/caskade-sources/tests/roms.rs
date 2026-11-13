//! The `roms` source imports ROM scanners during library updates (#119).
use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
    emulation::{
        EmulationService, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, ProfileHooks,
        ProfileRef, RomScanner, Tracking,
    },
    model::InstallState,
};
use caskade_emulation_fixtures::{RomFixture, SCANNER_EXCLUSIONS};
use caskade_sources::RomsSource;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

#[tokio::test]
async fn a_library_update_imports_the_thousand_file_tree() {
    let fixture = RomFixture::thousand().unwrap();
    let tree = &fixture.tree;
    let extensions: BTreeSet<String> = tree
        .files
        .iter()
        .map(|file| {
            file.extension()
                .and_then(|ext| ext.to_str())
                .map_or("<none>".into(), str::to_ascii_lowercase)
        })
        .collect();
    let emu = tempfile::tempdir().unwrap();
    std::fs::write(emu.path().join("emu"), b"").unwrap();

    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let emulation = EmulationService::new(
        store.clone(),
        bus.clone(),
        caskade_sources::emulation::catalog().unwrap(),
    );
    emulation
        .save_emulator(Emulator {
            id: "emu".into(),
            name: "Emu".into(),
            definition: None,
            install: EmulatorInstall::Folder {
                dir: emu.path().to_path_buf(),
                executable: "emu".into(),
            },
            profiles: vec![EmulatorProfile::Custom {
                id: "all".into(),
                name: "Everything".into(),
                executable: None,
                args: vec!["{RomPath}".into()],
                working_dir: None,
                platforms: vec!["nintendo_snes".into()],
                extensions: extensions.into_iter().collect(),
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
            name: "Collection".into(),
            folder: tree.root.clone(),
            platform: None,
            profile: ProfileRef {
                emulator_id: "emu".into(),
                profile_id: "all".into(),
            },
            launch_with: EmulatorTarget::PlatformDefault,
            recursive: true,
            scan_archives: false,
            skip_cloud_placeholders: true,
            exclusions: SCANNER_EXCLUSIONS.iter().map(|s| (*s).to_owned()).collect(),
            in_library_update: true,
        })
        .await
        .unwrap();

    let source = RomsSource::new(scanners.clone());
    assert_eq!(source.id(), caskade_core::emulation::ROMS_SOURCE.as_str());
    let mut events = bus.stream();
    let watch = source.watch(&bus);
    let started = Instant::now();
    bus.emit(Event::LibraryScanCompleted { sources: vec![] });
    let counts = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(Event::EmulationScanCompleted { scanners }) = events.recv().await {
                return scanners;
            }
        }
    })
    .await
    .expect("the library update ran the scanner");
    let elapsed = started.elapsed();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].scanner_id, scanner.id);
    assert_eq!(counts[0].added as usize, tree.expected_games.len());
    assert!(
        elapsed < Duration::from_secs(2),
        "1,000 files took {elapsed:?}"
    );

    let entries = store.entries_for_scanner(&scanner.id).await.unwrap();
    assert_eq!(entries.len(), tree.expected_games.len());
    assert!(entries.iter().all(|e| e.state == InstallState::Installed));
    let discs: usize = entries
        .iter()
        .map(|entry| match &entry.launch {
            Some(caskade_core::model::LaunchSpec::Emulator { roms, .. }) => roms.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(discs, tree.expected_discs());

    // A second update finds nothing new.
    let again = source.update().await.unwrap();
    assert_eq!((again[0].added, again[0].missing), (0, 0));
    watch.abort();
}
