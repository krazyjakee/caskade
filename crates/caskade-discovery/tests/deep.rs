#![cfg(feature = "deep")]
//! Original, generated synthetic machine trees; payload headers are inert.
use caskade_discovery::{
    deep::{self, Options, Report, ScanKind},
    *,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Machine {
    temp: tempfile::TempDir,
}
impl Machine {
    fn new() -> Self {
        let temp = crate::fixtures::tempdir();
        for path in ["home/fixture", "media/usb"] {
            fs::create_dir_all(temp.path().join("root").join(path)).unwrap();
        }
        fs::write(
            temp.path().join("machine.toml"),
            r#"schema_version=1
os="linux"
[env]
HOME="/home/fixture"
[[volumes]]
path="/"
kind="fixed"
filesystem="ext4"
[[volumes]]
path="/media/usb"
kind="removable"
filesystem="ext4"
"#,
        )
        .unwrap();
        Self { temp }
    }
    fn env(&self) -> HostEnv {
        HostEnv::fixture(self.temp.path()).unwrap()
    }
    fn path(&self, p: &str) -> PathBuf {
        self.env().resolve_path(Path::new(p)).unwrap()
    }
    fn write(&self, p: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn elf(&self, p: &str) {
        let mut bytes = [0u8; 64];
        bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
        bytes[16] = 2;
        bytes[18] = 62;
        bytes[24] = 1;
        let path = self.write(p, bytes);
        executable(&path);
    }
    fn pe(&self, p: &str) {
        let mut bytes = [0u8; 70];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60] = 64;
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        self.write(p, bytes);
    }
    fn unity(&self, p: &str, name: &str) {
        self.elf(&format!("{p}/{name}"));
        self.write(
            &format!("{p}/UnityPlayer.so"),
            b"original synthetic engine marker",
        );
        self.write(
            &format!("{p}/{name}_Data/globalgamemanagers"),
            b"original synthetic engine marker",
        );
    }
    fn ctx(&self, deep: DeepScans) -> ScanCtx {
        ScanCtx::with_config(
            Cancel::new(),
            Arc::new(Config {
                deep,
                ..Default::default()
            }),
        )
    }
    fn scan(&self, deep: DeepScans) -> (Report, Vec<String>) {
        let ctx = self.ctx(deep);
        let report = deep::scan_with_options(
            &self.env(),
            &ctx,
            &Options {
                budget_ms: 10_000,
                ..Default::default()
            },
            &|_| {},
        )
        .unwrap();
        (report, ctx.take_warnings())
    }
}
fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}
#[test]
fn every_scan_is_opt_in_and_default_scan_does_not_walk_home_or_drives() {
    let m = Machine::new();
    m.unity("/home/fixture/Games/Example", "Example");
    let (report, w) = m.scan(DeepScans::default());
    assert!(report.games.is_empty());
    assert_eq!(report.visited, 0);
    assert!(w.is_empty());
}
#[test]
fn custom_folder_classification_keeps_games_separate_and_never_runs_or_writes() {
    let m = Machine::new();
    m.unity("/custom/Games/First", "First");
    m.unity("/custom/Games/Second", "Second");
    m.elf("/custom/Games/Utility/editor");
    m.write("/custom/Games/Utility/readme.txt", b"ordinary utility");
    let before = fs::read(m.path("/custom/Games/First/First")).unwrap();
    let (report, w) = m.scan(DeepScans {
        folders: vec![PathBuf::from("/custom/Games")],
        ..Default::default()
    });
    assert_eq!(report.games.len(), 2, "{w:?}");
    for game in &report.games {
        assert_eq!(game.confidence, Confidence::Likely);
        assert_eq!(game.launch.len(), 1);
        assert!(
            matches!(&game.launch[0],LaunchTarget::Exec{program,..} if program.starts_with(m.path("/custom/Games")))
        );
        assert!(
            game.evidence
                .iter()
                .any(|e| matches!(e,Evidence::Rule{rule,..} if rule=="fingerprint.unity-linux"))
        );
    }
    assert_eq!(
        before,
        fs::read(m.path("/custom/Games/First/First")).unwrap()
    );
    assert!(!m.path("/custom/Games/First/.caskade").exists());
}
#[test]
fn unlisted_steam_library_on_usb_is_found_without_steam_installation() {
    let m = Machine::new();
    m.write("/media/usb/SteamLibrary/steamapps/appmanifest_620.acf",br#""AppState" { "appid" "620" "name" "Portal 2" "installdir" "Portal 2" "StateFlags" "4" }"#);
    m.write("/media/usb/SteamLibrary/steamapps/appmanifest_228980.acf",br#""AppState" { "appid" "228980" "name" "Steamworks Common Redistributables" "installdir" "Steamworks Shared" "StateFlags" "4" }"#);
    m.elf("/media/usb/SteamLibrary/steamapps/common/Portal 2/portal2_linux");
    let (report, w) = m.scan(DeepScans {
        external_drives: true,
        ..Default::default()
    });
    assert_eq!(report.games.len(), 1, "{w:?}");
    let game = &report.games[0];
    assert_eq!(game.title, "Portal 2");
    assert!(game.ids.contains(&ExternalId::Steam { app_id: 620 }));
    assert_eq!(
        game.launch,
        [LaunchTarget::Uri {
            uri: "steam://rungameid/620".into()
        }]
    );
    assert_eq!(
        game.install_dir,
        Some(m.path("/media/usb/SteamLibrary/steamapps/common/Portal 2"))
    );
    assert!(!m.path("/home/fixture/.local/share/Steam").exists());
}
#[test]
fn external_game_folders_and_caskade_root_handoff_are_distinct() {
    let m = Machine::new();
    m.unity("/media/usb/GOG Games/Adventure", "Adventure");
    m.unity("/media/usb/Epic Games/Other", "Other");
    m.unity("/media/usb/Heroic/Games/Third", "Third");
    m.unity("/media/usb/Lutris/Games/Fourth", "Fourth");
    m.write(
        "/media/usb/Caskade/.caskade/root.json",
        br#"{"schema_version":1,"root_id":"fixture-usb-root"}"#,
    );
    m.unity("/media/usb/Caskade/Managed", "Managed");
    let (report, w) = m.scan(DeepScans {
        external_drives: true,
        ..Default::default()
    });
    assert_eq!(report.games.len(), 4, "{w:?}");
    assert_eq!(
        report.caskade_roots,
        [deep::CaskadeRoot {
            path: m.path("/media/usb/Caskade"),
            root_id: "fixture-usb-root".into()
        }]
    );
    assert!(report.games.iter().all(|g| g.title != "Managed"));
}
#[test]
fn portable_appimage_is_a_single_file_game_not_the_applications_directory() {
    let m = Machine::new();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../caskade-installer/tests/fixtures/bin/appimage/Indie_Game-x86_64.AppImage");
    let bytes = fs::read(fixture).unwrap();
    for name in ["First.AppImage", "Second.AppImage"] {
        let p = m.write(&format!("/home/fixture/Applications/{name}"), &bytes);
        executable(&p);
    }
    m.elf("/home/fixture/Applications/Utility.AppImage");
    m.unity("/home/fixture/Games/Unpacked", "Unpacked");
    let (report, w) = m.scan(DeepScans {
        portable: true,
        ..Default::default()
    });
    assert_eq!(report.games.len(), 3, "{w:?}");
    let images: Vec<_> = report
        .games
        .iter()
        .filter(|g| g.title == "Indie Game")
        .collect();
    assert_eq!(images.len(), 2);
    assert!(images.iter().all(|g| g.install_dir.is_none()));
    let groups =
        caskade_discovery::merge::merge(&m.env(), report.games, &Default::default()).unwrap();
    assert_eq!(groups.len(), 3);
}
#[test]
fn full_disk_results_are_maybe_and_system_trees_are_excluded() {
    let m = Machine::new();
    m.unity("/home/fixture/MysteryGame", "MysteryGame");
    m.unity("/usr/lib/LooksLikeGame", "LooksLikeGame");
    m.unity("/proc/FakeGame", "FakeGame");
    let (report, w) = m.scan(DeepScans {
        full_disk: true,
        ..Default::default()
    });
    assert_eq!(report.games.len(), 1, "{w:?}");
    assert_eq!(report.games[0].title, "MysteryGame");
    assert_eq!(report.games[0].confidence, Confidence::Maybe);
}

#[test]
fn ambiguous_pe_targets_remain_review_candidates_and_installers_are_excluded() {
    let m = Machine::new();
    m.pe("/custom/Bundle/aa.exe");
    m.pe("/custom/Bundle/bb.exe");
    m.pe("/custom/Bundle/setup.exe");
    m.pe("/custom/Bundle/unins000.exe");
    m.write("/custom/Bundle/UnityPlayer.dll", b"engine marker");
    m.write(
        "/custom/Bundle/Bundle_Data/globalgamemanagers",
        b"engine marker",
    );
    let (report, w) = m.scan(DeepScans {
        folders: vec![PathBuf::from("/custom/Bundle")],
        ..Default::default()
    });
    assert_eq!(report.games.len(), 1, "{w:?}");
    let g = &report.games[0];
    assert_eq!(g.confidence, Confidence::Maybe);
    assert_eq!(g.launch.len(), 2);
    assert!(
        g.evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..} if rule=="deep.ambiguous_launch"))
    );
    assert!(g.launch.iter().all(|l|matches!(l,LaunchTarget::Exec{program,..} if !program.ends_with("setup.exe")&&!program.ends_with("unins000.exe"))));
    assert!(matches!(g.runtime, GameRuntime::Windows { .. }));
}
#[test]
fn depth_entry_and_injected_architecture_limits_are_enforced() {
    let m = Machine::new();
    m.unity("/custom/deep/one/two/Game", "Game");
    let ctx = m.ctx(DeepScans {
        folders: vec![PathBuf::from("/custom/deep")],
        ..Default::default()
    });
    let options = Options {
        max_depth: 1,
        budget_ms: 10_000,
        ..Default::default()
    };
    let report = deep::scan_with_options(&m.env(), &ctx, &options, &|_| {}).unwrap();
    assert!(report.games.is_empty());
    assert!(
        ctx.take_warnings()
            .iter()
            .any(|s| s.contains("depth budget"))
    );
    let ctx = m.ctx(DeepScans {
        folders: vec![PathBuf::from("/custom/deep")],
        ..Default::default()
    });
    let report = deep::scan_with_options(
        &m.env(),
        &ctx,
        &Options {
            max_entries: 2,
            budget_ms: 10_000,
            ..Default::default()
        },
        &|_| {},
    )
    .unwrap();
    assert!(report.visited <= 2);
    assert!(
        ctx.take_warnings()
            .iter()
            .any(|s| s.contains("entry/path byte budget"))
    );
    m.unity("/custom/ArchGame", "ArchGame");
    let ctx = m.ctx(DeepScans {
        folders: vec![PathBuf::from("/custom/ArchGame")],
        ..Default::default()
    });
    let report = deep::scan_with_options(
        &m.env(),
        &ctx,
        &Options {
            arch: Some("aarch64".into()),
            budget_ms: 10_000,
            ..Default::default()
        },
        &|_| {},
    )
    .unwrap();
    assert!(
        report.games.iter().all(|g| g.launch.is_empty()),
        "wrong-architecture executable must not be selected"
    );
}
#[test]
fn full_disk_fake_clock_budget_reports_progress_and_stops_at_the_next_entry() {
    let m = Machine::new();
    for n in 0..100 {
        m.write(&format!("/home/fixture/Tree/file{n}"), b"synthetic");
    }
    let clock = testing::FakeClock::default();
    let env = m.env().with_clock(Arc::new(clock.clone()));
    let ctx = m.ctx(DeepScans {
        full_disk: true,
        ..Default::default()
    });
    let progress = Mutex::new(Vec::new());
    let report = deep::scan_with_options(
        &env,
        &ctx,
        // The budget also stops on real time, as a backstop; keep it out of
        // reach so only the fake clock ends the scan.
        &Options {
            budget_ms: 1000,
            ..Default::default()
        },
        &|p| {
            if p.visited == 4 && !p.done {
                clock.advance(Duration::from_millis(2000));
            }
            progress.lock().unwrap().push((p.kind, p.visited, p.done));
        },
    )
    .unwrap();
    assert!(report.timed_out.contains(&ScanKind::FullDisk));
    assert_eq!(report.visited, 4);
    let events = progress.lock().unwrap();
    assert!(events.iter().any(|e| e.1 > 0 && !e.2));
    assert!(events.last().unwrap().2);
}
#[test]
fn cancellation_during_full_disk_enumeration_returns_within_two_hundred_ms() {
    let m = Machine::new();
    for n in 0..1000 {
        m.write(&format!("/home/fixture/Tree/file{n}"), b"synthetic");
    }
    let cancel = Cancel::new();
    let ctx = ScanCtx::with_config(
        cancel.clone(),
        Arc::new(Config {
            deep: DeepScans {
                full_disk: true,
                ..Default::default()
            },
            ..Default::default()
        }),
    );
    let requested = Mutex::new(None);
    let result = deep::scan_with_options(
        &m.env(),
        &ctx,
        &Options {
            budget_ms: 10_000,
            ..Default::default()
        },
        &|p| {
            if p.visited == 64 && !p.done {
                *requested.lock().unwrap() = Some(Instant::now());
                cancel.cancel();
            }
        },
    );
    assert!(matches!(result, Err(SourceError::Cancelled)));
    assert!(requested.lock().unwrap().unwrap().elapsed() < Duration::from_millis(200));
}
#[test]
fn each_kind_has_its_own_budget_and_precancelled_scans_do_no_work() {
    let m = Machine::new();
    let ctx = m.ctx(DeepScans {
        folders: vec![PathBuf::from("/home/fixture")],
        portable: true,
        ..Default::default()
    });
    let report = deep::scan_with_options(
        &m.env(),
        &ctx,
        &Options {
            budget_ms: 0,
            ..Default::default()
        },
        &|_| {},
    )
    .unwrap();
    assert_eq!(
        report.timed_out,
        [ScanKind::CustomFolders, ScanKind::Portable]
    );
    assert_eq!(report.visited, 0);
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        deep::scan_with_options(
            &m.env(),
            &ScanCtx::new(cancel),
            &Options::default(),
            &|_| {}
        ),
        Err(SourceError::Cancelled)
    ));
}
#[test]
fn watch_roots_include_opted_custom_locations_outside_home() {
    let m = Machine::new();
    let config = Config {
        deep: DeepScans {
            folders: vec![PathBuf::from("/custom")],
            portable: true,
            external_drives: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let paths = deep::watch_paths(&m.env(), &config);
    assert!(
        paths
            .iter()
            .any(|p| p.path == m.path("/custom") && p.recursive)
    );
    assert!(
        paths
            .iter()
            .any(|p| p.path == m.path("/media/usb") && p.recursive)
    );
}

#[test]
fn public_scan_runs_only_the_enabled_deep_source_and_keeps_its_evidence() {
    let m = Machine::new();
    m.unity("/custom/Example", "Example");
    let report = scan(
        &m.env(),
        &Config {
            sources: SourceSelection::only([deep::ID]),
            deep: DeepScans {
                folders: vec![PathBuf::from("/custom")],
                ..Default::default()
            },
            ..Default::default()
        },
        &Cancel::new(),
    )
    .unwrap();
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].source, deep::ID);
    assert!(report.sources[0].error.is_none());
    assert_eq!(report.sources[0].count, 1);
    assert_eq!(report.groups.len(), 1);
    let game = report.groups[0].primary_entry().unwrap();
    assert_eq!(game.title, "Example");
    assert!(
        game.evidence
            .iter()
            .any(|e| matches!(e,Evidence::Rule{rule,..}if rule=="deep.custom"))
    );
}
#[test]
fn overlapping_full_disk_volumes_do_not_duplicate_candidates() {
    let m = Machine::new();
    m.unity("/media/usb/OneGame", "OneGame");
    let (report, w) = m.scan(DeepScans {
        full_disk: true,
        ..Default::default()
    });
    assert_eq!(report.games.len(), 1, "{w:?}");
    assert_eq!(report.games[0].confidence, Confidence::Maybe);
}
#[cfg(feature = "windows-sources")]
#[test]
fn removable_xbox_marker_reuses_game_config_reader_on_linux() {
    let m = Machine::new();
    let mut marker = b"RGBX".to_vec();
    marker.extend_from_slice(&1u32.to_le_bytes());
    marker.extend(
        "XboxGames"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes),
    );
    m.write("/media/usb/.GamingRoot", marker);
    m.write("/media/usb/XboxGames/Fixture/Content/MicrosoftGame.config",r#"<Game configVersion="1"><Identity Name="Fixture.ExternalGame" Publisher="CN=Fixture Publisher" Version="1.0.0.0"/><ExecutableList><Executable Name="play.exe" Id="Game" TargetDeviceFamily="PC"/></ExecutableList><ShellVisuals DefaultDisplayName="External Xbox Game"/><StoreId>9N000FIXTURE1</StoreId></Game>"#);
    m.pe("/media/usb/XboxGames/Fixture/Content/play.exe");
    let (report, w) = m.scan(DeepScans {
        external_drives: true,
        ..Default::default()
    });
    assert_eq!(report.games.len(), 1, "{w:?}");
    let game = &report.games[0];
    assert_eq!(game.title, "External Xbox Game");
    assert_eq!(game.confidence, Confidence::Certain);
    assert!(
        matches!(&game.launch[0],LaunchTarget::Uri{uri} if uri==r"shell:AppsFolder\Fixture.ExternalGame_85311vnhwwzqm!Game")
    );
}
#[cfg(unix)]
#[test]
fn symlinked_directories_and_payloads_are_not_followed_or_modified() {
    use std::os::unix::fs::symlink;
    let m = Machine::new();
    m.unity("/outside/Game", "Game");
    fs::create_dir_all(m.path("/custom")).unwrap();
    symlink(m.path("/outside/Game"), m.path("/custom/LinkedGame")).unwrap();
    let (report, w) = m.scan(DeepScans {
        folders: vec![PathBuf::from("/custom")],
        ..Default::default()
    });
    assert!(report.games.is_empty(), "{w:?}");
    let outside = crate::fixtures::tempdir();
    fs::write(outside.path().join("sentinel"), b"unchanged").unwrap();
    symlink(outside.path(), m.path("/custom/HostData")).unwrap();
    let _ = m.scan(DeepScans {
        folders: vec![PathBuf::from("/custom")],
        ..Default::default()
    });
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}
#[cfg(unix)]
#[test]
fn arbitrary_home_prefix_is_scanned_through_the_shared_prefix_api() {
    let m = Machine::new();
    let prefix = "/home/fixture/.hidden/arbitrary";
    m.write(
        &format!("{prefix}/system.reg"),
        r#"WINE REGISTRY Version 2
#arch=win64

[Software\\Wow6432Node\\GOG.com\\Games\\1207658930] 1700000000
"gameName"="Fixture Deep Prefix"
"path"="C:\\GOG Games\\Deep Prefix"
"exe"="C:\\GOG Games\\Deep Prefix\\quest.exe"
"#,
    );
    m.write(
        &format!("{prefix}/user.reg"),
        "WINE REGISTRY Version 2\n#arch=win64\n",
    );
    m.write(
        &format!("{prefix}/drive_c/GOG Games/Deep Prefix/quest.exe"),
        b"inert never executed",
    );
    fs::create_dir_all(m.path(&format!("{prefix}/drive_c/users/fixture/AppData/Roaming"))).unwrap();
    let (report, w) = m.scan(DeepScans {
        wine_prefixes: true,
        ..Default::default()
    });
    let game = report
        .games
        .iter()
        .find(|g| {
            g.ids.contains(&ExternalId::Gog {
                product_id: 1_207_658_930,
            })
        })
        .unwrap_or_else(|| panic!("{:?}\n{w:?}", report.games));
    assert_eq!(game.title, "Fixture Deep Prefix");
    assert!(
        matches!(&game.runtime,GameRuntime::Windows{prefix:Some(p),..} if p.path==m.path(prefix)&&p.owner.is_none())
    );
}
