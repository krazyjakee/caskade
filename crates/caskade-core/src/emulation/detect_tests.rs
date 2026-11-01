use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    emulation::{Availability, DetectJob, DetectStatus, EmulationService},
};
use caskade_emulation_fixtures::{FixtureOs, MachineFixture};
use std::time::Duration;

const PLATFORMS: &str = r#"
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc"]

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue"]

[[platform]]
id = "sony_playstation2"
name = "Sony PlayStation 2"
short = "PS2"
extensions = ["iso"]
"#;

const RETROARCH: &str = r#"
id = "retroarch"
name = "RetroArch"

[detect.windows]
executable = '^retroarch\.exe$'
locations = ["{ProgramFiles}/RetroArch", "{AppData}/RetroArch", "{UserHome}/scoop/apps/retroarch/current"]

[detect.linux]
executable = '^retroarch$'
flatpak = "org.libretro.RetroArch"
path = ["retroarch"]
appimage = '^RetroArch.*\.AppImage$'
locations = ["{UserHome}/Emulation/tools/RetroArch"]

[detect.macos]
bundle = "com.libretro.RetroArch"

[vars]
cores = "{EmulatorDir}/cores"
lib = ".dll"
[vars.linux]
cores = "{XdgConfig}/retroarch/cores"
lib = ".so"
[vars.linux.flatpak]
cores = "{UserHome}/.var/app/org.libretro.RetroArch/config/retroarch/cores"
[vars.macos]
cores = "{AppSupport}/RetroArch/cores"
lib = ".dylib"

[[profile]]
name = "Snes9x"
platforms = ["nintendo_snes"]
extensions = ["sfc"]
args = ["-L", "{cores}/snes9x_libretro{lib}", "{RomPath}"]
requires = ["{cores}/snes9x_libretro{lib}"]

[[profile]]
name = "Beetle PSX HW"
platforms = ["sony_playstation"]
extensions = ["cue"]
args = ["-L", "{cores}/mednafen_psx_hw_libretro{lib}", "{RomPath}"]
requires = ["{cores}/mednafen_psx_hw_libretro{lib}"]
"#;

const PCSX2: &str = r#"
id = "pcsx2"
name = "PCSX2"

[detect.linux]
flatpak = "net.pcsx2.PCSX2"
appimage = '^pcsx2.*\.AppImage$'

[[profile]]
name = "Default"
platforms = ["sony_playstation2"]
extensions = ["iso"]
args = ["-batch", "--", "{RomPath}"]
"#;

const DUCKSTATION: &str = r#"
id = "duckstation"
name = "DuckStation"

[detect.linux]
executable = '^duckstation-qt$'

[[profile]]
name = "Default"
platforms = ["sony_playstation"]
extensions = ["cue"]
args = ["{RomPath}"]
"#;

fn catalog() -> Catalog {
    Catalog::load([
        ("platforms.toml", PLATFORMS),
        ("retroarch.toml", RETROARCH),
        ("pcsx2.toml", PCSX2),
        ("duckstation.toml", DUCKSTATION),
    ])
    .unwrap()
}

fn machine(os: FixtureOs) -> (MachineFixture, HostEnv) {
    let fixture = MachineFixture::new(os).unwrap();
    let env = HostEnv::fixture(fixture.path()).unwrap();
    (fixture, env)
}

fn run(env: &HostEnv, folder: Option<&Path>) -> Vec<DetectedInstall> {
    let ctx = ScanCtx::new(Default::default());
    detect(&catalog(), &[], env, &ctx, folder).unwrap()
}

fn available(found: &DetectedInstall) -> Vec<&str> {
    found
        .profiles
        .iter()
        .filter(|profile| profile.available)
        .map(|profile| profile.name.as_str())
        .collect()
}

#[test]
fn linux_installs_of_every_kind_are_found_with_their_profiles() {
    let (fixture, env) = machine(FixtureOs::Linux);
    let found = run(&env, None);
    let home = &fixture.home;
    let find = |install: &EmulatorInstall| {
        found
            .iter()
            .find(|candidate| candidate.install == *install)
            .unwrap_or_else(|| panic!("{install:?} not in {found:#?}"))
    };

    let flatpak = find(&EmulatorInstall::Flatpak {
        app_id: "org.libretro.RetroArch".into(),
    });
    assert_eq!(available(flatpak), ["Snes9x", "Beetle PSX HW"]);
    let path = find(&EmulatorInstall::Path {
        binary: "retroarch".into(),
    });
    assert_eq!(
        available(path),
        ["Snes9x"],
        "only that core is in ~/.config"
    );
    let folder = find(&EmulatorInstall::Folder {
        dir: home.join("Emulation/tools/RetroArch"),
        executable: "retroarch".into(),
    });
    assert_eq!(folder.definition_id, "retroarch");
    let pcsx2 = find(&EmulatorInstall::AppImage {
        file: home.join("Applications/PCSX2-v2.0.AppImage"),
    });
    assert_eq!(available(pcsx2), ["Default"]);
    // The desktop entry's `retroarch` is the PATH install, found once.
    assert_eq!(
        found
            .iter()
            .filter(|candidate| matches!(candidate.install, EmulatorInstall::Path { .. }))
            .count(),
        1
    );
    // DuckStation lives in no listed location; it needs a user folder.
    assert!(
        !found
            .iter()
            .any(|candidate| candidate.definition_id == "duckstation")
    );
    let with_folder = run(&env, Some(&fixture.root().join("opt/emulators")));
    assert!(with_folder.iter().any(|candidate| candidate.install
        == EmulatorInstall::Folder {
            dir: fixture.root().join("opt/emulators/DuckStation"),
            executable: "duckstation-qt".into(),
        }));
}

#[test]
fn windows_locations_and_macos_bundles_are_found() {
    let (fixture, env) = machine(FixtureOs::Windows);
    let found = run(&env, None);
    let drive = fixture.root().join("drives/c");
    let folders: Vec<(PathBuf, Vec<&str>)> = found
        .iter()
        .filter_map(|candidate| match &candidate.install {
            EmulatorInstall::Folder { dir, .. } => Some((dir.clone(), available(candidate))),
            _ => None,
        })
        .collect();
    for (dir, profiles) in [
        (drive.join("Program Files/RetroArch"), vec!["Snes9x"]),
        (
            drive.join("Users/fixture/AppData/Roaming/RetroArch"),
            vec!["Snes9x"],
        ),
        (
            drive.join("Users/fixture/scoop/apps/retroarch/current"),
            vec![],
        ),
    ] {
        assert!(
            folders.contains(&(dir.clone(), profiles)),
            "{dir:?} in {folders:#?}"
        );
    }

    let (fixture, env) = machine(FixtureOs::Macos);
    let found = run(&env, None);
    let bundle = found
        .iter()
        .find(|candidate| {
            candidate.install
                == EmulatorInstall::AppBundle {
                    bundle: fixture.root().join("Applications/RetroArch.app"),
                }
        })
        .unwrap_or_else(|| panic!("{found:#?}"));
    assert_eq!(bundle.version.as_deref(), Some("1.0.0"));
    assert_eq!(available(bundle), ["Snes9x"]);
}

#[test]
fn simple_executable_regexes_name_shortcut_targets() {
    assert_eq!(
        literal(r"^retroarch\.exe$").as_deref(),
        Some("retroarch.exe")
    );
    assert_eq!(
        literal(r"^duckstation-qt$").as_deref(),
        Some("duckstation-qt")
    );
    assert_eq!(literal(r"^duckstation-qt.*\.exe$"), None);
    assert_eq!(literal(r"retroarch\.exe"), None);
}

async fn finished(service: &EmulationService, id: &str) -> DetectJob {
    for _ in 0..400 {
        let job = service.detect_get(id).unwrap();
        if job.status != DetectStatus::Running {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("detection {id} did not finish");
}

/// #115 acceptance: with Flatpak RetroArch and an AppImage PCSX2, detecting
/// and importing configures both and their platforms with no typing.
#[tokio::test(flavor = "multi_thread")]
async fn flatpak_retroarch_and_appimage_pcsx2_are_configured_with_no_typing() {
    let (fixture, env) = machine(FixtureOs::Linux);
    let store = LibraryStore::in_memory().await.unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let service = EmulationService::new(store.clone(), bus, catalog())
        .with_host(OperatingSystem::Linux, folders(&env))
        .with_host_env(env);

    let started = service.detect_start(None).await.unwrap();
    assert_eq!(started.status, DetectStatus::Running);
    let job = finished(&service, &started.id).await;
    assert_eq!(job.status, DetectStatus::Done, "{job:?}");
    let pick = |install: EmulatorInstall| {
        job.results
            .iter()
            .find(|candidate| candidate.install == install)
            .unwrap()
            .key
            .clone()
    };
    let keys = [
        pick(EmulatorInstall::Flatpak {
            app_id: "org.libretro.RetroArch".into(),
        }),
        pick(EmulatorInstall::AppImage {
            file: fixture.home.join("Applications/PCSX2-v2.0.AppImage"),
        }),
    ];

    // A stale revision is refused, so an import matches what was reviewed.
    assert!(
        service
            .detect_import(&job.id, job.revision + 1, &keys)
            .await
            .is_err()
    );
    let report = service
        .detect_import(&job.id, job.revision, &keys)
        .await
        .unwrap();
    assert_eq!(report.emulators.len(), 2);
    let mut defaults = report.defaults_set.clone();
    defaults.sort();
    assert_eq!(
        defaults,
        ["nintendo_snes", "sony_playstation", "sony_playstation2"]
    );

    let emulators = service.emulators().await.unwrap();
    let retroarch = emulators
        .iter()
        .find(|view| view.emulator.definition.as_deref() == Some("retroarch"))
        .unwrap();
    assert_eq!(retroarch.emulator.name, "RetroArch");
    assert_eq!(retroarch.profiles.len(), 2);
    assert!(
        retroarch
            .profiles
            .iter()
            .all(|profile| profile.availability == Availability::Available),
        "{:#?}",
        retroarch.profiles
    );
    let platforms = service.platforms().await.unwrap();
    let ps2 = platforms
        .iter()
        .find(|view| view.platform.id == "sony_playstation2")
        .unwrap();
    let pcsx2 = emulators
        .iter()
        .find(|view| view.emulator.definition.as_deref() == Some("pcsx2"))
        .unwrap();
    assert_eq!(
        ps2.settings.default.as_ref().unwrap().emulator_id,
        pcsx2.emulator.id
    );
    assert_eq!(service.overview().await.unwrap().platforms_with_default, 3);

    // The job marks them configured, and a new detection agrees.
    let after = service.detect_get(&job.id).unwrap();
    assert!(after.revision > job.revision);
    assert!(
        after
            .results
            .iter()
            .filter(|candidate| keys.contains(&candidate.key))
            .all(|candidate| candidate.configured)
    );
    let again = service.detect_start(None).await.unwrap();
    let again = finished(&service, &again.id).await;
    assert_eq!(
        again
            .results
            .iter()
            .filter(|candidate| candidate.configured)
            .count(),
        2
    );
    // Importing a configured install again adds nothing.
    let report = service
        .detect_import(&again.id, again.revision, &keys)
        .await
        .unwrap();
    assert!(report.emulators.is_empty());

    let mut published = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, crate::bus::Event::EmulationDetectChanged(_)) {
            published += 1;
        }
    }
    assert!(published >= 4, "{published}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_detection_can_be_cancelled() {
    let (_fixture, env) = machine(FixtureOs::Linux);
    let store = LibraryStore::in_memory().await.unwrap();
    let service = EmulationService::new(store, Bus::default(), catalog()).with_host_env(env);
    let job = service.detect_start(None).await.unwrap();
    let cancelled = service.detect_cancel(&job.id).unwrap();
    // Either the cancel won, or detection had already finished.
    assert!(matches!(
        cancelled.status,
        DetectStatus::Cancelled | DetectStatus::Done
    ));
    let job = finished(&service, &job.id).await;
    assert_eq!(job.status, cancelled.status);
    assert!(
        service
            .detect_import(&job.id, job.revision, &[])
            .await
            .is_ok()
            == (job.status == DetectStatus::Done)
    );
    assert!(service.detect_cancel("missing").is_err());
}
