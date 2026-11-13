use caskade_core::emulation::{
    EmulatorTarget, Platform, PlatformIds, ProfileRef, Region, RomLocation, RomScanner,
    scan::{
        self, ArchiveMember, CancellationToken, Exclusions, RomFile, ScanError, ScanLimits,
        ScanRequest,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn scanner(folder: &Path) -> RomScanner {
    RomScanner {
        id: "fixture-scanner".into(),
        name: "Authored ROMs".into(),
        folder: folder.into(),
        platform: Some("sony_playstation".into()),
        profile: ProfileRef {
            emulator_id: "fixture".into(),
            profile_id: "discs".into(),
        },
        launch_with: EmulatorTarget::PlatformDefault,
        recursive: true,
        scan_archives: false,
        skip_cloud_placeholders: true,
        exclusions: vec![],
        in_library_update: true,
    }
}
fn platforms() -> Vec<Platform> {
    vec![
        Platform {
            id: "sony_playstation".into(),
            name: "PlayStation".into(),
            short: "PS1".into(),
            extensions: ["cue", "bin", "m3u", "gdi", "chd"]
                .map(String::from)
                .to_vec(),
            ids: PlatformIds::default(),
            bios: vec![],
            recommended: vec![],
        },
        Platform {
            id: "nintendo_snes".into(),
            name: "SNES".into(),
            short: "SNES".into(),
            extensions: vec!["sfc".into()],
            ids: PlatformIds::default(),
            bios: vec![],
            recommended: vec![],
        },
    ]
}
fn regions() -> Vec<Region> {
    vec![
        Region {
            id: "us".into(),
            name: "USA".into(),
            codes: vec!["USA".into(), "U".into()],
        },
        Region {
            id: "eu".into(),
            name: "Europe".into(),
            codes: vec!["Europe".into(), "E".into()],
        },
        Region {
            id: "jp".into(),
            name: "Japan".into(),
            codes: vec!["Japan".into(), "J".into()],
        },
    ]
}
fn file(path: &str) -> RomFile {
    RomFile {
        path: path.into(),
        size: 4,
        ..Default::default()
    }
}
fn playlist(path: &str, text: &str) -> RomFile {
    RomFile {
        playlist: Some(text.into()),
        ..file(path)
    }
}
fn request<'a>(
    scanner: &'a RomScanner,
    ext: &'a [String],
    platforms: &'a [Platform],
    regions: &'a [Region],
) -> ScanRequest<'a> {
    ScanRequest {
        scanner,
        extensions: ext,
        platforms,
        regions,
        global_exclusions: &[],
        limits: ScanLimits::default(),
    }
}
#[test]
fn no_intro_and_redump_names_keep_properties_region_and_disc_order() {
    let regions = regions();
    let names = [
        "Final Fantasy VII (USA) (Disc 2 of 3).cue",
        "Final Fantasy VII (USA) (Disc 1).cue",
        "Legend of Zelda, The (USA) (Rev 1).sfc",
        "Sonic_the_Hedgehog™ (Europe) [!].md",
        "Example® (Side B) (Japan).fds",
        "No suffix",
        "Overflow (Disc 9999999999999999999999999).bin",
        "Game (USA, Europe) [b1].sfc",
    ];
    let output: Vec<_> = names
        .iter()
        .map(|s| scan::parse_name(Path::new(s), &regions))
        .collect();
    assert_eq!(output[0].part.as_ref().unwrap().number, 2);
    assert_eq!(output[2].title, "The Legend of Zelda");
    assert!(output[6].part.is_none());
    assert_eq!(
        scan::parse_name(Path::new("Game (Disc2of3, USA).chd"), &regions)
            .part
            .unwrap()
            .number,
        2
    );
    assert_eq!(
        scan::parse_name(
            Path::new("Legend of Zelda, The - A Link to the Past.sfc"),
            &regions
        )
        .title,
        "The Legend of Zelda - A Link to the Past"
    );
    insta::assert_json_snapshot!("name_conventions", output);
}
#[test]
fn m3u_claims_cues_and_tracks_gdi_claims_tracks_and_discs_are_ordered() {
    let scanner = scanner(Path::new("/authored"));
    let platforms = platforms();
    let regions = regions();
    let ext = ["m3u", "cue", "bin", "gdi", "chd"].map(String::from);
    let files = vec![
        playlist(
            "Final Fantasy VII (USA).m3u",
            "#EXTM3U\nFinal Fantasy VII (USA) (Disc 2).cue\nFinal Fantasy VII (USA) (Disc 1).cue\n",
        ),
        playlist(
            "Final Fantasy VII (USA) (Disc 1).cue",
            "FILE \"track 1.bin\" BINARY\n",
        ),
        playlist(
            "Final Fantasy VII (USA) (Disc 2).cue",
            "FILE \"track 2.bin\" BINARY\n",
        ),
        file("track 1.bin"),
        file("track 2.bin"),
        playlist(
            "Dreamcast/Example.gdi",
            "2\n1 0 4 2352 \"track01.bin\" 0\n2 45000 0 2352 track02.raw 0\n",
        ),
        file("Dreamcast/track01.bin"),
        file("Dreamcast/track02.raw"),
        file("Separate/Game (Disc 2).CHD"),
        file("Separate/Game (Disc 1).chd"),
        file("Other/Game (Disc 1).chd"),
    ];
    let report = scan::scan_files(
        &files,
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(report.games.len(), 4);
    let playlist_game = report
        .games
        .iter()
        .find(|g| g.title == "Final Fantasy VII")
        .unwrap();
    assert_eq!(playlist_game.roms.len(), 1);
    assert_eq!(playlist_game.files.len(), 5);
    let discs = report.games.iter().find(|g| g.roms.len() == 2).unwrap();
    assert_eq!(discs.roms[0].name.as_deref(), Some("Disc 1"));
    assert_eq!(discs.roms[1].name.as_deref(), Some("Disc 2"));
    insta::assert_json_snapshot!("playlist_absorption", report);
}
#[test]
fn all_exclusion_modes_apply_per_scanner_and_globally() {
    let mut scanner = scanner(Path::new("/authored"));
    scanner.exclusions = vec!["bios/".into(), ">*.txt".into()];
    let global = ["?^beta/.*".into(), ">?\\(Beta\\)".into()];
    let ext = ["bin", "txt"].map(String::from);
    let platforms = platforms();
    let regions = regions();
    let mut req = request(&scanner, &ext, &platforms, &regions);
    req.global_exclusions = &global;
    let files = [
        "bios/scph1001.bin",
        "nested/bios/Game.bin",
        "notes.txt",
        "nested/readme.TXT",
        "beta/Game.bin",
        "nested/beta/Game.bin",
        "Game (Beta).bin",
        "nested/Game (Beta).bin",
        "Game (USA).bin",
    ]
    .map(file);
    let report = scan::scan_files(&files, &req, &CancellationToken::new()).unwrap();
    assert_eq!(report.games.len(), 3);
    insta::assert_json_snapshot!("all_exclusion_modes", report);
    for invalid in ["?[", ">[", "../outside"] {
        assert!(Exclusions::compile([invalid]).is_err());
    }
}
#[test]
fn invalid_missing_and_cyclic_playlists_do_not_absorb_playable_files() {
    let scanner = scanner(Path::new("/authored"));
    let platforms = platforms();
    let regions = regions();
    let ext = ["m3u", "cue", "bin"].map(String::from);
    let files = vec![
        playlist("Cycle A.m3u", "Cycle B.m3u"),
        playlist("Cycle B.m3u", "Cycle A.m3u"),
        playlist("Missing.cue", "FILE missing.bin BINARY"),
        playlist("Escape.m3u", "../outside.bin"),
        file("Valid.bin"),
    ];
    let report = scan::scan_files(
        &files,
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].title, "Valid");
    assert_eq!(report.warnings.len(), 4);
    insta::assert_json_snapshot!("invalid_playlists", report);
    assert_eq!(
        scan::playlist_references(Path::new("Nested/Game.m3u"), "../Disc.chd").unwrap(),
        [PathBuf::from("Disc.chd")]
    );
}
#[test]
fn case_insensitive_references_hidden_placeholders_and_extensionless_files() {
    let mut scanner = scanner(Path::new("/authored"));
    let platforms = platforms();
    let regions = regions();
    let ext = ["cue", "bin", "<none>"].map(String::from);
    let files = vec![
        playlist("Game.cue", "FILE \"TRACK.BIN\" BINARY"),
        file("track.bin"),
        file(".hidden.bin"),
        file(".hidden/also.bin"),
        RomFile {
            hidden: true,
            ..file("Windows hidden.bin")
        },
        RomFile {
            cloud_placeholder: true,
            ..file("Cloud.bin")
        },
        file("NoExtension"),
    ];
    let req = request(&scanner, &ext, &platforms, &regions);
    let report = scan::scan_files(&files, &req, &CancellationToken::new()).unwrap();
    assert_eq!(report.games.len(), 2);
    assert!(report.warnings.is_empty());
    scanner.skip_cloud_placeholders = false;
    let report = scan::scan_files(
        &files,
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.games.len(), 3);
}
#[test]
fn archive_metadata_matches_only_when_opted_in_and_archive_is_not_already_supported() {
    let mut scanner = scanner(Path::new("/authored"));
    scanner.platform = Some("nintendo_snes".into());
    let files = [
        RomFile {
            archive_entries: vec![ArchiveMember {
                path: "Game (Japan).sfc".into(),
                size: 42,
            }],
            ..file("Game.zip")
        },
        file("Unsupported.7z"),
    ];
    let platforms = platforms();
    let regions = regions();
    let ext = ["sfc".into()];
    assert!(
        scan::scan_files(
            &files,
            &request(&scanner, &ext, &platforms, &regions),
            &CancellationToken::new()
        )
        .unwrap()
        .games
        .is_empty()
    );
    scanner.scan_archives = true;
    let report = scan::scan_files(
        &files,
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.games.len(), 1);
    assert!(
        matches!(&report.games[0].roms[0].location,RomLocation::Scanner{path,..} if path==Path::new("Game.zip"))
    );
    let ext = ["zip".into()];
    scanner.scan_archives = false;
    assert_eq!(
        scan::scan_files(
            &files,
            &request(&scanner, &ext, &platforms, &regions),
            &CancellationToken::new()
        )
        .unwrap()
        .games
        .len(),
        1
    );
}
#[test]
fn enumeration_is_bounded_cancellable_nonrecursive_and_does_not_follow_symlinks() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("Nested")).unwrap();
    fs::create_dir(t.path().join(".hidden")).unwrap();
    fs::write(t.path().join("Game.bin"), b"").unwrap();
    fs::write(t.path().join("Nested/Game.bin"), b"").unwrap();
    fs::write(t.path().join(".hidden/Game.bin"), b"").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(t.path().parent().unwrap(), t.path().join("escape")).unwrap();
    let mut scanner = scanner(t.path());
    scanner.recursive = false;
    let platforms = platforms();
    let regions = regions();
    let ext = ["bin".into()];
    let report = scan::scan_folder(
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.games.len(), 1);
    scanner.recursive = true;
    let req = request(&scanner, &ext, &platforms, &regions);
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        scan::scan_folder(&req, &token),
        Err(ScanError::Cancelled)
    ));
    let mut req = request(&scanner, &ext, &platforms, &regions);
    req.limits.max_files = 1;
    assert!(matches!(
        scan::scan_folder(&req, &CancellationToken::new()),
        Err(ScanError::Limit(_))
    ));
    let token = CancellationToken::new();
    let mut updates = Vec::new();
    scan::scan_folder_with_progress(
        &request(&scanner, &ext, &platforms, &regions),
        &token,
        &mut |p| {
            updates.push(p);
            token.cancel();
        },
    )
    .unwrap_err();
    assert!(!updates.is_empty());
}
#[test]
fn one_thousand_files_are_scanned_with_correct_multidisc_groups_under_two_seconds() {
    let fixture = caskade_emulation_fixtures::RomFixture::thousand().unwrap();
    let tree = &fixture.tree;
    let mut scanner = scanner(&tree.root);
    scanner.platform = Some("fixture".into());
    scanner.exclusions = caskade_emulation_fixtures::SCANNER_EXCLUSIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let ext = [
        "bin", "cue", "m3u", "gdi", "chd", "sfc", "smc", "zip", "7z", "<none>",
    ]
    .map(String::from);
    let platforms = vec![Platform {
        id: "fixture".into(),
        name: "Synthetic test platform".into(),
        short: "Test".into(),
        extensions: ext.to_vec(),
        ids: PlatformIds::default(),
        bios: vec![],
        recommended: vec![],
    }];
    let regions = regions();
    let req = request(&scanner, &ext, &platforms, &regions);
    let start = Instant::now();
    let report = scan::scan_folder(&req, &CancellationToken::new()).unwrap();
    let elapsed = start.elapsed();
    let mut actual: Vec<_> = report
        .games
        .iter()
        .map(|game| {
            let roms: Vec<_> = game
                .roms
                .iter()
                .map(|rom| match &rom.location {
                    RomLocation::Scanner { path, .. } => path.clone(),
                    _ => panic!("scanner paths must remain relative"),
                })
                .collect();
            (game.title.clone(), roms)
        })
        .collect();
    actual.sort();
    let expected: Vec<_> = tree
        .expected_games
        .iter()
        .map(|game| (game.title.clone(), game.roms.clone()))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(tree.files.len(), 1000);
    assert_eq!(report.games.len(), 963);
    assert_eq!(
        report
            .games
            .iter()
            .filter(|game| game.roms.len() == 2)
            .count(),
        20
    );
    assert_eq!(
        report.games.iter().map(|g| g.roms.len()).sum::<usize>(),
        983
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(
        elapsed < Duration::from_secs(2),
        "1000-file scan took {elapsed:?}"
    );
    eprintln!("shared1000files →963games,20multidiscgroups in {elapsed:?}");
}

#[test]
fn real_zip_and_seven_zip_directories_are_inspected_without_extracting_rom_bytes() {
    let fixture = caskade_emulation_fixtures::RomFixture::thousand().unwrap();
    let mut scanner = scanner(&fixture.tree.root);
    scanner.platform = Some("nintendo_snes".into());
    scanner.scan_archives = true;
    let ext = ["sfc", "smc"].map(String::from);
    let platforms = platforms();
    let regions = regions();
    let report = scan::scan_folder(
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    let archives: Vec<_> = report
        .games
        .iter()
        .flat_map(|g| &g.roms)
        .filter_map(|r| match &r.location {
            RomLocation::Scanner { path, .. }
                if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("zip" | "7z")
                ) =>
            {
                Some(path.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        archives,
        [
            PathBuf::from("Archives/Fixture Seven (Japan).7z"),
            PathBuf::from("Archives/Fixture Zip (USA).zip")
        ]
    );
    assert_eq!(report.games.len(), 5);
    assert!(report.warnings.is_empty());
}

#[test]
fn accepted_m3u_absorbs_tracks_through_an_unsupported_cue_but_unmatched_cues_do_not_hide_roms() {
    let scanner = scanner(Path::new("/authored"));
    let platforms = platforms();
    let regions = regions();
    let ext = ["m3u", "bin"].map(String::from);
    let files = vec![
        playlist("Set.m3u", "disc.cue"),
        playlist("disc.cue", "FILE track.bin BINARY"),
        file("track.bin"),
        playlist("unsupported.cue", "FILE independent.bin BINARY"),
        file("independent.bin"),
    ];
    let report = scan::scan_files(
        &files,
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.games.len(), 2);
    assert!(report.warnings.is_empty());
    let set = report.games.iter().find(|g| g.title == "Set").unwrap();
    assert_eq!(set.files.len(), 3);
    assert!(report.games.iter().any(|g| g.title == "independent"));
}

#[test]
fn playlist_depth_failures_do_not_poison_shallower_independent_members() {
    let scanner = scanner(Path::new("/authored"));
    let platforms = platforms();
    let regions = regions();
    let ext = ["m3u", "bin"].map(String::from);
    let mut files = Vec::new();
    for n in 0..40 {
        files.push(playlist(
            &format!("Chain{n:02}.m3u"),
            &format!("Chain{:02}.m3u", n + 1),
        ));
    }
    files.push(playlist("Chain40.m3u", "Final.bin"));
    files.push(file("Final.bin"));
    let report = scan::scan_files(
        &files,
        &request(&scanner, &ext, &platforms, &regions),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].title, "Chain09");
    assert_eq!(report.games[0].files.len(), 33);
    assert_eq!(report.warnings.len(), 9);
    let mut req = request(&scanner, &ext, &platforms, &regions);
    req.limits.max_references = 1;
    assert!(matches!(
        scan::scan_files(&files, &req, &CancellationToken::new()),
        Err(ScanError::Limit(_))
    ));
}

#[test]
fn aggregate_metadata_limits_and_malformed_archive_warnings_are_explicit() {
    let mut scanner = scanner(Path::new("/authored"));
    let platforms = platforms();
    let regions = regions();
    let ext = ["m3u", "bin"].map(String::from);
    let files = vec![
        playlist("One.m3u", "Game.bin"),
        playlist("Two.m3u", "Game.bin"),
        file("Game.bin"),
    ];
    let mut req = request(&scanner, &ext, &platforms, &regions);
    req.limits.max_total_text_bytes = 10;
    assert!(matches!(
        scan::scan_files(&files, &req, &CancellationToken::new()),
        Err(ScanError::Limit("total playlist text"))
    ));
    let files = vec![RomFile {
        archive_entries: vec![
            ArchiveMember {
                path: "One.bin".into(),
                size: 1,
            },
            ArchiveMember {
                path: "Two.bin".into(),
                size: 1,
            },
        ],
        ..file("One.zip")
    }];
    let mut req = request(&scanner, &ext, &platforms, &regions);
    req.limits.max_total_archive_entries = 1;
    assert!(matches!(
        scan::scan_files(&files, &req, &CancellationToken::new()),
        Err(ScanError::Limit("total archive entries"))
    ));
    let temp = tempfile::tempdir().unwrap();
    scanner.folder = temp.path().into();
    scanner.scan_archives = true;
    fs::write(temp.path().join("Broken.zip"), b"PK\x03\x04truncated").unwrap();
    let req = request(&scanner, &ext, &platforms, &regions);
    let report = scan::scan_folder(&req, &CancellationToken::new()).unwrap();
    assert!(report.games.is_empty());
    assert_eq!(report.warnings.len(), 1);
    let mut req = request(&scanner, &ext, &platforms, &regions);
    req.limits.max_total_archive_read_bytes = 0;
    assert!(matches!(
        scan::scan_folder(&req, &CancellationToken::new()),
        Err(ScanError::Limit("total archive reads"))
    ));
}
