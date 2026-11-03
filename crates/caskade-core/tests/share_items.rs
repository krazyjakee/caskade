//! Smaller things shared once (#248), between cores on loopback that aren't
//! paired: a save arrives as a new version of the same game's saves and
//! never replaces the recipient's own without asking; a web game's offline
//! copy becomes the offline copy of the web game at that address; a ROM
//! arrives as a game of its platform, pinned to the same emulator profile
//! when that emulator is here and otherwise installing it; and a collection
//! arrives as a list, showing which games the recipient has or owns.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use caskade_core::emulation::{
    ArgsOverride, Emulator, EmulatorInstall, EmulatorProfile, EmulatorTarget, PlatformSettings,
    ProfileHooks, ProfileRef, RomLocation,
};
use caskade_core::install::{JobSnapshot, JobStatus};
use caskade_core::ludusavi::Ludusavi;
use caskade_core::model::{
    CollectionKind, ExternalId, Game, GameEntry, InstallLocation, InstallState, LaunchSpec,
    SaveCollection,
};
use caskade_core::saves::Saves;
use caskade_core::saves::versions::{SaveCheck, SaveVersions, VersionReason};
use caskade_core::share::{
    EmulatorInstaller, GameHere, ShareContents, ShareExpiry, ShareItem, ShareState,
};
use caskade_discovery::HostEnv;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::cores::{Core, core, within};

async fn manifest_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/saves/manifest.yaml")),
        )
        .mount(&server)
        .await;
    server
}

/// The link of `item` shared by `core`, once its files are read.
async fn link(core: &Core, item: ShareItem) -> String {
    let share = core
        .shares
        .share_item(item, ShareExpiry::Day, &core.account)
        .await
        .unwrap();
    within("the share is ready", 30, || async {
        core.shares
            .list()
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == share.id && s.state == ShareState::Ready)
    })
    .await;
    core.shares.link(&share.id).await.unwrap()
}

/// Receives `link` on `core` and waits for its job to end.
async fn receive(core: &Core, link: &str) -> JobSnapshot {
    let job = core.shares.receive(link, None).await.unwrap();
    within("the share arrives", 60, || async {
        core.install
            .job(&job.id)
            .is_some_and(|job| job.status.is_finished())
    })
    .await;
    let ended = core.install.job(&job.id).unwrap();
    assert_eq!(ended.status, JobStatus::Completed, "{:?}", ended.error);
    ended
}

/// Title Fixture installed on `core` (its saves under `<base>/title-saves`),
/// with save versions on a Linux machine of its own.
async fn with_saves(core: &Core, manifest: &MockServer) -> (SaveVersions, String, PathBuf) {
    let machine = core.dir.path().join("machine");
    fs::create_dir_all(machine.join("root/home/player")).unwrap();
    fs::write(
        machine.join("machine.toml"),
        "os = 'linux'\n[env]\nHOME = '/home/player'\nUSER = 'player'\n",
    )
    .unwrap();
    let env = HostEnv::fixture(&machine).unwrap();
    let base = env.fs_root().join("games/Title Fixture");
    fs::create_dir_all(base.join("title-saves")).unwrap();
    let game = core
        .store
        .upsert_game(&Game::new("Title Fixture"))
        .await
        .unwrap();
    let mut entry = GameEntry::new(&game.id, "manual".to_owned().into(), "title-fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: base.clone() });
    let entry = core.store.upsert_entry(&entry).await.unwrap();
    let ludusavi = Ludusavi::with_url(core.dir.path().join("ludusavi"), manifest.uri());
    ludusavi.refresh(true).await.unwrap();
    let saves = Saves::with_environment(core.store.clone(), ludusavi, env);
    let versions = SaveVersions::new(saves, core.dir.path().join("blobs"));
    versions.set_settings(core.settings.clone());
    let account = core.account.clone();
    versions.set_account(move || Some(account.clone()));
    core.shares.set_saves(versions.clone());
    (versions, entry.id, base.join("title-saves/slot1.sav"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shared_save_is_a_new_version_and_never_replaces_one_without_asking() {
    let manifest = manifest_server().await;
    let (alex, sam, kim) = (core("Alex").await, core("Sam").await, core("Kim").await);
    let (alex_saves, alex_entry, alex_file) = with_saves(&alex, &manifest).await;
    let (sam_saves, sam_entry, sam_file) = with_saves(&sam, &manifest).await;
    let (kim_saves, kim_entry, kim_file) = with_saves(&kim, &manifest).await;
    fs::write(&alex_file, b"alex reached the summit").unwrap();
    let shared = alex_saves
        .capture(&alex_entry, None, VersionReason::Session)
        .await
        .unwrap()
        .unwrap();
    // Sam has played too; Kim hasn't.
    fs::write(&sam_file, b"sam is on chapter two").unwrap();
    sam_saves
        .capture(&sam_entry, None, VersionReason::Session)
        .await
        .unwrap()
        .unwrap();

    let link = link(
        &alex,
        ShareItem::Save {
            version_id: shared.id.clone(),
        },
    )
    .await;
    let info = sam.shares.inspect(&link).await.unwrap();
    assert_eq!(info.kind, "save");
    let ShareContents::Save { game, here, files } = info.contents else {
        panic!("a save: {:?}", info.contents);
    };
    assert_eq!(game, "Title Fixture");
    assert_eq!(files, 1);
    let sam_game = sam
        .store
        .get_entry(&sam_entry)
        .await
        .unwrap()
        .unwrap()
        .game_id;
    assert_eq!(
        here.as_deref(),
        Some(sam_game.as_str()),
        "found by its title"
    );

    let job = receive(&sam, &link).await;
    assert_eq!(job.game_id.as_deref(), Some(sam_game.as_str()));
    let history = sam_saves.history(&sam_game).await.unwrap();
    let received = history
        .versions
        .iter()
        .find(|state| state.version.reason == VersionReason::Shared)
        .expect("in the game's Saves panel");
    assert_eq!(received.version.content_hash, shared.content_hash);
    assert!(received.complete && !received.current);
    // Two saves now: Sam's stays in place, and the next Play asks.
    assert_eq!(history.conflict.len(), 2);
    assert_eq!(fs::read(&sam_file).unwrap(), b"sam is on chapter two");
    let check = sam_saves.before_play(&sam_entry).await.unwrap().check;
    assert!(matches!(check, SaveCheck::Conflict { .. }), "{check:?}");
    assert_eq!(fs::read(&sam_file).unwrap(), b"sam is on chapter two");
    // Chosen: it's Sam's save.
    sam_saves
        .choose(&sam_entry, &received.version.id)
        .await
        .unwrap();
    assert_eq!(fs::read(&sam_file).unwrap(), b"alex reached the summit");

    // Kim has no save of their own: the shared one is simply theirs.
    receive(&kim, &link).await;
    let check = kim_saves.before_play(&kim_entry).await.unwrap().check;
    assert!(matches!(check, SaveCheck::Restored { .. }), "{check:?}");
    assert_eq!(fs::read(&kim_file).unwrap(), b"alex reached the summit");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_web_games_offline_copy_becomes_the_recipients_offline_copy() {
    let (alex, sam) = (core("Alex").await, core("Sam").await);
    let url = "https://games.example/snake/";
    let folder = alex.games.join("Snake");
    let bundle = caskade_core::web::bundle::Bundle::create(&folder, &url.parse().unwrap()).unwrap();
    bundle
        .put(
            &"https://games.example/snake/index.html".parse().unwrap(),
            200,
            Some("text/html"),
            b"<canvas></canvas>",
        )
        .unwrap();
    bundle.save().unwrap();
    let game = alex.store.upsert_game(&Game::new("Snake")).await.unwrap();
    let root = alex.store.roots().await.unwrap().remove(0);
    let mut entry = GameEntry::new(&game.id, caskade_core::web::WEB_SOURCE, url);
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Web { url: url.into() });
    entry.location = Some(InstallLocation::Managed {
        root_id: root.id,
        rel_dir: "Snake".into(),
    });
    alex.store.upsert_entry(&entry).await.unwrap();

    let link = link(&alex, ShareItem::Web { game_id: game.id }).await;
    let info = sam.shares.inspect(&link).await.unwrap();
    assert!(
        matches!(&info.contents, ShareContents::Web { url: shared, here: None } if shared == url),
        "{:?}",
        info.contents
    );
    let job = receive(&sam, &link).await;
    let entry = sam
        .store
        .entry_by_source(caskade_core::web::WEB_SOURCE.as_str(), url)
        .await
        .unwrap()
        .expect("the web game is in the library");
    assert_eq!(job.game_id.as_deref(), Some(entry.game_id.as_str()));
    assert_eq!(
        sam.store.game(&entry.game_id).await.unwrap().unwrap().title,
        "Snake"
    );
    assert_eq!(entry.launch, Some(LaunchSpec::Web { url: url.into() }));
    let Some(InstallLocation::Managed { rel_dir, .. }) = &entry.location else {
        panic!("an offline copy in Sam's storage: {:?}", entry.location);
    };
    let copy = sam.games.join(rel_dir);
    let reopened = caskade_core::web::bundle::Bundle::open(&copy).unwrap();
    assert!(reopened.has(&"https://games.example/snake/index.html".parse().unwrap()));
}

/// Records the emulators it was asked to install.
#[derive(Default)]
struct Installs(Mutex<Vec<String>>);

#[async_trait]
impl EmulatorInstaller for Installs {
    async fn install(&self, definition: &str) -> caskade_core::Result<()> {
        self.0.lock().unwrap().push(definition.to_owned());
        Ok(())
    }
}

/// An mGBA emulator on `core` with its `default` profile; returns its ids.
async fn mgba(core: &Core) -> (String, String) {
    let saved = core
        .store
        .upsert_emulator(&Emulator {
            id: "mgba-here".into(),
            name: "mGBA".into(),
            definition: Some("mgba".into()),
            install: EmulatorInstall::Path {
                binary: "mgba".into(),
            },
            profiles: vec![EmulatorProfile::BuiltIn {
                id: "mgba-default".into(),
                definition_profile: "default".into(),
                args: ArgsOverride::default(),
                hooks: ProfileHooks::default(),
            }],
        })
        .await
        .unwrap();
    (saved.id.clone(), saved.profiles[0].id().to_owned())
}

fn rom_path(entry: &GameEntry) -> (EmulatorTarget, Vec<PathBuf>) {
    let Some(LaunchSpec::Emulator {
        platform_id,
        target,
        roms,
        ..
    }) = &entry.launch
    else {
        panic!("an emulated game: {:?}", entry.launch);
    };
    assert_eq!(platform_id, "gba");
    let paths = roms
        .iter()
        .map(|rom| match &rom.location {
            RomLocation::Absolute { path } => path.clone(),
            other => panic!("placed by path: {other:?}"),
        })
        .collect();
    (target.clone(), paths)
}

async fn the_rom_game(core: &Core, job: &JobSnapshot) -> GameEntry {
    let game = job.game_id.clone().expect("the game it placed");
    core.store
        .entries_for_game(&game)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| matches!(entry.launch, Some(LaunchSpec::Emulator { .. })))
        .expect("an emulated release")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rom_arrives_with_its_platform_and_emulator_profile() {
    let (alex, sam, kim) = (core("Alex").await, core("Sam").await, core("Kim").await);
    let installs = Arc::new(Installs::default());
    sam.shares.set_emulator_installer(installs.clone());
    kim.shares.set_emulator_installer(installs.clone());
    let (emulator, profile) = mgba(&alex).await;
    alex.store
        .set_platform_settings(&PlatformSettings {
            platform: "gba".into(),
            default: Some(ProfileRef {
                emulator_id: emulator,
                profile_id: profile,
            }),
            bios_dir: None,
        })
        .await
        .unwrap();
    let folder = alex.dir.path().join("ROMs");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("Golden Sun (Europe).gba"), vec![7u8; 40_000]).unwrap();
    let (game, mut entry) = caskade_core::emulation::rom_entry(
        "gba",
        &folder,
        &[PathBuf::from("Golden Sun (Europe).gba")],
        None,
    );
    let game = alex.store.upsert_game(&game).await.unwrap();
    entry.game_id = game.id.clone();
    let entry = alex.store.upsert_entry(&entry).await.unwrap();

    let link = link(&alex, ShareItem::Rom { entry_id: entry.id }).await;
    let info = sam.shares.inspect(&link).await.unwrap();
    assert_eq!(
        info.contents,
        ShareContents::Rom {
            platform: "gba".into(),
            emulator: Some("mgba".into()),
            ready: false,
        }
    );

    // No mGBA here: the game arrives, and emulator setup installs it.
    let job = receive(&sam, &link).await;
    let placed = the_rom_game(&sam, &job).await;
    let (target, paths) = rom_path(&placed);
    assert_eq!(target, EmulatorTarget::PlatformDefault);
    assert_eq!(paths.len(), 1);
    assert!(
        paths[0].starts_with(&sam.games),
        "in Sam's storage: {paths:?}"
    );
    assert_eq!(fs::read(&paths[0]).unwrap(), vec![7u8; 40_000]);
    within("emulator setup is asked for mGBA", 10, || async {
        installs.0.lock().unwrap().as_slice() == ["mgba"]
    })
    .await;
    assert_eq!(
        sam.store
            .game(&placed.game_id)
            .await
            .unwrap()
            .unwrap()
            .title,
        "Golden Sun"
    );

    // Kim has mGBA: it plays with the same profile, and nothing installs.
    let (here, here_profile) = mgba(&kim).await;
    let job = receive(&kim, &link).await;
    let (target, _) = rom_path(&the_rom_game(&kim, &job).await);
    assert_eq!(
        target,
        EmulatorTarget::Profile {
            emulator_id: here,
            profile_id: here_profile,
        }
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(installs.0.lock().unwrap().len(), 1);

    // The same link again on Sam, whose first copy's files moved on into
    // the library (#278): it arrives whole again, as a game of its own.
    let again = the_rom_game(&sam, &receive(&sam, &link).await).await;
    assert_ne!(again.game_id, placed.game_id);
    let (_, paths) = rom_path(&again);
    assert_eq!(fs::read(&paths[0]).unwrap(), vec![7u8; 40_000]);
}

/// The harness's Hades on `core`.
async fn hades_id(core: &Core) -> String {
    core.store
        .entry_by_source("steam", "1145360")
        .await
        .unwrap()
        .unwrap()
        .game_id
}

#[tokio::test(flavor = "multi_thread")]
async fn a_collection_arrives_as_a_list_showing_what_the_recipient_has() {
    let (alex, sam) = (core("Alex").await, core("Sam").await);
    // Alex: Hades (Steam, from the harness), Celeste and Outer Wilds.
    let hades = hades_id(&alex).await;
    let celeste = alex.store.upsert_game(&Game::new("Celeste")).await.unwrap();
    let wilds = alex
        .store
        .upsert_game(&Game::new("Outer Wilds"))
        .await
        .unwrap();
    let collection = alex
        .store
        .save_collection(
            &alex.account,
            &SaveCollection {
                id: None,
                name: "Cosy evenings".into(),
                description: String::new(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    alex.store
        .add_to_collection(
            &alex.account,
            &collection.id,
            &[hades, celeste.id.clone(), wilds.id.clone()],
        )
        .await
        .unwrap();
    // Sam: Hades installed, Celeste owned through a store account.
    let sam_hades = hades_id(&sam).await;
    let mut installed = sam
        .store
        .entries_for_game(&sam_hades)
        .await
        .unwrap()
        .remove(0);
    installed.state = InstallState::Installed;
    sam.store.upsert_entry(&installed).await.unwrap();
    let owned = sam.store.upsert_game(&Game::new("Celeste")).await.unwrap();
    let mut store_entry = GameEntry::new(&owned.id, "store:steam-sam".to_owned().into(), "504230");
    store_entry.ids = vec![ExternalId::Steam { app_id: 504_230 }];
    sam.store.upsert_entry(&store_entry).await.unwrap();

    let link = link(
        &alex,
        ShareItem::Collection {
            collection_id: collection.id,
        },
    )
    .await;
    let info = sam.shares.inspect(&link).await.unwrap();
    let ShareContents::Collection { name, games } = info.contents else {
        panic!("a collection: {:?}", info.contents);
    };
    assert_eq!(name, "Cosy evenings");
    let states: Vec<(&str, GameHere)> = games
        .iter()
        .map(|game| (game.title.as_str(), game.here))
        .collect();
    assert_eq!(
        states,
        [
            ("Hades", GameHere::Installed),
            ("Celeste", GameHere::Owned),
            ("Outer Wilds", GameHere::Missing),
        ]
    );
    // Nothing to download: no job, and the list is a collection of Sam's.
    let received = sam
        .shares
        .receive_collection(&link, &sam.account)
        .await
        .unwrap();
    assert_eq!(received.name, "Cosy evenings");
    assert_eq!(received.game_ids.len(), 3);
    assert_eq!(received.game_ids[0], sam_hades);
    assert_eq!(received.game_ids[1], owned.id);
    let wilds_here = sam
        .store
        .game(&received.game_ids[2])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wilds_here.title, "Outer Wilds");
    assert!(
        sam.store
            .entries_for_game(&wilds_here.id)
            .await
            .unwrap()
            .is_empty(),
        "a title-only record, no files"
    );
    assert!(sam.install.jobs().is_empty());
}
