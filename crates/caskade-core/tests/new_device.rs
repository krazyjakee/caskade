//! A new device set up from another Caskade (#242), between two cores on
//! loopback (one socket on 127.0.0.1, no relay and no address lookup: a
//! network with no internet). After pairing and the first sync, the new
//! device has the household's library, accounts and collections, and keeps
//! its own storage, emulators and hardware settings.

use std::sync::Arc;
use std::time::{Duration, Instant};

use caskade_core::accounts::{
    AccountAccess, AccountRole, AccountService, AccountUpdate, NewAccount,
};
use caskade_core::bus::Bus;
use caskade_core::db::LibraryStore;
use caskade_core::devices::{Devices, MemoryKeyStore, Reach};
use caskade_core::emulation::{Emulator, EmulatorInstall};
use caskade_core::model::{
    CollectionKind, ExternalId, Game, GameEntry, InstallState, SaveCollection, SourceId,
    StorageRoot,
};
use caskade_core::paths::AppPaths;
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncService;
use serde_json::{Value, json};

struct Core {
    store: LibraryStore,
    accounts: AccountService,
    settings: SettingsService,
    devices: Devices,
    sync: SyncService,
    _temp: tempfile::TempDir,
}

impl Core {
    async fn id(&self) -> String {
        self.devices.status().await.this_device.id.unwrap()
    }

    async fn setting(&self, namespace: &str, field: &str) -> Value {
        self.settings.get(namespace).await.unwrap().value[field].clone()
    }

    async fn set(&self, namespace: &str, field: &str, value: Value) {
        let mut section = self.settings.get(namespace).await.unwrap().value;
        section[field] = value;
        self.settings.set(namespace, section, None).await.unwrap();
    }

    async fn titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = self
            .store
            .list_games(&Default::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|game| game.title)
            .collect();
        titles.sort();
        titles
    }
}

async fn core(name: &str) -> Core {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(temp.path()))
        .unwrap();
    let devices = Devices::with_reach(
        store.clone(),
        accounts.clone(),
        Arc::new(MemoryKeyStore::default()),
        Reach::loopback(),
    );
    devices.set_settings(settings.clone());
    devices.start();
    let sync = SyncService::new(store.clone(), devices.clone());
    sync.set_settings(settings.clone());
    // As the app does: this device's untouched admin joins the household's.
    let joined = accounts.clone();
    sync.on_account_joined(move |from, into| {
        let joined = joined.clone();
        tokio::spawn(async move { joined.joined(&from, &into).await });
    });
    let core = Core {
        store,
        accounts,
        settings,
        devices,
        sync,
        _temp: temp,
    };
    within("the endpoint starts", 20, || async {
        core.devices.status().await.this_device.listening
    })
    .await;
    core.devices.rename(name).await.unwrap();
    core
}

async fn within<F, Fut>(what: &str, seconds: u64, check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !check().await {
        assert!(Instant::now() < deadline, "{what} within {seconds} s");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The household's desktop: a library, two accounts (the admin with a
/// PIN), a collection, and its own storage, emulator and hardware settings.
async fn desktop() -> Core {
    let desk = core("Desktop").await;
    let player = desk.accounts.list().await.unwrap().remove(0);
    desk.accounts
        .update(
            &player.id,
            AccountUpdate {
                name: "Ada".into(),
                avatar: player.avatar.clone(),
                access: None,
            },
        )
        .await
        .unwrap();
    desk.accounts
        .set_pin(&player.id, Some("2468".into()))
        .await
        .unwrap();
    desk.accounts
        .create(NewAccount {
            name: "Kid".into(),
            avatar: "avatar:moon".into(),
            access: AccountAccess {
                role: AccountRole::Standard,
                permissions: Default::default(),
                max_age: Some(12),
                unrated_needs_pin: false,
            },
            pin: None,
        })
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (title, app_id) in [
        ("Hades", 1_145_360),
        ("Celeste", 504_230),
        ("Outer Wilds", 753_640),
    ] {
        let game = desk.store.upsert_game(&Game::new(title)).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), app_id.to_string());
        entry.ids = vec![ExternalId::Steam { app_id }];
        entry.state = InstallState::Installed;
        desk.store.upsert_entry(&entry).await.unwrap();
        ids.push(game.id);
    }
    let collection = desk
        .store
        .save_collection(
            &player.id,
            &SaveCollection {
                id: None,
                name: "Roguelikes".into(),
                description: "One more run".into(),
                kind: CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    desk.store
        .add_to_collection(&player.id, &collection.id, &ids[..1])
        .await
        .unwrap();
    // What belongs to the desktop alone.
    desk.store
        .upsert_root(&StorageRoot {
            id: "desktop-games".into(),
            path: "/mnt/games".into(),
            is_default: true,
            label: Some("Games drive".into()),
        })
        .await
        .unwrap();
    desk.store
        .upsert_emulator(&Emulator {
            id: "desk-dolphin".into(),
            name: "Dolphin".into(),
            definition: Some("dolphin".into()),
            install: EmulatorInstall::Folder {
                dir: "/opt/dolphin".into(),
                executable: "dolphin-emu".into(),
            },
            profiles: Vec::new(),
        })
        .await
        .unwrap();
    desk.set("core.console", "active", json!(true)).await;
    desk.set("core.appearance", "uiScale", json!(125)).await;
    // And a household choice that travels.
    desk.set("core.appearance", "density", json!("compact"))
        .await;
    desk
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_set_up_from_another_has_its_library_and_keeps_its_own_hardware() {
    let desk = desktop().await;
    let deck = core("Deck").await;
    // Its own storage, set during setup before pairing.
    deck.store
        .upsert_root(&StorageRoot {
            id: "deck-sd".into(),
            path: "/run/media/deck/sd".into(),
            is_default: true,
            label: Some("SD card".into()),
        })
        .await
        .unwrap();
    assert!(
        deck.accounts.active_id().is_some(),
        "a fresh install signs its admin in"
    );

    // Set up from another device: a pairing link from the desktop, accepted
    // on the Deck, then the first sync.
    let link = desk.devices.pairing_link().await.unwrap();
    deck.devices.accept_pairing_link(&link.url).await.unwrap();
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let desk_id = desk.id().await;
    deck.sync.sync_now(&desk_id);
    within("the Deck has the household's library", 30, || async {
        deck.titles().await == ["Celeste", "Hades", "Outer Wilds"]
    })
    .await;

    // Accounts, with the admin's PIN: the Deck's own untouched admin joined
    // Ada, and the PIN is asked for here (nobody stays signed in).
    within("the accounts arrive", 30, || async {
        let names: Vec<String> = deck
            .accounts
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|account| account.name)
            .collect();
        names.contains(&"Ada".to_owned()) && names.contains(&"Kid".to_owned())
    })
    .await;
    let ada = deck
        .accounts
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|account| account.name == "Ada")
        .unwrap();
    assert!(ada.has_pin);
    assert_eq!(ada.role, AccountRole::Admin);
    within("the Deck's stand-in joins Ada", 30, || async {
        deck.accounts.active_id().is_none()
    })
    .await;

    // Collections, with their games.
    within("Ada's collection arrives", 30, || async {
        deck.store
            .collection_summaries(&ada.id)
            .await
            .unwrap()
            .iter()
            .any(|collection| collection.name == "Roguelikes" && collection.games == 1)
    })
    .await;
    assert_eq!(deck.setting("core.appearance", "density").await, "compact");

    // Its own storage, emulators and hardware settings.
    let roots = deck.store.roots().await.unwrap();
    assert_eq!(
        roots
            .iter()
            .map(|root| root.id.as_str())
            .collect::<Vec<_>>(),
        ["deck-sd"]
    );
    assert!(deck.store.emulators().await.unwrap().is_empty());
    assert_eq!(deck.setting("core.console", "active").await, false);
    assert_eq!(deck.setting("core.appearance", "uiScale").await, 100);
}

/// #276: after the first sync, the new device lists the other device's
/// installed games it could copy here, with their sizes: a game in a
/// folder of its own, and a Windows build (through Proton on Linux), but
/// not a launcher's install, a hidden game or one installed here already.
#[cfg(feature = "net")]
#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_lists_the_games_it_can_copy_from_the_other() {
    use caskade_core::model::{GameRuntime, InstallLocation, LaunchSpec};
    let desk = core("Desktop").await;
    let folder_game = |title: &str, runtime: GameRuntime, launch: LaunchSpec, size: u64| {
        let mut game = Game::new(title);
        game.hidden = title == "Secret";
        let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), title.to_lowercase());
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: format!("/games/{title}").into(),
        });
        entry.launch = Some(launch);
        entry.runtime = runtime;
        entry.size = Some(size);
        (game, entry)
    };
    let program = |name: &str| LaunchSpec::Executable {
        path: name.into(),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
    };
    for (game, mut entry) in [
        folder_game(
            "Celeste",
            GameRuntime::Native,
            program("Celeste"),
            1_200_000_000,
        ),
        folder_game(
            "Halo",
            GameRuntime::Windows {
                prefix: None,
                runner: None,
            },
            program("halo.exe"),
            9_000_000_000,
        ),
        folder_game(
            "Hades",
            GameRuntime::Native,
            LaunchSpec::Uri {
                uri: "steam://rungameid/1145360".into(),
            },
            15_000_000_000,
        ),
        folder_game("Secret", GameRuntime::Native, program("secret"), 1),
        folder_game("Tunic", GameRuntime::Native, program("Tunic"), 2),
    ] {
        let game = desk.store.upsert_game(&game).await.unwrap();
        entry.game_id = game.id.clone();
        desk.store.upsert_entry(&entry).await.unwrap();
    }
    let deck = core("Deck").await;
    // The Deck has Tunic installed already: it isn't offered.
    let tunic = deck.store.upsert_game(&Game::new("Tunic")).await.unwrap();
    let mut own = GameEntry::new(&tunic.id, SourceId::new("manual"), "tunic-here");
    own.state = InstallState::Installed;
    own.location = Some(InstallLocation::External {
        path: "/home/deck/Tunic".into(),
    });
    own.launch = Some(program("Tunic"));
    deck.store.upsert_entry(&own).await.unwrap();
    let link = desk.devices.pairing_link().await.unwrap();
    deck.devices.accept_pairing_link(&link.url).await.unwrap();
    let (_a, _b) = (desk.sync.start(), deck.sync.start());
    let desk_id = desk.id().await;
    deck.sync.sync_now(&desk_id);
    let listed = || async {
        caskade_core::sync::offers::device_games(&deck.store, &deck.devices, &desk_id)
            .await
            .unwrap()
    };
    within("the Deck lists the Desktop's games", 30, || async {
        deck.sync.sync_now(&desk_id);
        listed().await.len() == 2
    })
    .await;
    // The Desktop's Tunic is the same game as the Deck's, known there.
    within("the Desktop's copy of Tunic is known", 30, || async {
        deck.sync.sync_now(&desk_id);
        !deck
            .store
            .sync_peer_copies(&tunic.id)
            .await
            .unwrap()
            .is_empty()
    })
    .await;
    let games = listed().await;
    let titles: Vec<&str> = games.iter().map(|game| game.title.as_str()).collect();
    assert_eq!(titles, ["Celeste", "Halo"]);
    assert_eq!(games[0].size_bytes, Some(1_200_000_000));
    assert_eq!(games[1].size_bytes, Some(9_000_000_000));
    assert_eq!(games[1].through_proton, cfg!(target_os = "linux"));
}
