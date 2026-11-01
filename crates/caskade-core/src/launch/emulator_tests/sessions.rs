use super::*;
use crate::launch::{
    LaunchCommand, LaunchEvent, LaunchHooks, LaunchService, LaunchTicket,
    test_support::{Events, fast_config},
};
use async_trait::async_trait;
use caskade_emulation_fixtures::{FakeEmulator, Invocation};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// Adds the fake emulator's controls in `launch.before`, as an interceptor
/// rewriting the emulator's command would, and records the payloads.
struct Interceptor {
    env: BTreeMap<String, String>,
    before: std::sync::Mutex<Vec<LaunchCommand>>,
    events: Events,
}

#[async_trait]
impl LaunchHooks for Interceptor {
    async fn before(&self, _launch: &LaunchTicket, command: &mut LaunchCommand) -> Result<bool> {
        self.before.lock().unwrap().push(command.clone());
        command.env.extend(self.env.clone());
        Ok(true)
    }
    async fn started(&self, event: &LaunchEvent) {
        self.events.started(event).await;
    }
    async fn exited(&self, event: &LaunchEvent) {
        self.events.exited(event).await;
    }
    async fn after(&self, event: &LaunchEvent) {
        self.events.after(event).await;
    }
}

fn interceptor(env: &[(&str, String)]) -> Arc<Interceptor> {
    Arc::new(Interceptor {
        env: env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
        before: Default::default(),
        events: Events::default(),
    })
}

fn launch_service(setup: &Setup, hooks: Arc<Interceptor>) -> LaunchService {
    launch_with(setup, setup.launches.clone(), hooks)
}

fn launch_with(
    setup: &Setup,
    launches: EmulatorLaunches,
    hooks: Arc<Interceptor>,
) -> LaunchService {
    LaunchService::new(setup.store.clone())
        .with_hooks(hooks)
        .with_emulators(launches)
        .with_config(crate::launch::LaunchConfig {
            launch_timeout: Duration::from_secs(5),
            ..fast_config()
        })
        .with_aliases(None)
}

#[tokio::test]
async fn ps1_multi_disc_game_launches_through_flatpak_retroarch_with_playtime() {
    let setup = Setup::new().await;
    // `flatpak run` starts the sandboxed `retroarch`, which isn't our child
    // process in a real sandbox: it is found by name.
    let fake = FakeEmulator::new(setup.temp.path()).unwrap();
    fake.copy_to(&setup.temp.path().join("bin/flatpak"))
        .unwrap();
    let retroarch = fake
        .copy_to(&setup.temp.path().join("sandbox/retroarch"))
        .unwrap();
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
    let record = setup.temp.path().join("records/flatpak");
    let hooks = interceptor(&[
        ("CASKADE_FAKE_RECORD", text(&record)),
        ("CASKADE_FAKE_FORK", "wait".into()),
        ("CASKADE_FAKE_CHILD_PROGRAM", text(retroarch.path())),
        ("CASKADE_FAKE_CHILD_SLEEP_MS", "700".into()),
    ]);
    let service = launch_service(&setup, hooks.clone());

    let asked = match service.launch(&entry.id).await {
        Err(LaunchError::NeedsChoice(prompt)) => prompt,
        other => panic!(
            "expected the disc picker, got {:?}",
            other.map(|h| h.ticket)
        ),
    };
    assert_eq!(asked.discs.len(), 2);
    let session = service
        .launch_with(
            &entry.id,
            &LaunchChoice {
                action: None,
                profile: None,
                disc: Some(1),
            },
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let expected = [
        "run".to_owned(),
        format!("--filesystem={}:ro", text(&setup.roms())),
        FLATPAK_ID.into(),
        "-L".into(),
        text(&setup.cores().join("mednafen_psx_hw_libretro.so")),
        text(&roms[1]),
    ];
    assert_eq!(Invocation::read(&record).unwrap().argv, expected);
    let mut child = record.clone().into_os_string();
    child.push(".child");
    assert_eq!(Invocation::read(Path::new(&child)).unwrap().argv, expected);

    // Playtime is the sandboxed emulator's lifetime, recorded in the library.
    assert!(
        (400..5_000).contains(&session.duration_ms),
        "{}",
        session.duration_ms
    );
    let saved = setup
        .store
        .sessions_for_game(&entry.game_id, 10)
        .await
        .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].duration_ms, session.duration_ms);

    // launch.before and launch.after carry the resolved emulator and argv.
    let before = hooks.before.lock().unwrap().clone();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].args, expected);
    let events = hooks.events.0.lock().unwrap().clone();
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["game.started", "game.exited", "launch.after"]);
    let after = events[2].1.command.as_ref().unwrap();
    let emulation = after.emulation.as_ref().unwrap();
    assert_eq!(emulation.platform_id, "sony_playstation");
    assert_eq!(emulation.rom, roms[1]);
    assert_eq!(after.args, expected);
}

#[tokio::test]
async fn fake_emulator_runs_for_every_install_kind_with_its_tracking_default() {
    let setup = Setup::new().await;
    let fake = FakeEmulator::new(setup.temp.path()).unwrap();
    let folder = setup.temp.path().join("RetroArch Folder");
    fake.copy_to(&folder.join("retroarch")).unwrap();
    let bin = setup.temp.path().join("path-bin");
    fake.copy_to(&bin.join("retroarch")).unwrap();
    let appimage = setup
        .temp
        .path()
        .join("Applications/RetroArch-x86_64.AppImage");
    fake.copy_to(&appimage).unwrap();
    let bundle = setup.temp.path().join("Applications/RetroArch.app");
    fake.copy_to(&bundle.join("Contents/MacOS/RetroArch"))
        .unwrap();
    let launches = setup.launches.clone();
    let roms = setup.rom_files(&setup.roms(), &["Vagrant Story.chd"]);
    let core = text(
        &setup
            .machine
            .home
            .join(".config/retroarch/cores/mednafen_psx_hw_libretro.so"),
    );
    let kinds = [
        (
            "folder",
            EmulatorInstall::Folder {
                dir: folder.clone(),
                executable: "retroarch".into(),
            },
            Some(folder.clone()),
        ),
        (
            "path",
            EmulatorInstall::Path {
                binary: "retroarch".into(),
            },
            None,
        ),
        (
            "appimage",
            EmulatorInstall::AppImage { file: appimage },
            None,
        ),
        // OriginalProcess: a waiting launcher's own lifetime is the session.
        ("bundle", EmulatorInstall::AppBundle { bundle }, None),
    ];
    for (name, install, cwd) in kinds {
        setup
            .emulator(
                install,
                built_in(ArgsOverride::Default, ProfileHooks::default()),
            )
            .await;
        let entry = setup
            .game(
                EmulatorTarget::PlatformDefault,
                &roms,
                ArgsOverride::Default,
            )
            .await;
        setup.platform_default().await;
        let record = setup.temp.path().join("records").join(name);
        // The launcher exits after 150 ms and its child after 700 ms:
        // ProcessTree counts the child, OriginalProcess only the launcher.
        let hooks = interceptor(&[
            ("CASKADE_FAKE_RECORD", text(&record)),
            ("CASKADE_FAKE_FORK", "detach".into()),
            ("CASKADE_FAKE_SLEEP_MS", "150".into()),
            ("CASKADE_FAKE_CHILD_SLEEP_MS", "700".into()),
        ]);
        let service = launch_with(&setup, launches.clone(), hooks);
        let session = service
            .launch(&entry.id)
            .await
            .unwrap()
            .wait()
            .await
            .unwrap();
        let invocation = Invocation::read(&record).unwrap();
        assert_eq!(invocation.argv, ["-L", &core, &text(&roms[0])], "{name}");
        if let Some(cwd) = &cwd {
            assert_eq!(&invocation.cwd, &fs::canonicalize(cwd).unwrap(), "{name}");
        }
        if name == "bundle" {
            assert!(session.duration_ms < 450, "{name}: {}", session.duration_ms);
        } else {
            assert!(
                (500..5_000).contains(&session.duration_ms),
                "{name}: {}",
                session.duration_ms
            );
        }
        // Let a detached child finish before the next kind's launch.
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
}

#[tokio::test]
async fn profile_before_command_can_cancel_and_after_exit_runs_when_the_session_ends() {
    let setup = Setup::new().await;
    let fake = FakeEmulator::new(setup.temp.path()).unwrap();
    let dir = setup.temp.path().join("RetroArch");
    fake.copy_to(&dir.join("retroarch")).unwrap();
    let marker = setup.temp.path().join("after-exit");
    let install = EmulatorInstall::Folder {
        dir,
        executable: "retroarch".into(),
    };
    let hooks = |before: &str| ProfileHooks {
        before: Some(vec!["/bin/sh".into(), "-c".into(), before.into()]),
        after_exit: Some(vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf %s \"$1\" > \"$2\"".into(),
            "sh".into(),
            "{RomNameNoExt}".into(),
            text(&marker),
        ]),
    };
    setup
        .emulator(
            install.clone(),
            built_in(ArgsOverride::Default, hooks("exit 3")),
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
    let record = setup.temp.path().join("records/hooks");
    let env = [("CASKADE_FAKE_RECORD", text(&record))];

    let service = launch_service(&setup, interceptor(&env));
    // The failing before-command is named, not reported as a cancel (#160).
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::Refused(reason)) if reason.contains("exit code 3")
    ));
    assert!(Invocation::read(&record).is_err());
    assert!(
        setup
            .store
            .sessions_for_game(&entry.game_id, 10)
            .await
            .unwrap()
            .is_empty()
    );

    setup
        .emulator(install, built_in(ArgsOverride::Default, hooks("exit 0")))
        .await;
    let service = launch_service(&setup, interceptor(&env));
    service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(Invocation::read(&record).is_ok());
    assert_eq!(fs::read_to_string(&marker).unwrap(), "Vagrant Story");
}

#[tokio::test]
async fn changing_a_games_pinned_profile_and_arguments_changes_the_next_launch() {
    let setup = Setup::new().await;
    let fake = FakeEmulator::new(setup.temp.path()).unwrap();
    let dir = setup.temp.path().join("RetroArch");
    fake.copy_to(&dir.join("retroarch")).unwrap();
    setup
        .emulator(
            EmulatorInstall::Folder {
                dir,
                executable: "retroarch".into(),
            },
            built_in(ArgsOverride::Default, ProfileHooks::default()),
        )
        .await;
    let roms = setup.rom_files(&setup.roms(), &["Vagrant Story.chd"]);
    // No platform default: the game can't launch until it is pinned.
    let entry = setup
        .game(
            EmulatorTarget::PlatformDefault,
            &roms,
            ArgsOverride::Default,
        )
        .await;
    let record = setup.temp.path().join("records/pinned");
    let service = launch_service(
        &setup,
        interceptor(&[("CASKADE_FAKE_RECORD", text(&record))]),
    );
    assert!(matches!(
        service.launch(&entry.id).await,
        Err(LaunchError::Invalid(_))
    ));

    setup
        .service
        .set_game(
            &entry.id,
            EmulatorTarget::Profile {
                emulator_id: "ra".into(),
                profile_id: "psx".into(),
            },
            ArgsOverride::Append {
                args: vec!["--fullscreen".into()],
            },
        )
        .await
        .unwrap();
    service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    let core = setup
        .machine
        .home
        .join(".config/retroarch/cores/mednafen_psx_hw_libretro.so");
    assert_eq!(
        Invocation::read(&record).unwrap().argv,
        [
            "-L".to_owned(),
            text(&core),
            text(&roms[0]),
            "--fullscreen".into()
        ]
    );
}
