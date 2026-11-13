use super::*;
use crate::{
    emulation::{ArgsOverride, EmulatorTarget, GameRom, RomLocation},
    model::{Game, GameEntry},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

const SHA: &str = "de8b19cdb4e3ad0c10fd6d8127f7a6cc2a9eba106d89c753439178b718887c42";

/// A small catalogue in the published shape.
fn plans() -> serde_json::Value {
    let release = |asset: &str, exe: &str, files: serde_json::Value| {
        serde_json::json!({
            "redirect_repo": null,
            "games": [{
                "game_title": "The Legend of Zelda: Majora's Mask",
                "console": "Nintendo 64",
                "build": {
                    "method": "release", "release_asset": asset, "release_tag": "v1.2.2",
                    "build_steps": [], "required_tools": [], "executable": exe,
                    "scanned": { "sha256": SHA }
                },
                "game_files": files,
                "notes": ["Only the US version is supported"],
                "unsure": [],
                "confidence": "high"
            }]
        })
    };
    let picker = serde_json::json!([{
        "label": "Majora's Mask US ROM", "kind": "game", "how_provided": "in_app_picker",
        "accepted_formats": [".z64"], "expected_region": "USA",
        "expected_hashes": [{ "algo": "sha1", "value": "D6133ACE5AFAA0882CF214CF88DABA39E266C078" }]
    }]);
    serde_json::json!({
        "updatedAt": "2026-11-01T00:00:00Z",
        "plans": {
            "github:zelda64recomp/zelda64recomp": {
                "tag": "v1.2.2",
                "targets": {
                    "windows": release("Zelda64Recompiled-Windows.zip", "Zelda64Recompiled.exe", picker.clone()),
                    "linux": release("Zelda64Recompiled-Linux-X64.zip", "Zelda64Recompiled", picker.clone()),
                    "macos-arm64": release("Zelda64Recompiled-macOS.zip", "Zelda64Recompiled", picker),
                }
            },
            "github:example/mm-folder-port": {
                "targets": {
                    "linux": {
                        "games": [{
                            "game_title": "Majora's Mask",
                            "console": "Nintendo 64",
                            "build": { "method": "release", "release_asset": "port linux.zip", "release_tag": "1.0", "executable": "mm.elf" },
                            "game_files": [{
                                "label": "Majora's Mask ROM", "kind": "game", "how_provided": "folder",
                                "drop_into": "roms", "expected_filename": "baserom.us.z64"
                            }]
                        }]
                    },
                    "windows": {
                        "games": [{
                            "game_title": "Majora's Mask",
                            "console": "Nintendo 64",
                            "build": { "method": "source", "release_asset": null, "release_tag": null, "executable": null },
                            "game_files": []
                        }]
                    }
                }
            },
            "github:example/quake-port": {
                "targets": {
                    "linux": { "games": [{
                        "game_title": "Quake", "console": "PC (MS-DOS)",
                        "build": { "method": "release", "release_asset": "quake.tar.gz", "release_tag": "r1", "executable": "quake" },
                        "game_files": [{ "label": "Quake game data (id1 folder)", "kind": "game", "how_provided": "folder" }]
                    }]},
                    "windows": { "games": [{
                        "game_title": "Quake", "console": "PC (MS-DOS)",
                        "build": { "method": "release", "release_asset": "quake.zip", "release_tag": "r1", "executable": "quake.exe" },
                        "game_files": []
                    }]}
                }
            },
            "github:example/snes-port": {
                "targets": { "linux": { "games": [{
                    "game_title": "Majora's Mask", "console": "Super Nintendo",
                    "build": { "method": "release", "release_asset": "a.zip", "release_tag": "1", "executable": "a" },
                    "game_files": []
                }]}}
            },
            "github:../escape": { "targets": {} },
            "github:broken/plan": { "targets": "not a map" },
            "bitbucket:someone/else": { "targets": {} }
        }
    })
}

#[test]
fn plans_parse_leniently() {
    let catalogue = parse(&serde_json::to_vec(&plans()).unwrap()).unwrap();
    let keys: Vec<&str> = catalogue
        .projects
        .iter()
        .map(|project| project.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "github:example/mm-folder-port",
            "github:example/quake-port",
            "github:example/snes-port",
            "github:zelda64recomp/zelda64recomp",
        ]
    );
    let zelda = &catalogue.projects[3];
    assert_eq!(zelda.games.len(), 1, "one game across three systems");
    let game = &zelda.games[0];
    assert_eq!(game.families, BTreeSet::from(["nintendo_n64".to_owned()]));
    assert!(game.keys.contains("legend of zelda majoras mask"));
    assert!(game.keys.contains("majoras mask"), "from the file's label");
    let linux = &game.targets["linux"];
    assert!(linux.installable());
    assert_eq!(linux.sha256.as_deref(), Some(SHA));
    assert_eq!(
        linux.files[0].sha1,
        ["d6133ace5afaa0882cf214cf88daba39e266c078"]
    );
    assert!(parse(b"[]").is_err(), "not a catalogue at all");
}

#[test]
fn hasheous_spellings_cover_no_intro_names() {
    assert_eq!(
        spellings("The Legend of Zelda: Majora's Mask"),
        [
            "The Legend of Zelda: Majora's Mask",
            "The Legend of Zelda - Majora's Mask",
            "Legend of Zelda, The - Majora's Mask",
        ]
    );
    assert_eq!(spellings("Quake"), ["Quake"]);
}

#[test]
fn release_urls_are_github_downloads_with_their_checksum() {
    let catalogue = parse(&serde_json::to_vec(&plans()).unwrap()).unwrap();
    let zelda = &catalogue.projects[3];
    assert_eq!(
        install::release_url(zelda, &zelda.games[0].targets["linux"]).unwrap(),
        format!(
            "https://github.com/zelda64recomp/zelda64recomp/releases/download/v1.2.2/Zelda64Recompiled-Linux-X64.zip#sha256={SHA}"
        )
    );
    let folder = &catalogue.projects[0];
    assert_eq!(
        install::release_url(folder, &folder.games[0].targets["linux"]).unwrap(),
        "https://github.com/example/mm-folder-port/releases/download/1.0/port%20linux.zip"
    );
}

async fn seeded(dir: &std::path::Path) -> Ports {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("plans.json"),
        serde_json::to_vec(&plans()).unwrap(),
    )
    .unwrap();
    Ports::new(dir)
}

/// A library game with an emulated N64 release.
async fn majoras_mask(store: &LibraryStore, rom: &std::path::Path) -> Game {
    let game = store
        .upsert_game(&Game::new("The Legend of Zelda: Majora's Mask"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, crate::install::HANDOFF_SOURCE, "mm-rom");
    entry.launch = Some(LaunchSpec::Emulator {
        platform_id: "nintendo_n64".into(),
        target: EmulatorTarget::PlatformDefault,
        roms: vec![GameRom {
            location: RomLocation::Absolute {
                path: rom.to_path_buf(),
            },
            name: None,
            size: None,
        }],
        args: ArgsOverride::Default,
    });
    store.upsert_entry(&entry).await.unwrap();
    game
}

#[tokio::test]
async fn games_list_the_ports_of_their_own_system() {
    let temp = tempfile::tempdir().unwrap();
    let ports = seeded(&temp.path().join("ports")).await;
    let store = LibraryStore::in_memory().await.unwrap();
    let rom = temp.path().join("Majora's Mask (USA).z64");
    let game = majoras_mask(&store, &rom).await;
    let found = ports.for_game(&store, None, &game.id).await.unwrap();
    assert!(found.status.available && found.status.enabled);
    assert_eq!(found.status.projects, 4);
    let names: Vec<&str> = found.ports.iter().map(|port| port.name.as_str()).collect();
    // Not the SNES project of the same name, nor the PC game's.
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"zelda64recomp") && names.contains(&"mm-folder-port"));
    let zelda = found
        .ports
        .iter()
        .find(|port| port.name == "zelda64recomp")
        .unwrap();
    assert_eq!(zelda.matched_by, PortMatch::Title);
    assert_eq!(zelda.url, "https://github.com/zelda64recomp/zelda64recomp");
    assert_eq!(zelda.runs_on, ["linux", "macos-arm64", "windows"]);
    assert_eq!(zelda.needs.len(), 1);
    assert_eq!(zelda.needs[0].region.as_deref(), Some("USA"));
    assert_eq!(zelda.needs[0].how, PortFileHow::InAppPicker);
    assert!(zelda.needs[0].have, "the library has its ROM");
    assert!(zelda.entry_id.is_none());
    if this_system().is_some() {
        assert!(matches!(
            &zelda.here,
            PortHere::Installable { release, checked: true, .. } if release == "v1.2.2"
        ));
    }

    let pc = store.upsert_game(&Game::new("Quake")).await.unwrap();
    let mut entry = GameEntry::new(&pc.id, crate::install::STORAGE_SOURCE, "quake");
    entry.launch = Some(LaunchSpec::Executable {
        path: "quake.exe".into(),
        args: vec![],
        cwd: None,
        env: Default::default(),
    });
    store.upsert_entry(&entry).await.unwrap();
    let found = ports.for_game(&store, None, &pc.id).await.unwrap();
    let names: Vec<&str> = found.ports.iter().map(|port| port.name.as_str()).collect();
    assert_eq!(names, ["quake-port"]);
}

#[tokio::test]
async fn nothing_is_listed_while_ports_are_off_or_not_downloaded() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let game = majoras_mask(&store, &temp.path().join("rom.z64")).await;
    let ports = Ports::new(temp.path().join("empty"));
    let found = ports.for_game(&store, None, &game.id).await.unwrap();
    assert!(!found.status.available);
    assert!(found.ports.is_empty());
    let ports = seeded(&temp.path().join("ports")).await;
    *crate::lock(&ports.inner.enabled) = false;
    let found = ports.for_game(&store, None, &game.id).await.unwrap();
    assert!(!found.status.enabled);
    assert!(found.ports.is_empty());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn installing_places_the_rom_where_the_plan_wants_it() {
    use crate::install::{AttachTo, PlacedFile};

    let temp = tempfile::tempdir().unwrap();
    let ports = seeded(&temp.path().join("ports")).await;
    let store = LibraryStore::in_memory().await.unwrap();
    let rom = temp.path().join("Majora's Mask (USA).z64");
    let game = majoras_mask(&store, &rom).await;
    let plan = ports
        .plan_install(&store, &game.id, "github:example/mm-folder-port")
        .await
        .unwrap();
    assert_eq!(
        plan.url,
        "https://github.com/example/mm-folder-port/releases/download/1.0/port%20linux.zip"
    );
    assert_eq!(
        plan.hints.installer_title.as_deref(),
        Some("mm-folder-port")
    );
    assert_eq!(
        plan.attach,
        AttachTo {
            game_id: game.id.clone(),
            executable: Some("mm.elf".into()),
            files: vec![PlacedFile {
                source: rom.clone(),
                into: "roms".into(),
                name: Some("baserom.us.z64".into()),
            }],
            args: vec![],
        }
    );
    let plan = ports
        .plan_install(&store, &game.id, "github:zelda64recomp/zelda64recomp")
        .await
        .unwrap();
    assert!(plan.url.ends_with(&format!("#sha256={SHA}")));
    assert!(plan.attach.files.is_empty(), "it asks for the ROM itself");
    let error = ports
        .plan_install(&store, &game.id, "github:example/snes-port")
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("no release for this system"),
        "{error}"
    );
}

#[tokio::test]
async fn refreshes_download_once_a_day_and_revalidate_with_the_etag() {
    let server = MockServer::start().await;
    let body = serde_json::to_vec(&plans()).unwrap();
    Mock::given(method("GET"))
        .and(path("/plans.json"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/plans.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_bytes(body),
        )
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let ports = Ports::with_url(temp.path(), format!("{}/plans.json", server.uri()));
    assert_eq!(ports.refresh(false).await.unwrap(), Refresh::Offline);
    assert_eq!(
        ports.refresh(true).await.unwrap(),
        Refresh::Updated { projects: 4 }
    );
    assert_eq!(ports.refresh(true).await.unwrap(), Refresh::Fresh);
    // A day later the server says nothing changed.
    let mut state: Cached =
        serde_json::from_slice(&std::fs::read(temp.path().join("state.json")).unwrap()).unwrap();
    state.checked -= DAY.as_secs() + 1;
    std::fs::write(
        temp.path().join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert_eq!(ports.refresh(true).await.unwrap(), Refresh::Unchanged);
    assert_eq!(ports.status().await.unwrap().projects, 4);
    // A restart reads the plans from disk.
    let again = Ports::with_url(temp.path(), format!("{}/plans.json", server.uri()));
    assert_eq!(again.catalogue().await.unwrap().unwrap().projects.len(), 4);
}

#[tokio::test]
async fn a_bad_download_keeps_the_last_catalogue() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let ports = seeded(temp.path()).await;
    *crate::lock(&ports.inner.url) = format!("{}/plans.json", server.uri());
    assert!(ports.refresh(true).await.is_err());
    assert_eq!(ports.status().await.unwrap().projects, 4);
}

/// The published catalogue parses, with a port everyone knows installable.
#[tokio::test]
#[ignore = "fetches Decomp Buddy's published plans"]
async fn live_catalogue_parses() {
    let temp = tempfile::tempdir().unwrap();
    let ports = Ports::new(temp.path());
    let refreshed = ports.refresh(true).await.unwrap();
    assert!(
        matches!(refreshed, Refresh::Updated { projects } if projects > 1000),
        "{refreshed:?}"
    );
    let catalogue = ports.catalogue().await.unwrap().unwrap();
    let games: Vec<&PortGame> = catalogue
        .projects
        .iter()
        .flat_map(|project| &project.games)
        .collect();
    let unknown = games.iter().filter(|game| game.families.is_empty()).count();
    eprintln!(
        "{} projects, {} games, {unknown} on systems without a family",
        catalogue.projects.len(),
        games.len()
    );
    assert!(unknown * 10 < games.len(), "most systems are known");
    let zelda = catalogue
        .projects
        .iter()
        .find(|project| project.key == "github:zelda64recomp/zelda64recomp")
        .expect("Zelda64Recomp is listed");
    assert!(zelda.games.iter().any(|game| {
        game.families.contains("nintendo_n64")
            && game.targets.get("linux").is_some_and(Target::installable)
    }));
}

/// A real port installs from its pinned, checked release into the game,
/// and a wrong checksum is refused.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads Zelda64Recomp's Linux release from GitHub"]
async fn live_a_port_installs_into_the_game() {
    use crate::install::{InstallOptions, InstallService, JobStatus};
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("Games");
    std::fs::create_dir_all(&root_path).unwrap();
    let identity = caskade_installer::manifest::RootManifest::new("root-live-port").unwrap();
    identity.write(&root_path).unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let mut root = crate::model::StorageRoot::from_manifest(
        std::fs::canonicalize(&root_path).unwrap(),
        &identity,
    );
    root.is_default = true;
    store.upsert_root(&root).await.unwrap();
    let game = majoras_mask(&store, &temp.path().join("Majora's Mask (USA).z64")).await;
    let ports = Ports::new(temp.path().join("ports"));
    ports.refresh(true).await.unwrap();
    let installs = InstallService::new(
        store.clone(),
        crate::bus::Bus::default(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    installs.set_tracker(crate::updates::Tracker::builtin(store.clone()));
    let finished = |id: String| {
        let installs = installs.clone();
        async move {
            for _ in 0..2400 {
                let job = installs.job(&id).unwrap();
                if job.status.is_finished() || job.status == JobStatus::NeedsUser {
                    return job;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            panic!("{id} didn't finish");
        }
    };
    let project = "github:zelda64recomp/zelda64recomp";
    let mut plan = ports.plan_install(&store, &game.id, project).await.unwrap();
    assert!(plan.url.contains("#sha256="), "{}", plan.url);
    // A checksum that doesn't match is refused.
    let good = plan.url.clone();
    plan.url = format!(
        "{}#sha256={}",
        good.split('#').next().unwrap(),
        "0".repeat(64)
    );
    let bad = installs
        .install_attached(
            plan.url.clone(),
            None,
            plan.hints.clone(),
            plan.attach.clone(),
        )
        .await
        .unwrap();
    let bad = finished(bad.id).await;
    assert_eq!(bad.status, JobStatus::Failed, "{bad:?}");

    let job = ports
        .install(&store, &installs, &game.id, project, None)
        .await
        .unwrap();
    let job = finished(job.id).await;
    assert_eq!(job.status, JobStatus::Completed, "{job:?}");
    assert_eq!(job.game_id.as_deref(), Some(game.id.as_str()));
    let found = ports.for_game(&store, None, &game.id).await.unwrap();
    let zelda = found
        .ports
        .iter()
        .find(|port| port.project == project)
        .unwrap();
    let entry = zelda
        .entry_id
        .clone()
        .expect("installed and followed for updates");
    let entry = store.get_entry(&entry).await.unwrap().unwrap();
    assert!(
        matches!(&entry.launch, Some(LaunchSpec::Executable { path, .. })
            if path.file_name().is_some_and(|name| name == "Zelda64Recompiled")),
        "{:?}",
        entry.launch
    );
}
