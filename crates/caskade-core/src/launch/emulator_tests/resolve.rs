use super::*;

#[tokio::test]
async fn flatpak_retroarch_shares_an_unseen_rom_folder_and_tracks_by_process_name() {
    let setup = Setup::new().await;
    setup
        .emulator(
            flatpak(),
            built_in(ArgsOverride::Default, ProfileHooks::default()),
        )
        .await;
    setup.platform_default().await;
    let roms = setup.rom_files(&setup.roms(), &["FF7 (Disc 1).cue", "FF7 (Disc 2).cue"]);
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;

    // Two discs without a playlist: only the disc is asked for.
    let asked = prompt(
        setup
            .launches
            .resolve(&entry, &LaunchChoice::default())
            .await,
    );
    assert!(asked.profiles.is_empty());
    assert_eq!(
        asked
            .discs
            .iter()
            .map(|disc| disc.name.as_str())
            .collect::<Vec<_>>(),
        ["FF7 (Disc 1).cue", "FF7 (Disc 2).cue"]
    );

    let choice = LaunchChoice {
        action: None,
        profile: None,
        disc: Some(1),
    };
    let prepared = setup.launches.resolve(&entry, &choice).await.unwrap();
    assert_eq!(
        prepared.command.program,
        setup.temp.path().join("bin/flatpak")
    );
    assert_eq!(
        prepared.command.args,
        [
            "run".to_owned(),
            format!("--filesystem={}:ro", text(&setup.roms())),
            FLATPAK_ID.into(),
            "-L".into(),
            text(&setup.cores().join("mednafen_psx_hw_libretro.so")),
            text(&roms[1]),
        ]
    );
    assert_eq!(prepared.command.cwd, None);
    let emulation = prepared.command.emulation.as_ref().unwrap();
    assert_eq!(
        (
            emulation.emulator_id.as_str(),
            emulation.profile_id.as_str()
        ),
        ("ra", "psx")
    );
    assert_eq!(emulation.rom, roms[1]);
    assert_eq!(prepared.target.mode, TrackingMode::ProcessName);
    assert_eq!(prepared.target.exe_names, ["retroarch"]);
    assert!(!prepared.direct);
}

#[tokio::test]
async fn flatpak_gets_no_filesystem_flag_for_folders_it_already_sees() {
    let setup = Setup::new().await;
    setup
        .emulator(
            flatpak(),
            built_in(ArgsOverride::Default, ProfileHooks::default()),
        )
        .await;
    setup.platform_default().await;
    // `filesystems=home` in the fixture's metadata covers ~/ROMs.
    let roms = setup.rom_files(&setup.machine.home.join("ROMs"), &["Vagrant Story.chd"]);
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    let prepared = setup
        .launches
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(&prepared.command.args[..2], ["run", FLATPAK_ID]);

    // A user override revoking `home` brings the flag back.
    let overrides = setup.machine.home.join(".local/share/flatpak/overrides");
    fs::create_dir_all(&overrides).unwrap();
    fs::write(
        overrides.join(FLATPAK_ID),
        "[Context]\nfilesystems=!home;\n",
    )
    .unwrap();
    let prepared = setup
        .launches
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(
        prepared.command.args[1],
        format!("--filesystem={}:ro", text(&setup.machine.home.join("ROMs")))
    );
}

#[tokio::test]
async fn a_playlist_launches_without_asking_for_a_disc() {
    let setup = Setup::new().await;
    setup
        .emulator(
            flatpak(),
            built_in(ArgsOverride::Default, ProfileHooks::default()),
        )
        .await;
    setup.platform_default().await;
    let roms = setup.rom_files(
        &setup.roms(),
        &["FF7 (Disc 1).cue", "FF7 (Disc 2).cue", "FF7.m3u"],
    );
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    let prepared = setup
        .launches
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(prepared.command.args.last().unwrap(), &text(&roms[2]));
}

#[tokio::test]
async fn ask_targets_offer_compatible_profiles_and_resume_with_the_answer() {
    let setup = Setup::new().await;
    setup
        .emulator(
            flatpak(),
            built_in(ArgsOverride::Default, ProfileHooks::default()),
        )
        .await;
    let roms = setup.rom_files(&setup.roms(), &["Vagrant Story.chd"]);
    let option = ProfileOption {
        emulator_id: "ra".into(),
        emulator_name: "RetroArch".into(),
        profile_id: "psx".into(),
        profile_name: "Beetle PSX HW".into(),
        installed: true,
    };
    for target in [
        EmulatorTarget::AskEmulator,
        EmulatorTarget::AskProfile {
            emulator_id: "ra".into(),
        },
    ] {
        let entry = setup.game(target, &roms, ArgsOverride::Default).await;
        let asked = prompt(
            setup
                .launches
                .resolve(&entry, &LaunchChoice::default())
                .await,
        );
        assert_eq!(asked.profiles, std::slice::from_ref(&option));
        assert!(asked.discs.is_empty());

        let wrong = LaunchChoice {
            action: None,
            profile: Some(ProfileRef {
                emulator_id: "ra".into(),
                profile_id: "other".into(),
            }),
            disc: None,
        };
        assert!(matches!(
            setup.launches.resolve(&entry, &wrong).await,
            Err(LaunchError::Invalid(_))
        ));
        let answer = LaunchChoice {
            action: None,
            profile: Some(psx()),
            disc: None,
        };
        let prepared = setup.launches.resolve(&entry, &answer).await.unwrap();
        assert_eq!(prepared.command.args.last().unwrap(), &text(&roms[0]));
    }

    // Without a platform default the launch fails with the game page's
    // readiness message (#122) instead of asking.
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    match setup
        .launches
        .resolve(&entry, &LaunchChoice::default())
        .await
    {
        Err(LaunchError::Invalid(message)) => assert!(message.contains("PS1"), "{message}"),
        other => panic!("expected the readiness message, got {:?}", other.err()),
    }

    // Asking for another emulator's profiles offers nothing to choose.
    let entry = setup
        .game(
            EmulatorTarget::AskProfile {
                emulator_id: "missing".into(),
            },
            &roms,
            ArgsOverride::Default,
        )
        .await;
    assert!(matches!(
        setup
            .launches
            .resolve(&entry, &LaunchChoice::default())
            .await,
        Err(LaunchError::Invalid(_))
    ));
}

#[tokio::test]
async fn each_install_kind_builds_its_argv_and_tracking_default() {
    let setup = Setup::new().await;
    let roms = setup.rom_files(&setup.roms(), &["Vagrant Story.chd"]);
    let rom = text(&roms[0]);
    let linux_cores = setup.machine.home.join(".config/retroarch/cores");
    let core = |cores: &Path| text(&cores.join("mednafen_psx_hw_libretro.so"));

    let folder = setup.temp.path().join("RetroArch");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("retroarch"), b"").unwrap();
    let bin = setup.temp.path().join("path-bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("retroarch"), b"").unwrap();
    let appimage = setup
        .temp
        .path()
        .join("Applications/RetroArch-x86_64.AppImage");
    let bundle = setup.temp.path().join("Applications/RetroArch.app");
    fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    fs::write(&appimage, b"").unwrap();
    fs::write(
        bundle.join("Contents/Info.plist"),
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>RetroArch-bin</string></dict></plist>",
    )
    .unwrap();
    let launches = setup.launches.clone();

    let cases = [
        (
            EmulatorInstall::Folder {
                dir: folder.clone(),
                executable: "retroarch".into(),
            },
            folder.join("retroarch"),
            Some(folder.clone()),
            core(&linux_cores),
            TrackingMode::ProcessTree,
            true,
        ),
        (
            EmulatorInstall::Path {
                binary: "retroarch".into(),
            },
            bin.join("retroarch"),
            None,
            core(&linux_cores),
            TrackingMode::ProcessTree,
            true,
        ),
        (
            EmulatorInstall::AppImage {
                file: appimage.clone(),
            },
            appimage.clone(),
            None,
            core(&linux_cores),
            TrackingMode::ProcessTree,
            true,
        ),
        (
            EmulatorInstall::AppBundle {
                bundle: bundle.clone(),
            },
            bundle.join("Contents/MacOS/RetroArch-bin"),
            None,
            core(&linux_cores),
            TrackingMode::OriginalProcess,
            true,
        ),
    ];
    for (install, program, cwd, core, mode, direct) in cases {
        setup
            .emulator(
                install.clone(),
                built_in(ArgsOverride::Default, ProfileHooks::default()),
            )
            .await;
        let entry = setup
            .game(
                EmulatorTarget::Profile {
                    emulator_id: "ra".into(),
                    profile_id: "psx".into(),
                },
                &roms,
                ArgsOverride::Default,
            )
            .await;
        let prepared = launches
            .resolve(&entry, &LaunchChoice::default())
            .await
            .unwrap();
        assert_eq!(prepared.command.program, program, "{install:?}");
        assert_eq!(prepared.command.args, ["-L", &core, &rom], "{install:?}");
        assert_eq!(prepared.command.cwd, cwd, "{install:?}");
        assert_eq!(prepared.target.mode, mode, "{install:?}");
        assert_eq!(prepared.direct, direct, "{install:?}");
    }
}

#[tokio::test]
async fn game_arguments_and_custom_profiles_change_the_argv_and_tracking() {
    let setup = Setup::new().await;
    setup
        .emulator(
            flatpak(),
            built_in(
                ArgsOverride::Append {
                    args: vec!["--verbose".into()],
                },
                ProfileHooks::default(),
            ),
        )
        .await;
    setup.platform_default().await;
    let roms = setup.rom_files(&setup.machine.home.join("ROMs"), &["Vagrant Story.chd"]);
    let rom = text(&roms[0]);
    let appended = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Append {
                args: vec!["--fullscreen".into()],
            },
        )
        .await;
    let prepared = setup
        .launches
        .resolve(&appended, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(
        prepared.command.args[2..],
        [
            "-L".to_owned(),
            text(&setup.cores().join("mednafen_psx_hw_libretro.so")),
            rom.clone(),
            "--verbose".into(),
            "--fullscreen".into(),
        ]
    );
    let replaced = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Replace {
                args: vec!["{RomName}".into()],
                replaced: Vec::new(),
            },
        )
        .await;
    let prepared = setup
        .launches
        .resolve(&replaced, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(prepared.command.args[2..], ["Vagrant Story.chd"]);

    // A custom profile with its own program, environment, hooks and tracking.
    let tool = setup.temp.path().join("tools/psx");
    setup
        .emulator(
            flatpak(),
            EmulatorProfile::Custom {
                id: "psx".into(),
                name: "Standalone".into(),
                executable: Some(tool.clone()),
                args: vec!["--game".into(), "{RomPath}".into()],
                working_dir: Some("{RomDir}".into()),
                platforms: vec!["sony_playstation".into()],
                extensions: vec!["chd".into()],
                env: vec![("PSX_TITLE".into(), "{Name}".into())],
                tracking: Tracking::ProcessName {
                    name: "psx-{Platform}".into(),
                },
                hooks: ProfileHooks {
                    before: Some(vec!["/bin/mount-saves".into(), "{GameId}".into()]),
                    after_exit: Some(vec!["/bin/sync-saves".into(), "{RomNameNoExt}".into()]),
                },
            },
        )
        .await;
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    let prepared = setup
        .launches
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(prepared.command.program, tool);
    assert_eq!(prepared.command.args, ["--game", &rom]);
    assert_eq!(
        prepared.command.cwd,
        roms[0].parent().map(Path::to_path_buf)
    );
    assert_eq!(prepared.command.env["PSX_TITLE"], "Final Fantasy VII");
    assert_eq!(prepared.target.mode, TrackingMode::ProcessName);
    assert_eq!(prepared.target.exe_names, ["psx-sony_playstation"]);
    let emulation = prepared.command.emulation.unwrap();
    assert_eq!(
        emulation.before.unwrap(),
        ["/bin/mount-saves".to_owned(), entry.game_id.clone()]
    );
    assert_eq!(
        emulation.after_exit.unwrap(),
        ["/bin/sync-saves", "Vagrant Story"]
    );
}

#[tokio::test]
async fn folder_tracking_follows_the_expanded_folder() {
    let setup = Setup::new().await;
    let dir = setup.temp.path().join("RetroArch");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("retroarch"), b"").unwrap();
    setup
        .emulator(
            EmulatorInstall::Folder {
                dir: dir.clone(),
                executable: "retroarch".into(),
            },
            EmulatorProfile::Custom {
                id: "psx".into(),
                name: "Folder tracked".into(),
                executable: None,
                args: vec!["{RomPath}".into()],
                working_dir: None,
                platforms: vec!["sony_playstation".into()],
                extensions: Vec::new(),
                env: Vec::new(),
                tracking: Tracking::Folder {
                    path: "{EmulatorDir}/bin".into(),
                },
                hooks: ProfileHooks::default(),
            },
        )
        .await;
    setup.platform_default().await;
    let roms = setup.rom_files(&setup.roms(), &["Vagrant Story.chd"]);
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    let prepared = setup
        .launches
        .resolve(&entry, &LaunchChoice::default())
        .await
        .unwrap();
    assert_eq!(prepared.command.program, dir.join("retroarch"));
    assert_eq!(prepared.command.cwd, Some(dir.clone()));
    assert_eq!(prepared.target.mode, TrackingMode::Folder);
    assert_eq!(prepared.target.real_dir, Some(dir.join("bin")));
    assert!(!prepared.direct);
}

#[tokio::test]
async fn console_mode_and_desktop_full_screen_add_the_fullscreen_flags() {
    let mut setup = Setup::new().await;
    let settings = crate::settings::SettingsService::new(setup.store.clone());
    settings
        .register_core(&crate::AppPaths::under(setup.temp.path()))
        .unwrap();
    setup.launches = setup.launches.clone().with_settings(settings.clone());
    setup
        .emulator(
            flatpak(),
            built_in(ArgsOverride::Default, ProfileHooks::default()),
        )
        .await;
    setup.platform_default().await;
    let roms = setup.rom_files(&setup.machine.home.join("ROMs"), &["Vagrant Story.chd"]);
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    let args = async || {
        setup
            .launches
            .resolve(&entry, &LaunchChoice::default())
            .await
            .unwrap()
            .command
            .args
    };

    // Desktop mode plays windowed.
    assert_eq!(&args().await[..3], ["run", FLATPAK_ID, "-L"]);

    // Console mode: the flags go before the profile's arguments.
    let console = serde_json::json!({ "start": "auto", "active": true });
    settings.set("core.console", console, None).await.unwrap();
    assert_eq!(
        &args().await[..4],
        ["run", FLATPAK_ID, "--fullscreen", "-L"]
    );

    // Desktop full screen.
    let desktop = serde_json::json!({ "start": "auto", "active": false });
    settings.set("core.console", desktop, None).await.unwrap();
    let fullscreen = serde_json::json!({ "desktopFullscreen": true });
    settings
        .set("core.emulation", fullscreen, None)
        .await
        .unwrap();
    assert_eq!(&args().await[..3], ["run", FLATPAK_ID, "--fullscreen"]);
}
