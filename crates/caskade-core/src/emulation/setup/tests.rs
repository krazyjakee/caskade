//! Installs and updates against a fake GitHub (wiremock).

use super::*;
use crate::{
    bus::BusOptions,
    emulation::{Catalog, EmulatorInstall, OperatingSystem, OsFolders, rom_entry},
};
use std::{io::Write, path::Path, time::Duration};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const PLATFORMS: &str = r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd"]
recommended = ["fakestation"]
bios = [{ file = "scph5501.bin", required = true }]
"#;

const EMULATOR: &str = r#"
id = "fakestation"
name = "FakeStation"

[detect.linux]
executable = "^fakestation$"

[install.linux]
source = "github"
repo = "fake/station"
asset = '^fakestation-[\d.]+-linux\.zip$'

[vars]
bios = "{EmulatorDir}/bios"

[[setup]]
ini = "{EmulatorDir}/settings.ini"
section = "Main"
key = "SetupWizardIncomplete"
value = "false"
[[setup]]
ini = "{EmulatorDir}/settings.ini"
section = "BIOS"
key = "SearchDirectory"
value = "{bios}"
[[setup]]
file = "{EmulatorDir}/portable.txt"

[[profile]]
name = "Default"
platforms = ["sony_playstation"]
extensions = ["cue", "chd"]
args = ["{RomPath}"]
"#;

fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Serves release `tag` of fake/station as the latest, with its zip.
async fn release(server: &MockServer, tag: &str, version: &str) {
    server.reset().await;
    let file = format!("fakestation-{version}-linux.zip");
    Mock::given(method("HEAD"))
        .and(path("/fake/station/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("/fake/station/releases/tag/{tag}")),
        )
        .mount(server)
        .await;
    let assets = format!(
        r#"<ul><li><a href="/fake/station/releases/download/{tag}/fakestation-{version}-windows.zip">w</a></li>
        <li><a href="/fake/station/releases/download/{tag}/{file}" rel="nofollow">l</a></li>
        <li><a href="/fake/station/archive/refs/tags/{tag}.zip">source</a></li></ul>"#
    );
    Mock::given(method("GET"))
        .and(path(format!(
            "/fake/station/releases/expanded_assets/{tag}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_string(assets))
        .mount(server)
        .await;
    let zip = zip_of(&[
        (&format!("fakestation-{version}/fakestation"), version),
        (&format!("fakestation-{version}/resources/ui.dat"), version),
    ]);
    Mock::given(method("GET"))
        .and(path(format!(
            "/fake/station/releases/download/{tag}/{file}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
        .mount(server)
        .await;
}

struct Fixture {
    temp: tempfile::TempDir,
    store: LibraryStore,
    setup: EmulatorSetup,
}

async fn fixture(server: &MockServer) -> Fixture {
    fixture_with(
        server,
        &[
            ("platforms.toml", PLATFORMS),
            ("fakestation.toml", EMULATOR),
        ],
        &["sony_playstation"],
    )
    .await
}

/// `files` as the catalog, with a game for each of `games`' platforms.
async fn fixture_with(server: &MockServer, files: &[(&str, &str)], games: &[&str]) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let catalog = Catalog::load(files.iter().copied()).unwrap();
    let bus = Bus::new(BusOptions::default());
    let emulation = EmulationService::new(store.clone(), bus.clone(), catalog.clone()).with_host(
        OperatingSystem::Linux,
        OsFolders {
            user_home: Some(temp.path().join("home")),
            ..OsFolders::default()
        },
    );
    for platform in games {
        let roms = temp.path().join("roms").join(platform);
        std::fs::create_dir_all(&roms).unwrap();
        std::fs::write(roms.join("Game.cue"), "").unwrap();
        let (game, entry) = rom_entry(platform, &roms, &[roms.join("Game.cue")], Some(&catalog));
        store.upsert_game(&game).await.unwrap();
        store.upsert_entry(&entry).await.unwrap();
    }
    let hosts = Hosts {
        github: GitHub::new(Url::parse(&format!("{}/", server.uri())).unwrap()),
        dolphin: Url::parse(&format!("{}/dolphin/", server.uri())).unwrap(),
    };
    let setup = EmulatorSetup::new(store.clone(), bus, emulation, &paths)
        .with_hosts(hosts, temp.path().join("no-flatpak"));
    Fixture { temp, store, setup }
}

async fn finished(setup: &EmulatorSetup, id: &str) -> SetupJob {
    for _ in 0..200 {
        let job = setup.job(id).unwrap();
        if job.status.finished() {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the install didn't finish");
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[tokio::test]
async fn one_click_installs_configures_and_updates_keeping_the_users_files() {
    let server = MockServer::start().await;
    release(&server, "v1.0", "1.0").await;
    let f = fixture(&server).await;

    let missing = f.setup.missing().await.unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].games, 1);
    let recommended = missing[0].recommended.as_ref().unwrap();
    assert_eq!(recommended.definition_id, "fakestation");

    let job = f.setup.install_for("sony_playstation").await.unwrap();
    let job = finished(&f.setup, &job.id).await;
    assert_eq!(job.status, SetupStatus::Done, "{:?}", job.error);
    assert_eq!(job.version.as_deref(), Some("v1.0"));
    assert_eq!(job.defaults_set, ["sony_playstation"]);
    assert_eq!(job.downloaded, job.total.unwrap());
    // The platform's required BIOS isn't there yet.
    assert!(
        job.warnings[0].contains("scph5501.bin"),
        "{:?}",
        job.warnings
    );

    let dir = f.setup.root().join("fakestation");
    let emulators = f.store.emulators().await.unwrap();
    assert_eq!(emulators.len(), 1);
    assert_eq!(
        emulators[0].install,
        EmulatorInstall::Folder {
            dir: dir.clone(),
            executable: "fakestation".into()
        }
    );
    assert_eq!(emulators[0].profiles.len(), 1);
    assert_eq!(
        read(&dir.join("settings.ini")),
        format!(
            "[Main]\nSetupWizardIncomplete = false\n\n[BIOS]\nSearchDirectory = {}\n",
            dir.join("bios").display()
        )
    );
    assert!(dir.join("portable.txt").is_file());
    assert!(dir.join("bios").is_dir());
    assert!(f.setup.missing().await.unwrap().is_empty());
    let installable = f.setup.installable().await.unwrap();
    let managed = installable[0].managed.as_ref().unwrap();
    assert_eq!(managed.installed.as_deref(), Some("v1.0"));
    assert_eq!(managed.emulator_id, emulators[0].id);
    assert!(installable[0].updates);

    // The user's saves and BIOS stay; the release's files are replaced.
    std::fs::write(dir.join("bios/scph5501.bin"), "bios").unwrap();
    std::fs::write(dir.join("settings.ini"), "[Main]\nTheme = Mine\n").unwrap();
    release(&server, "v1.1", "1.1").await;
    let id = emulators[0].id.clone();
    let checked = f.setup.check(&id).await.unwrap();
    assert_eq!(checked.latest.as_deref(), Some("v1.1"));
    let offered = |list: Vec<Installable>| list[0].update_available;
    assert!(offered(f.setup.installable().await.unwrap()));
    f.setup.skip(&id, None).await.unwrap();
    assert!(!offered(f.setup.installable().await.unwrap()));
    // Checks are due once the interval has passed since the last one.
    assert_eq!(f.setup.check_due().await.unwrap(), 0);
    let job = f.setup.update(&id).await.unwrap();
    let job = finished(&f.setup, &job.id).await;
    assert_eq!(job.status, SetupStatus::Done, "{:?}", job.error);
    assert_eq!(job.version.as_deref(), Some("v1.1"));
    assert!(job.defaults_set.is_empty(), "the default still works");
    assert!(job.warnings.is_empty(), "{:?}", job.warnings);
    assert_eq!(read(&dir.join("fakestation")), "1.1");
    assert_eq!(read(&dir.join("bios/scph5501.bin")), "bios");
    // Setup runs again, keeping the user's other settings.
    assert!(
        read(&dir.join("settings.ini"))
            .starts_with("[Main]\nTheme = Mine\nSetupWizardIncomplete = false\n")
    );
    let after = f.store.emulators().await.unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, emulators[0].id);
    assert!(
        !f.setup
            .root()
            .join(".old")
            .read_dir()
            .unwrap()
            .any(|_| true)
    );
    let installable = f.setup.installable().await.unwrap();
    assert_eq!(installable[0].installed_label.as_deref(), Some("v1.1"));
    assert!(!installable[0].update_available);
    assert!(!f.temp.path().join("cache/emulators").join(&job.id).exists());
}

#[tokio::test]
async fn a_release_without_this_systems_file_fails_the_job() {
    let server = MockServer::start().await;
    release(&server, "v1.0", "1.0").await;
    Mock::given(method("GET"))
        .and(path("/fake/station/releases/expanded_assets/v1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<ul></ul>"))
        .with_priority(1)
        .mount(&server)
        .await;
    let f = fixture(&server).await;
    let job = f.setup.install("fakestation").await.unwrap();
    let job = finished(&f.setup, &job.id).await;
    assert_eq!(job.status, SetupStatus::Failed);
    assert!(job.error.unwrap().contains("no file for this system"));
    assert!(f.store.emulators().await.unwrap().is_empty());
    assert!(!f.setup.root().join("fakestation").exists());
}

/// A RetroArch-like emulator: one release, cores fetched per platform.
const CORES_PLATFORMS: &str = r#"
[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue"]
recommended = ["fakearch:Beetle PSX"]

[[platform]]
id = "nintendo_snes"
name = "Super Nintendo"
short = "SNES"
extensions = ["cue"]
recommended = ["fakearch:Snes9x"]
"#;

fn cores_emulator(server: &MockServer) -> String {
    format!(
        r#"
id = "fakearch"
name = "FakeArch"

[detect.linux]
executable = "^fakearch$"

[install.linux]
source = "github"
repo = "fake/arch"
asset = '^fakearch-[\d.]+-linux\.zip$'

[requires_from]
linux = "{}/cores/{{arch}}/{{file}}.zip"

[vars]
cores = "{{EmulatorDir}}/cores"
lib = ".so"

[[profile]]
name = "Beetle PSX"
platforms = ["sony_playstation"]
extensions = ["cue"]
args = ["-L", "{{cores}}/beetle_libretro{{lib}}", "{{RomPath}}"]
requires = ["{{cores}}/beetle_libretro{{lib}}"]

[[profile]]
name = "Snes9x"
platforms = ["nintendo_snes"]
extensions = ["cue"]
args = ["-L", "{{cores}}/snes9x_libretro{{lib}}", "{{RomPath}}"]
requires = ["{{cores}}/snes9x_libretro{{lib}}"]
"#,
        server.uri()
    )
}

#[tokio::test]
async fn a_platform_gets_its_core_and_reuses_the_emulator_installed_for_another() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/fake/arch/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "/fake/arch/releases/tag/v1.0"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fake/arch/releases/expanded_assets/v1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<a href="/fake/arch/releases/download/v1.0/fakearch-1.0-linux.zip">l</a>"#,
        ))
        .mount(&server)
        .await;
    // Downloaded once: the second platform keeps the install.
    Mock::given(method("GET"))
        .and(path(
            "/fake/arch/releases/download/v1.0/fakearch-1.0-linux.zip",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_of(&[("fakearch", "1.0")])))
        .expect(1)
        .mount(&server)
        .await;
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x86_64"
    };
    for core in ["beetle", "snes9x"] {
        let file = format!("{core}_libretro.so");
        Mock::given(method("GET"))
            .and(path(format!("/cores/{arch}/{file}.zip")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_of(&[(&file, core)])))
            .expect(1)
            .mount(&server)
            .await;
    }
    let emulator = cores_emulator(&server);
    let f = fixture_with(
        &server,
        &[
            ("platforms.toml", CORES_PLATFORMS),
            ("fakearch.toml", &emulator),
        ],
        &["sony_playstation", "nintendo_snes"],
    )
    .await;
    let mut names: Vec<String> = f
        .setup
        .missing()
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.recommended.unwrap().name)
        .collect();
    names.sort();
    assert_eq!(names, ["FakeArch (Beetle PSX)", "FakeArch (Snes9x)"]);

    let job = f.setup.install_for("sony_playstation").await.unwrap();
    let job = finished(&f.setup, &job.id).await;
    assert_eq!(job.status, SetupStatus::Done, "{:?}", job.error);
    assert_eq!(job.defaults_set, ["sony_playstation"]);
    let cores = f.setup.root().join("fakearch/cores");
    assert_eq!(read(&cores.join("beetle_libretro.so")), "beetle");
    assert!(!cores.join("snes9x_libretro.so").exists());
    assert_eq!(f.store.emulators().await.unwrap()[0].profiles.len(), 1);

    let job = f.setup.install_for("nintendo_snes").await.unwrap();
    let job = finished(&f.setup, &job.id).await;
    assert_eq!(job.status, SetupStatus::Done, "{:?}", job.error);
    assert_eq!(job.defaults_set, ["nintendo_snes"]);
    assert_eq!(read(&cores.join("snes9x_libretro.so")), "snes9x");
    let emulators = f.store.emulators().await.unwrap();
    assert_eq!(emulators.len(), 1);
    assert_eq!(emulators[0].profiles.len(), 2);
    assert!(f.setup.missing().await.unwrap().is_empty());
}
