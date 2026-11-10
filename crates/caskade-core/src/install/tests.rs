use super::*;
use caskade_installer::HandOff;
use std::path::Path;
use std::time::Duration;

#[test]
fn rom_and_delegated_handoffs_become_library_entries() {
    let rom = HandOff::Rom {
        platform: "snes".into(),
        files: vec!["game.sfc".into()],
    };
    let (game, entry) = from_handoff(&rom, "game.sfc", Path::new("/roms/game.sfc"));
    assert_eq!(game.title, "game");
    assert_eq!(entry.source_id, HANDOFF_SOURCE);
    assert_eq!(entry.state, caskade_discovery::InstallState::Installed);
    assert_eq!(entry.external_id, "/roms/game.sfc");
    assert!(matches!(
        &entry.launch,
        Some(crate::model::LaunchSpec::Emulator { platform_id, roms, .. })
            if platform_id == "snes"
                && roms == &[crate::emulation::GameRom::absolute("/roms/game.sfc")]
    ));
    assert_eq!(
        entry.location,
        Some(crate::model::InstallLocation::External {
            path: "/roms".into()
        })
    );

    let delegated = HandOff::Delegated {
        launch_uri: "steam://rungameid/620".into(),
        uninstall_steps: vec![],
    };
    let (_, entry) = from_handoff(&delegated, "Portal 2", Path::new("/tmp/x"));
    assert_eq!(entry.state, caskade_discovery::InstallState::Installed);
    assert_eq!(
        entry.launch,
        Some(crate::model::LaunchSpec::Uri {
            uri: "steam://rungameid/620".into()
        })
    );
}

#[test]
fn a_package_runs_the_command_it_installed_under_its_own_title() {
    let package = HandOff::Package {
        package: "ninja-master".into(),
        title: "Ninja Master".into(),
        program: Some("/usr/bin/ninja-master".into()),
        args: vec!["--fullscreen".into()],
        uninstall_steps: vec![],
    };
    let input = Path::new("/tmp/ninja-master-0.1.4-linux-x86_64.deb");
    let (game, entry) = from_handoff(&package, "ninja-master-0.1.4-linux-x86_64.deb", input);
    assert_eq!(game.title, "Ninja Master");
    assert_eq!(entry.game_id, game.id);
    assert_eq!(entry.source_id, HANDOFF_SOURCE);
    assert_eq!(entry.state, caskade_discovery::InstallState::Installed);
    assert_eq!(entry.location, None);
    assert_eq!(
        entry.launch,
        Some(crate::model::LaunchSpec::Executable {
            path: "/usr/bin/ninja-master".into(),
            args: vec!["--fullscreen".into()],
            cwd: None,
            env: Default::default(),
        })
    );
    // A package without a command is installed, with nothing to play.
    let silent = HandOff::Package {
        package: "ninja-master".into(),
        title: "Ninja Master".into(),
        program: None,
        args: vec![],
        uninstall_steps: vec![],
    };
    assert_eq!(from_handoff(&silent, "x", input).1.launch, None);
}

#[test]
fn titles_from_paths_and_urls() {
    assert_eq!(title_of("/games/Fixture Runner/"), "Fixture Runner");
    assert_eq!(title_of(r"C:\Downloads\setup_game.exe"), "setup_game.exe");
    assert_eq!(
        title_of("https://example.com/files/game.zip?token=x"),
        "game.zip"
    );
    assert_eq!(
        title_of("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Some+Game"),
        "Some Game"
    );
    assert_eq!(
        title_of("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"),
        "Torrent"
    );
}

async fn finished(service: &InstallService, id: &str) -> JobSnapshot {
    for _ in 0..600 {
        let job = service.job(id).unwrap();
        if job.status.is_finished() || job.status == JobStatus::NeedsUser {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {id} did not finish: {:?}", service.job(id));
}

/// #51 acceptance: a fixture installed through core is a launchable library
/// entry, and launching it through its alias records playtime.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn an_installed_fixture_launches_through_its_alias_and_records_playtime() {
    use crate::bus::GameSession;
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-fixture").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();

    // A portable game: the installer's real fixture executable plus data.
    let source = temp.path().join("Fixture Runner");
    std::fs::create_dir_all(&source).unwrap();
    let exe = source.join("fixture-runner.x86_64");
    crate::test_programs::copy_program(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        &exe,
    );
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(source.join("data.pak"), vec![7u8; 4096]).unwrap();

    let aliases = AliasStore::new(temp.path().join("aliases"));
    let bus = Bus::default();
    let mut stream = bus.stream();
    let service = InstallService::new(
        store.clone(),
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: Some(aliases.clone()),
        },
    );
    let queued = service
        .install(source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    assert_eq!(queued.status, JobStatus::Queued);
    assert_eq!(queued.root_id.as_deref(), Some("root-fixture"));
    let job = finished(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    let game_id = job.game_id.clone().unwrap();

    let entries = store.entries_for_game(&game_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.source_id, STORAGE_SOURCE);
    assert!(uuid::Uuid::parse_str(&entry.external_id).is_ok());
    let copied = std::fs::metadata(&exe).unwrap().len() + 4096;
    assert_eq!(entry.size, Some(copied));
    let Some(crate::model::InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
        panic!("{:?}", entry.location);
    };
    assert_eq!(root_id, "root-fixture");
    assert!(
        root_path
            .join(rel_dir)
            .join("fixture-runner.x86_64")
            .is_file()
    );
    let alias_id = entry.alias_id.clone().expect("installs get an alias");
    assert!(
        aliases
            .path(&alias_id)
            .unwrap()
            .join("fixture-runner.x86_64")
            .is_file()
    );
    assert!(matches!(
        &entry.launch,
        Some(crate::model::LaunchSpec::Executable { path, .. }) if path.ends_with("fixture-runner.x86_64")
    ));

    // Every change was published, ending with the completed snapshot.
    let mut last = None;
    while let Ok(event) = stream.try_recv() {
        if let Event::InstallJobChanged(snapshot) = event {
            last = Some(snapshot.revision);
        }
    }
    assert_eq!(last, Some(job.revision));

    // The user's launch options: the fixture writes a token when it runs.
    let token = temp.path().join("token");
    let mut launchable = entry.clone();
    if let Some(crate::model::LaunchSpec::Executable { args, .. }) = &mut launchable.launch {
        *args = vec!["--token".into(), token.to_string_lossy().into_owned()];
    }
    store.upsert_entry(&launchable).await.unwrap();
    let launcher = crate::launch::LaunchService::new(store.clone()).with_aliases(Some(aliases));
    let handle = launcher.launch(&entry.id).await.unwrap();
    let session = tokio::time::timeout(Duration::from_secs(60), handle.wait())
        .await
        .expect("the fixture exits")
        .unwrap();
    launcher.shutdown();
    assert!(session.ended_at_ms.is_some(), "{session:?}");
    assert_eq!(session.entry_id, entry.id);
    let ran = std::fs::read_to_string(&token).expect("the game ran");
    assert!(ran.trim_end().ends_with("fixture-runner.x86_64"), "{ran}");
    assert_eq!(
        store.sessions_for_game(&game_id, 10).await.unwrap().len(),
        1
    );

    // On first launch the game saved its real path; the first-exit rescan
    // records it as a rewritable reference.
    let game_dir = root_path.join(rel_dir);
    std::fs::write(
        game_dir.join("settings.ini"),
        format!("[paths]\ndata={}/data.pak\n", game_dir.display()),
    )
    .unwrap();
    let exited = GameSession {
        game_id: session.game_id.clone(),
        entry_id: session.entry_id.clone(),
        session_id: session.id.clone(),
        started_at_ms: session.started_at_ms,
        ended_at_ms: session.ended_at_ms,
        duration_ms: 0,
    };
    assert!(service.rescan_after_exit(&exited).await.unwrap());
    let relocation = caskade_installer::manifest::InstallManifest::read(&game_dir)
        .unwrap()
        .into_record()
        .relocation;
    assert_eq!(
        relocation.verdict,
        caskade_installer::manifest::RelocationVerdict::Rewritable,
        "{relocation:?}"
    );
    assert!(format!("{:?}", relocation.references).contains("settings.ini"));
}

/// A dropped shortcut's title and target outrank what the files say.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn install_hints_choose_the_title_and_launch_target() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-hints").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let source = temp.path().join("Fixture Runner");
    for exe in ["fixture-runner.x86_64", "tools/editor.x86_64"] {
        let exe = source.join(exe);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        crate::test_programs::copy_program(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
            ),
            &exe,
        );
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let plan = dropped::DropPlan {
        dropped: "/menu/Editor.desktop".into(),
        kind: dropped::DropKind::Shortcut,
        input: source.to_string_lossy().into_owned(),
        title: Some("Runner Editor".into()),
        launch: Some(dropped::DropLaunch {
            path: "tools/editor.x86_64".into(),
            args: vec!["--edit".into()],
            working_dir: None,
        }),
    };
    let queued = service
        .install_with(plan.input.clone(), None, plan.hints())
        .await
        .unwrap();
    assert_eq!(queued.title, "Runner Editor");
    let job = finished(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    let game_id = job.game_id.unwrap();
    let game = store.game(&game_id).await.unwrap().unwrap();
    assert_eq!(game.title, "Runner Editor");
    let entry = &store.entries_for_game(&game_id).await.unwrap()[0];
    assert!(
        matches!(
            &entry.launch,
            Some(crate::model::LaunchSpec::Executable { path, args, .. })
                if path.ends_with("tools/editor.x86_64") && args == &["--edit"]
        ),
        "{:?}",
        entry.launch
    );
}

/// A port installed for a game the user has becomes another release of it,
/// with the user's file copied in and passed along (ports, AttachTo).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn an_attached_install_becomes_a_release_of_the_existing_game() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-attach").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let owned = store
        .upsert_game(&crate::model::Game::new("Majora's Mask"))
        .await
        .unwrap();
    let source = temp.path().join("Zelda64Recomp");
    for exe in ["Zelda64Recompiled/Zelda64Recompiled", "tools/helper.x86_64"] {
        let exe = source.join(exe);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        crate::test_programs::copy_program(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
            ),
            &exe,
        );
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let rom = temp.path().join("Majora's Mask (USA).z64");
    std::fs::write(&rom, b"rom").unwrap();
    let service = InstallService::new(
        store.clone(),
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let queued = service
        .install_attached(
            source.to_string_lossy().into_owned(),
            None,
            caskade_installer::discover::DiscoveryHints {
                installer_title: Some("Zelda64Recomp".into()),
                ..Default::default()
            },
            AttachTo {
                game_id: owned.id.clone(),
                executable: Some("Zelda64Recompiled".into()),
                files: vec![PlacedFile {
                    source: rom.clone(),
                    into: "Game Files".into(),
                    name: None,
                }],
                args: vec![rom.to_string_lossy().into_owned()],
            },
        )
        .await
        .unwrap();
    let job = finished(&service, &queued.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    assert_eq!(job.game_id.as_deref(), Some(owned.id.as_str()));
    assert!(job.warnings.is_empty(), "{:?}", job.warnings);
    assert_eq!(
        store.game_ids(&Default::default()).await.unwrap(),
        vec![owned.id.clone()],
        "no second game"
    );
    let entries = store.entries_for_game(&owned.id).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(
            &entries[0].launch,
            Some(crate::model::LaunchSpec::Executable { path, args, .. })
                if path.ends_with("Zelda64Recompiled/Zelda64Recompiled")
                    && args == &[rom.to_string_lossy().into_owned()]
        ),
        "{:?}",
        entries[0].launch
    );
    let dir = job.target_dir.unwrap();
    assert_eq!(
        std::fs::read(dir.join("Game Files/Majora's Mask (USA).z64")).unwrap(),
        b"rom"
    );
}

/// A folder the installer has no way to install says why in words, with
/// what can be done, never the installer's reason as it is (#76).
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_with_nothing_to_install_says_why() {
    use caskade_installer::failure::{FailureKind, RecoveryAction};
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-unsupported").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root =
        StorageRoot::from_manifest(std::fs::canonicalize(&root_path).unwrap(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let source = temp.path().join("Snake");
    std::fs::create_dir_all(source.join("files")).unwrap();
    std::fs::write(source.join("caskade-web.json"), "{}").unwrap();
    std::fs::write(source.join("files/0a1b"), "<canvas></canvas>").unwrap();
    let service = InstallService::new(
        store,
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let job = service
        .install(source.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    let job = finished(&service, &job.id).await;
    assert_eq!(job.status, JobStatus::Failed, "{job:?}");
    let failure = job.failure.expect("a classified failure");
    assert_eq!(failure.kind, FailureKind::Unsupported);
    assert_eq!(
        failure.actions,
        vec![RecoveryAction::LocateSource, RecoveryAction::Cancel]
    );
    assert!(
        failure
            .summary
            .starts_with("\u{201c}Snake\u{201d} has no game program"),
        "{}",
        failure.summary
    );
    let error = job.error.unwrap_or_default();
    assert_eq!(error, failure.summary);
    assert!(!error.contains("unsupported_format"), "{error}");
}

#[tokio::test]
async fn installs_need_a_storage_root() {
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let service = InstallService::new(
        store,
        Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    assert!(service.install("/nowhere".into(), None).await.is_err());
    assert!(
        service
            .install("/nowhere".into(), Some("missing".into()))
            .await
            .is_err()
    );
}

/// #120: a ROM installed through the queue becomes an emulated game on the
/// catalogue's platform, imported where it is; installing it again updates
/// the same game.
#[tokio::test(flavor = "multi_thread")]
async fn a_rom_installs_as_an_emulated_game_on_the_catalogue_platform() {
    use crate::emulation::{
        Catalog, EmulationService, Emulator, EmulatorInstall, EmulatorProfile, PlatformSettings,
        ProfileHooks, ProfileRef, RomLocation, Tracking,
    };
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-roms").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root = StorageRoot::from_manifest(root_path.clone(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let roms = temp.path().join("ROMs");
    std::fs::create_dir_all(&roms).unwrap();
    let rom = roms.join("Advance Wars (USA).gba");
    std::fs::write(&rom, vec![0u8; 512]).unwrap();

    let bus = Bus::default();
    let catalog = Catalog::load([(
        "platforms.toml",
        r#"
[[platform]]
id = "nintendo_gba"
name = "Game Boy Advance"
short = "GBA"
extensions = ["gba"]

[[region]]
id = "usa"
name = "USA"
codes = ["USA"]
"#,
    )])
    .unwrap();
    let emulation = EmulationService::new(store.clone(), bus.clone(), catalog);
    let service = InstallService::new(
        store.clone(),
        bus,
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    service.set_emulation(emulation);

    let install = || async {
        let queued = service
            .install(rom.to_string_lossy().into_owned(), None)
            .await
            .unwrap();
        finished(&service, &queued.id).await
    };
    let job = install().await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    assert_eq!(job.title, "Advance Wars");
    let codes: Vec<&str> = job.warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(codes, ["rom_scanner_suggested", "rom_no_emulator"]);
    let game_id = job.game_id.clone().unwrap();
    let entries = store.entries_for_game(&game_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.state, caskade_discovery::InstallState::Installed);
    let Some(crate::model::LaunchSpec::Emulator {
        platform_id, roms, ..
    }) = &entry.launch
    else {
        panic!("{:?}", entry.launch);
    };
    // The installer says nintendo_game_boy_advance; the catalogue nintendo_gba.
    assert_eq!(platform_id, "nintendo_gba");
    assert_eq!(
        roms[0].location,
        RomLocation::Absolute { path: rom.clone() }
    );
    assert!(rom.is_file(), "imported where it is");

    // With an emulator for the platform, installing again updates the game
    // and no longer asks for one.
    let emulator = Emulator {
        id: "mgba".into(),
        name: "mGBA".into(),
        definition: None,
        install: EmulatorInstall::Path {
            binary: "mgba".into(),
        },
        profiles: vec![EmulatorProfile::Custom {
            id: "gba".into(),
            name: "GBA".into(),
            executable: None,
            args: vec!["{RomPath}".into()],
            working_dir: None,
            platforms: vec!["nintendo_gba".into()],
            extensions: vec!["gba".into()],
            env: vec![],
            tracking: Tracking::Default,
            hooks: ProfileHooks::default(),
        }],
    };
    store.upsert_emulator(&emulator).await.unwrap();
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
    let again = install().await;
    assert_eq!(again.status, JobStatus::Completed, "{again:?}");
    assert_eq!(again.game_id.as_deref(), Some(game_id.as_str()));
    let codes: Vec<&str> = again.warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(codes, ["rom_scanner_suggested"]);
    assert_eq!(store.entries_for_game(&game_id).await.unwrap().len(), 1);
}

/// #120: with a ROM scanner for the platform, the job asks where the ROM
/// goes; moving it there runs the scanner, keeping it imports in place.
#[tokio::test(flavor = "multi_thread")]
async fn a_rom_goes_into_a_scanner_folder_when_the_user_picks_it() {
    use crate::emulation::{
        Catalog, EmulationService, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget,
        ProfileHooks, ProfileRef, RomLocation, RomScanner, Tracking,
    };
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-scan").unwrap();
    identity.write(&root_path).unwrap();
    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let mut root = StorageRoot::from_manifest(root_path.clone(), &identity);
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let library = temp.path().join("Library/GBA");
    std::fs::create_dir_all(&library).unwrap();
    let downloads = temp.path().join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();

    let bus = Bus::default();
    let catalog = Catalog::load([(
        "platforms.toml",
        "[[platform]]\nid = \"nintendo_gba\"\nname = \"Game Boy Advance\"\nshort = \"GBA\"\nextensions = [\"gba\"]\n",
    )])
    .unwrap();
    let emulation = EmulationService::new(store.clone(), bus.clone(), catalog);
    store
        .upsert_emulator(&Emulator {
            id: "mgba".into(),
            name: "mGBA".into(),
            definition: None,
            install: EmulatorInstall::Path {
                binary: "mgba".into(),
            },
            profiles: vec![EmulatorProfile::Custom {
                id: "gba".into(),
                name: "GBA".into(),
                executable: None,
                args: vec!["{RomPath}".into()],
                working_dir: None,
                platforms: vec!["nintendo_gba".into()],
                extensions: vec!["gba".into()],
                env: vec![],
                tracking: Tracking::Default,
                hooks: ProfileHooks::default(),
            }],
        })
        .await
        .unwrap();
    store
        .upsert_scanner(&RomScanner {
            id: "gba-library".into(),
            name: "GBA library".into(),
            folder: library.clone(),
            platform: Some("nintendo_gba".into()),
            profile: ProfileRef {
                emulator_id: "mgba".into(),
                profile_id: "gba".into(),
            },
            launch_with: EmulatorTarget::PlatformDefault,
            recursive: true,
            scan_archives: false,
            skip_cloud_placeholders: false,
            exclusions: vec![],
            in_library_update: true,
        })
        .await
        .unwrap();
    let service = InstallService::new(
        store.clone(),
        bus,
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    service.set_emulation(emulation);

    let install = |name: &str| {
        let rom = downloads.join(name);
        std::fs::write(&rom, vec![0u8; 256]).unwrap();
        let service = service.clone();
        async move {
            let queued = service
                .install(rom.to_string_lossy().into_owned(), None)
                .await
                .unwrap();
            (rom, finished(&service, &queued.id).await)
        }
    };

    // Moving into the scanner's folder (the suggestion on one filesystem).
    let (rom, job) = install("Advance Wars (USA).gba").await;
    assert_eq!(job.status, JobStatus::NeedsUser, "{job:?}");
    let Some(caskade_installer::Question::ChooseDestination { options, .. }) = &job.question else {
        panic!("{:?}", job.question);
    };
    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
    assert!(labels[0].starts_with("Move into GBA library"), "{labels:?}");
    assert!(labels[1].starts_with("Copy into GBA library"), "{labels:?}");
    assert_eq!(labels[2], "Keep them where they are");
    let done = service
        .answer(&job.id, job.revision, JobAnswer::Select { index: 0 })
        .await
        .unwrap();
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert!(!rom.exists());
    assert!(library.join("Advance Wars (USA).gba").is_file());
    let entries = store
        .entries_for_game(done.game_id.as_ref().unwrap())
        .await
        .unwrap();
    assert!(entries.iter().any(|entry| matches!(
        &entry.launch,
        Some(crate::model::LaunchSpec::Emulator { roms, .. })
            if roms[0].location == RomLocation::Scanner {
                scanner_id: "gba-library".into(),
                path: "Advance Wars (USA).gba".into(),
            }
    )));

    // Keeping it where it is imports it in place.
    let (rom, job) = install("Golden Sun (USA).gba").await;
    let done = service
        .answer(&job.id, job.revision, JobAnswer::Select { index: 2 })
        .await
        .unwrap();
    assert_eq!(done.status, JobStatus::Completed, "{done:?}");
    assert!(rom.is_file());
    let entries = store
        .entries_for_game(done.game_id.as_ref().unwrap())
        .await
        .unwrap();
    assert_eq!(entries[0].source_id, HANDOFF_SOURCE);

    // An out-of-range answer leaves the question open.
    let (_, job) = install("Mother 3 (Japan).gba").await;
    assert!(
        service
            .answer(&job.id, job.revision, JobAnswer::Select { index: 9 })
            .await
            .is_err()
    );
    assert_eq!(service.job(&job.id).unwrap().status, JobStatus::NeedsUser);
}

/// Installer progress is throttled: a burst of chunk reports becomes a few
/// job snapshots, while phase changes, totals and the newest numbers still
/// come through.
#[tokio::test]
async fn progress_bursts_are_throttled() {
    use caskade_installer::events::Phase;

    let store = crate::db::LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let mut stream = bus.stream();
    let service = InstallService::new(store, bus, InstallOptions::default());
    let id = "throttled".to_owned();
    let now = now_ms();
    lock(&service.inner.jobs).insert(
        id.clone(),
        Job {
            snapshot: JobSnapshot {
                id: id.clone(),
                revision: 0,
                kind: JobKind::Install,
                status: JobStatus::Downloading,
                title: "burst".into(),
                input: "https://example.com/burst.zip".into(),
                root_id: None,
                phase: None,
                progress: None,
                warnings: vec![],
                question: None,
                game_id: None,
                error: None,
                candidates: vec![],
                selected_candidate_index: None,
                target_dir: None,
                dependencies: vec![],
                verification: None,
                failure: None,
                created_at_ms: now,
                updated_at_ms: now,
            },
            cancel: CancellationToken::new(),
            control: pipeline::RunControl::default(),
            active: true,
            update: None,
            hints: Default::default(),
            peer: None,
            peer_fetcher: None,
            attach: None,
        },
    );
    let event = |phase, bytes, total_bytes, message: &str| JobEvent {
        job_id: id.clone(),
        phase,
        step: None,
        bytes,
        total_bytes,
        files: 0,
        eta_secs: None,
        message: message.into(),
    };
    let counts = |stream: &mut tokio::sync::broadcast::Receiver<Event>| {
        let (mut changed, mut progress) = (0, Vec::new());
        while let Ok(event) = stream.try_recv() {
            match event {
                Event::InstallJobChanged(_) => changed += 1,
                Event::InstallProgress {
                    phase, progress: p, ..
                } => progress.push((phase, p.bytes, p.message)),
                _ => {}
            }
        }
        (changed, progress)
    };

    const CHUNK: u64 = 64 * 1024;
    const N: u64 = 2_000;
    let sink = Progress::new(service.clone(), id.clone());
    for i in 1..N {
        sink.emit(event(
            Phase::Download,
            i * CHUNK,
            Some(N * CHUNK),
            "progress",
        ));
    }
    let (changed, progress) = counts(&mut stream);
    // The first report always goes out; the rest only every 200 ms.
    assert!(
        (1..20).contains(&changed),
        "{changed} snapshots for {N} reports"
    );
    assert_eq!(progress.len(), changed, "{progress:?}");
    assert_eq!(progress[0], (JobPhase::Download, CHUNK, "progress".into()));

    // The report that reaches the total is never held back.
    sink.emit(event(
        Phase::Download,
        N * CHUNK,
        Some(N * CHUNK),
        "progress",
    ));
    let (_, progress) = counts(&mut stream);
    assert_eq!(
        progress,
        [(JobPhase::Download, N * CHUNK, "progress".into())]
    );

    // A phase change goes out at once, inside the interval.
    sink.emit(event(Phase::Install, 1, None, "extracting"));
    let (changed, progress) = counts(&mut stream);
    assert_eq!(changed, 1);
    assert_eq!(progress, [(JobPhase::Install, 1, "extracting".into())]);

    // Throttled reports are kept: dropping the sink publishes the newest.
    for bytes in 2..=500 {
        sink.emit(event(Phase::Install, bytes, None, "extracting"));
    }
    drop(sink);
    let job = service.job(&id).unwrap();
    assert_eq!(job.phase, Some(JobPhase::Install));
    assert_eq!(job.progress.unwrap().bytes, 500);
    let (_, progress) = counts(&mut stream);
    assert_eq!(progress.last().unwrap().1, 500);
}
