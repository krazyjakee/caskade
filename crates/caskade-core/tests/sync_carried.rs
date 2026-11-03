//! The rest of what policy declares synced (#273), between two devices'
//! engines: appearance and the rating board as fields changed from their
//! defaults; game scripts, launch actions and hand-made achievement links,
//! by the release they belong to.

use caskade_core::achievements::{AchievementLink, LinkMethod};
use caskade_core::bus::Bus;
use caskade_core::db::{LibraryStore, PluginRow};
use caskade_core::emulation::{Emulator, EmulatorInstall, EmulatorProfile};
use caskade_core::model::{
    ActionKind, ActionTracking, EntryActionSettings, ExternalId, Game, GameAction, GameEntry,
    LaunchSpec,
};
use caskade_core::paths::AppPaths;
use caskade_core::scripts::{LifecycleScript, ScriptCommand, ScriptPhase, ScriptScope, Shell};
use caskade_core::settings::SettingsService;
use caskade_core::sync::SyncEngine;
use caskade_core::sync::change::{EXISTS, Entity};
use serde_json::{Value, json};

struct Device {
    engine: SyncEngine,
    store: LibraryStore,
    accounts: caskade_core::accounts::AccountService,
    settings: SettingsService,
    _temp: tempfile::TempDir,
}

async fn device(name: char) -> Device {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = caskade_core::accounts::AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let settings = SettingsService::new(store.clone());
    settings
        .register_core(&AppPaths::under(temp.path()))
        .unwrap();
    let engine = SyncEngine::new(store.clone(), name.to_string().repeat(64));
    engine.set_settings(settings.clone());
    let joined = accounts.clone();
    engine.on_account_joined(move |from, into| {
        let joined = joined.clone();
        tokio::spawn(async move { joined.joined(&from, &into).await });
    });
    Device {
        engine,
        store,
        accounts,
        settings,
        _temp: temp,
    }
}

/// Both ways, until neither has anything new.
async fn sync(a: &Device, b: &Device) {
    for _ in 0..2 {
        for (from, to) in [(a, b), (b, a)] {
            from.engine.capture().await.unwrap();
            loop {
                let cursor = to.engine.seen().await.unwrap();
                let changes = from.engine.changes_for(&cursor, 100).await.unwrap();
                if changes.is_empty() {
                    break;
                }
                to.engine.receive(&changes).await.unwrap();
            }
        }
    }
}

async fn get(device: &Device, namespace: &str, field: &str) -> Value {
    device.settings.get(namespace).await.unwrap().value[field].clone()
}

async fn set(device: &Device, namespace: &str, field: &str, value: Value) {
    let mut section = device.settings.get(namespace).await.unwrap().value;
    section[field] = value;
    device.settings.set(namespace, section, None).await.unwrap();
}

async fn set_category(device: &Device, category: &str, state: &str) {
    let mut section = device.settings.get("core.sync").await.unwrap().value;
    section["categories"][category] = json!(state);
    device
        .settings
        .set("core.sync", section, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn appearance_and_the_rating_board_follow_the_user_and_defaults_never_win() {
    let desk = device('a').await;
    set(&desk, "core.appearance", "density", json!("compact")).await;
    set(&desk, "core.appearance", "accent", json!("#ff8800")).await;
    set(&desk, "core.accounts", "ratingBoard", json!("esrb")).await;
    // A new device, set up after the Desk's choices: its defaults are newer,
    // and still don't replace them.
    let laptop = device('b').await;
    sync(&desk, &laptop).await;
    for device in [&desk, &laptop] {
        assert_eq!(get(device, "core.appearance", "density").await, "compact");
        assert_eq!(get(device, "core.appearance", "accent").await, "#ff8800");
        assert_eq!(get(device, "core.accounts", "ratingBoard").await, "esrb");
        // Fields nobody changed stay as they were.
        assert_eq!(get(device, "core.appearance", "cardSize").await, 176);
    }

    // Set back to its default on the Laptop: back to the default everywhere.
    set(&laptop, "core.appearance", "density", json!("comfortable")).await;
    sync(&desk, &laptop).await;
    assert_eq!(
        get(&desk, "core.appearance", "density").await,
        "comfortable"
    );
    assert_eq!(get(&desk, "core.appearance", "accent").await, "#ff8800");
}

#[tokio::test]
async fn appearance_set_to_this_device_only_stays_here_until_turned_back_on() {
    let desk = device('a').await;
    let laptop = device('b').await;
    sync(&desk, &laptop).await;
    set_category(&laptop, "appearance", "thisDeviceOnly").await;
    set(&desk, "core.appearance", "density", json!("compact")).await;
    set(&laptop, "core.appearance", "cardSize", json!(240)).await;
    sync(&desk, &laptop).await;
    assert_eq!(
        get(&laptop, "core.appearance", "density").await,
        "comfortable"
    );
    assert_eq!(get(&desk, "core.appearance", "cardSize").await, 176);

    // Back on: the Desk's choice, waiting here, applies. (Sending the
    // Laptop's own again is the exchange's replay, `sync_network`.)
    set_category(&laptop, "appearance", "sync").await;
    laptop.engine.apply_pending().await.unwrap();
    assert_eq!(get(&laptop, "core.appearance", "density").await, "compact");
}

/// Hades with its Steam release: the same game and release on each device.
async fn hades(device: &Device) -> (String, String) {
    let game = device.store.upsert_game(&Game::new("Hades")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "1145360");
    entry.ids = vec![ExternalId::Steam { app_id: 1_145_360 }];
    let entry = device.store.upsert_entry(&entry).await.unwrap();
    (game.id, entry.id)
}

fn shell(text: &str) -> ScriptCommand {
    ScriptCommand::Shell {
        shell: Shell::Sh,
        script: text.to_owned(),
    }
}

async fn game_scripts(device: &Device, game: &str) -> Vec<LifecycleScript> {
    device
        .store
        .lifecycle_scripts(&ScriptScope::Game {
            game_id: game.to_owned(),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn game_scripts_arrive_turned_off() {
    let desk = device('a').await;
    let laptop = device('b').await;
    let (desk_game, _) = hades(&desk).await;
    let (laptop_game, _) = hades(&laptop).await;
    let mut script = LifecycleScript::new(
        ScriptScope::Game {
            game_id: desk_game.clone(),
        },
        ScriptPhase::BeforeLaunch,
        shell("echo ready"),
    );
    script.name = "Mount the save drive".into();
    let script = desk.store.save_lifecycle_script(&script).await.unwrap();
    // A program on the Desk stays there.
    let mut direct = LifecycleScript::new(
        ScriptScope::Game {
            game_id: desk_game.clone(),
        },
        ScriptPhase::AfterExit,
        ScriptCommand::Direct {
            program: "/opt/tools/backup".into(),
            args: Vec::new(),
        },
    );
    direct.name = "Back up".into();
    desk.store.save_lifecycle_script(&direct).await.unwrap();

    // Game scripts are optional, off by default (turning one on later
    // replays what's there: `sync_network`).
    for device in [&desk, &laptop] {
        set_category(device, "gameScripts", "sync").await;
    }
    sync(&desk, &laptop).await;
    let arrived = game_scripts(&laptop, &laptop_game).await;
    assert_eq!(arrived.len(), 1);
    assert_eq!(arrived[0].id, script.id);
    assert_eq!(arrived[0].name, "Mount the save drive");
    assert_eq!(arrived[0].command, shell("echo ready"));
    assert!(
        !arrived[0].enabled,
        "a script from another device arrives off"
    );

    // Turned on here, it stays on while only its name changes there…
    let mut mine = arrived[0].clone();
    mine.enabled = true;
    laptop.store.save_lifecycle_script(&mine).await.unwrap();
    let mut renamed = script.clone();
    renamed.name = "Mount the drive".into();
    desk.store.save_lifecycle_script(&renamed).await.unwrap();
    sync(&desk, &laptop).await;
    let here = &game_scripts(&laptop, &laptop_game).await[0];
    assert_eq!(
        (here.name.as_str(), here.enabled),
        ("Mount the drive", true)
    );
    // …and goes off when its command changes there.
    let mut changed = renamed.clone();
    changed.command = shell("rm -rf ~/saves");
    desk.store.save_lifecycle_script(&changed).await.unwrap();
    sync(&desk, &laptop).await;
    let here = &game_scripts(&laptop, &laptop_game).await[0];
    assert_eq!(here.command, shell("rm -rf ~/saves"));
    assert!(!here.enabled);
    // The Desk's own stays as the Desk had it.
    assert!(
        game_scripts(&desk, &desk_game)
            .await
            .iter()
            .all(|script| script.enabled)
    );

    desk.store
        .delete_lifecycle_script(&script.id)
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    assert!(game_scripts(&laptop, &laptop_game).await.is_empty());
}

#[tokio::test]
async fn launch_actions_and_their_choices_follow_the_release_where_they_can_be_bound() {
    let desk = device('a').await;
    let laptop = device('b').await;
    let (_, desk_entry) = hades(&desk).await;
    let (_, laptop_entry) = hades(&laptop).await;
    for device in [&desk, &laptop] {
        set_category(device, "launchActions", "sync").await;
    }
    let flatpak = desk
        .store
        .save_game_action(&GameAction {
            id: "0190f1a2-0000-7000-8000-000000000001".into(),
            entry_id: desk_entry.clone(),
            name: "Play with the mod loader".into(),
            kind: ActionKind::Play,
            target: LaunchSpec::Flatpak {
                app_id: "org.example.Loader".into(),
                args: vec!["--hades".into()],
            },
            tracking: ActionTracking::default(),
        })
        .await
        .unwrap();
    // A program on the Desk: it stays there.
    desk.store
        .save_game_action(&GameAction {
            id: "0190f1a2-0000-7000-8000-000000000002".into(),
            entry_id: desk_entry.clone(),
            name: "Config".into(),
            kind: ActionKind::Utility,
            target: LaunchSpec::Executable {
                path: "/opt/hades/config".into(),
                args: Vec::new(),
                cwd: None,
                env: Default::default(),
            },
            tracking: ActionTracking::default(),
        })
        .await
        .unwrap();
    let mut choice = EntryActionSettings {
        default_action: Some(flatpak.id.clone()),
        ask: true,
        ..EntryActionSettings::default()
    };
    choice.built_in.name = Some("Play (Steam)".into());
    desk.store
        .save_entry_action_settings(&desk_entry, &choice)
        .await
        .unwrap();

    sync(&desk, &laptop).await;
    let actions = laptop.store.game_actions(&laptop_entry).await.unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, flatpak.id);
    assert_eq!(actions[0].name, "Play with the mod loader");
    // Bound here: the app, without the Desk's arguments.
    assert_eq!(
        actions[0].target,
        LaunchSpec::Flatpak {
            app_id: "org.example.Loader".into(),
            args: Vec::new()
        }
    );
    let settings = laptop
        .store
        .entry_action_settings(&laptop_entry)
        .await
        .unwrap();
    assert_eq!(
        settings.default_action.as_deref(),
        Some(flatpak.id.as_str())
    );
    assert!(settings.ask);
    assert_eq!(settings.built_in.name.as_deref(), Some("Play (Steam)"));

    // Removed on the Laptop: gone from the Desk, and so is its default.
    laptop.store.delete_game_action(&flatpak.id).await.unwrap();
    sync(&desk, &laptop).await;
    let left = desk.store.game_actions(&desk_entry).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "Config");
    assert_eq!(
        desk.store
            .entry_action_settings(&desk_entry)
            .await
            .unwrap()
            .default_action,
        None
    );
}

#[tokio::test]
async fn a_hand_made_achievement_link_follows_its_release() {
    let desk = device('a').await;
    let laptop = device('b').await;
    let (_, desk_entry) = hades(&desk).await;
    let (_, laptop_entry) = hades(&laptop).await;
    desk.store
        .save_achievement_link(&AchievementLink {
            entry_id: desk_entry.clone(),
            provider: "retroachievements".into(),
            product_id: "11240".into(),
            method: LinkMethod::Manual,
            linked_at_ms: 1_700_000_000_000,
        })
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    let links = laptop
        .store
        .achievement_links(std::slice::from_ref(&laptop_entry))
        .await
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].product_id, "11240");
    assert_eq!(links[0].method, LinkMethod::Manual);

    desk.store
        .delete_achievement_link(&desk_entry, "retroachievements")
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    assert!(
        laptop
            .store
            .achievement_links(std::slice::from_ref(&laptop_entry))
            .await
            .unwrap()
            .is_empty()
    );
}

async fn emulator(device: &Device, id: &str, definition: &str) {
    device
        .store
        .upsert_emulator(&Emulator {
            id: id.into(),
            name: definition.into(),
            definition: Some(definition.into()),
            install: EmulatorInstall::Folder {
                dir: "/opt/emulators".into(),
                executable: definition.into(),
            },
            profiles: vec![EmulatorProfile::BuiltIn {
                id: format!("{id}-gba"),
                definition_profile: "GBA".into(),
                args: Default::default(),
                hooks: Default::default(),
            }],
        })
        .await
        .unwrap();
}

async fn default_emulator(device: &Device, platform: &str) -> Option<String> {
    device
        .store
        .get_platform_settings(platform)
        .await
        .unwrap()
        .and_then(|settings| settings.default)
        .map(|default| default.emulator_id)
}

#[tokio::test]
async fn a_preferred_emulator_applies_where_that_emulator_is_installed() {
    let desk = device('a').await;
    let laptop = device('b').await;
    for device in [&desk, &laptop] {
        set_category(device, "preferredEmulators", "sync").await;
    }
    emulator(&desk, "desk-mgba", "mgba").await;
    let mut settings = caskade_core::emulation::PlatformSettings {
        platform: "gba".into(),
        default: Some(caskade_core::emulation::ProfileRef {
            emulator_id: "desk-mgba".into(),
            profile_id: "desk-mgba-gba".into(),
        }),
        bios_dir: None,
    };
    desk.store.set_platform_settings(&settings).await.unwrap();
    sync(&desk, &laptop).await;
    // The Laptop has no mGBA: nothing changes there.
    assert_eq!(default_emulator(&laptop, "gba").await, None);

    // Once it has one, it's the platform's default.
    emulator(&laptop, "laptop-mgba", "mgba").await;
    laptop.engine.apply_pending().await.unwrap();
    assert_eq!(
        default_emulator(&laptop, "gba").await.as_deref(),
        Some("laptop-mgba")
    );
    sync(&desk, &laptop).await;
    // Clearing the Desk's default is no reason to clear the Laptop's.
    settings.default = None;
    desk.store.set_platform_settings(&settings).await.unwrap();
    sync(&desk, &laptop).await;
    assert_eq!(
        default_emulator(&laptop, "gba").await.as_deref(),
        Some("laptop-mgba")
    );
}

/// #275: a preferred emulator the Laptop lacks is offered there, nothing
/// installs, and Keep mine sets it aside for good.
#[tokio::test]
async fn a_preferred_emulator_missing_here_is_offered_and_can_be_set_aside() {
    use caskade_core::sync::offers::{emulator_offers, keep_emulator};
    let desk = device('a').await;
    let laptop = device('b').await;
    for device in [&desk, &laptop] {
        set_category(device, "preferredEmulators", "sync").await;
    }
    emulator(&desk, "desk-mgba", "mgba").await;
    emulator(&desk, "desk-duck", "duckstation").await;
    for (platform, emulator, profile) in [
        ("gba", "desk-mgba", "desk-mgba-gba"),
        ("psx", "desk-duck", "desk-duck-gba"),
    ] {
        desk.store
            .set_platform_settings(&caskade_core::emulation::PlatformSettings {
                platform: platform.into(),
                default: Some(caskade_core::emulation::ProfileRef {
                    emulator_id: emulator.into(),
                    profile_id: profile.into(),
                }),
                bios_dir: None,
            })
            .await
            .unwrap();
    }
    sync(&desk, &laptop).await;
    let offers = emulator_offers(&laptop.engine).await.unwrap();
    let offered: Vec<_> = offers
        .iter()
        .map(|offer| (offer.platform_id.as_str(), offer.definition_id.as_str()))
        .collect();
    assert_eq!(offered, [("gba", "mgba"), ("psx", "duckstation")]);
    assert_eq!(offers[0].device_id, desk.engine.device());
    // Offered, not installed: the Laptop has no emulators.
    assert!(laptop.store.emulators().await.unwrap().is_empty());
    // The Desk has its own: nothing to offer there.
    assert!(emulator_offers(&desk.engine).await.unwrap().is_empty());

    // Keep mine: no longer offered, and installing it later changes nothing.
    keep_emulator(&laptop.engine, "psx").await.unwrap();
    let offers = emulator_offers(&laptop.engine).await.unwrap();
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].platform_id, "gba");
    emulator(&laptop, "laptop-duck", "duckstation").await;
    laptop.engine.apply_pending().await.unwrap();
    assert_eq!(default_emulator(&laptop, "psx").await, None);

    // Installed: it applies, and the offer goes.
    emulator(&laptop, "laptop-mgba", "mgba").await;
    laptop.engine.apply_pending().await.unwrap();
    assert_eq!(
        default_emulator(&laptop, "gba").await.as_deref(),
        Some("laptop-mgba")
    );
    assert!(emulator_offers(&laptop.engine).await.unwrap().is_empty());

    // Preferred emulators This device only: nothing offered.
    let settings = caskade_core::emulation::PlatformSettings {
        platform: "snes".into(),
        default: Some(caskade_core::emulation::ProfileRef {
            emulator_id: "desk-mgba".into(),
            profile_id: "desk-mgba-gba".into(),
        }),
        bios_dir: None,
    };
    set_category(&laptop, "preferredEmulators", "thisDeviceOnly").await;
    desk.store.set_platform_settings(&settings).await.unwrap();
    sync(&desk, &laptop).await;
    assert!(emulator_offers(&laptop.engine).await.unwrap().is_empty());
}

#[tokio::test]
async fn each_device_offers_its_add_ons_and_nothing_installs_itself() {
    let desk = device('a').await;
    let laptop = device('b').await;
    for device in [&desk, &laptop] {
        set_category(device, "addons", "sync").await;
    }
    let plugin = |enabled| PluginRow {
        id: "dev.example.stats".into(),
        enabled,
        grants: json!({}),
    };
    desk.store.upsert_plugin_state(&plugin(true)).await.unwrap();
    desk.store
        .set_setting(
            "addons.records",
            &json!({ "addons": { "caskade.ember": { "version": "1.2.0" } } }),
        )
        .await
        .unwrap();
    laptop
        .store
        .upsert_plugin_state(&plugin(false))
        .await
        .unwrap();
    sync(&desk, &laptop).await;

    // Each knows what the other has, under the other's id.
    let desk_id = "a".repeat(64);
    let offered = |device: &Device, key: String, field: &'static str| {
        let store = device.store.clone();
        async move {
            store
                .sync_field(Entity::Addon, &key, field)
                .await
                .unwrap()
                .and_then(|state| state.value)
        }
    };
    assert_eq!(
        offered(&laptop, format!("{desk_id}|caskade.ember"), EXISTS).await,
        Some(json!({ "version": "1.2.0" }))
    );
    assert_eq!(
        offered(&laptop, format!("{desk_id}|dev.example.stats"), "enabled").await,
        Some(json!(true))
    );
    assert_eq!(
        offered(
            &desk,
            format!("{}|dev.example.stats", "b".repeat(64)),
            "enabled"
        )
        .await,
        Some(json!(false))
    );
    // Nothing installed, turned on or changed by itself, and the two settle.
    assert!(!laptop.store.plugin_states().await.unwrap()[0].enabled);
    assert!(
        laptop
            .store
            .get_setting("addons.records")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(desk.engine.capture().await.unwrap(), 0);
    assert_eq!(laptop.engine.capture().await.unwrap(), 0);

    // Removed from the Desk: no longer offered.
    desk.store
        .delete_plugin_state("dev.example.stats")
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    assert_eq!(
        offered(&laptop, format!("{desk_id}|dev.example.stats"), EXISTS).await,
        None
    );
}

async fn names(device: &Device) -> Vec<String> {
    let mut names: Vec<String> = device
        .accounts
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|account| account.name)
        .collect();
    names.sort();
    names
}

async fn owns(device: &Device, account: &str, collection: &str) -> bool {
    device
        .store
        .collection_summaries(account)
        .await
        .unwrap()
        .iter()
        .any(|summary| summary.name == collection)
}

#[tokio::test]
async fn each_device_s_default_admin_is_the_household_s_and_joins_a_real_one() {
    let desk = device('a').await;
    let laptop = device('b').await;
    let desk_admin = desk.accounts.active_id().unwrap();
    let laptop_admin = laptop.accounts.active_id().unwrap();
    desk.store
        .save_collection(
            &desk_admin,
            &caskade_core::model::SaveCollection {
                id: None,
                name: "Couch".into(),
                description: String::new(),
                kind: caskade_core::model::CollectionKind::Manual,
            },
        )
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    // No second "Player" on either, and the Desk's collection is the
    // Laptop's own default admin's there.
    assert_eq!(names(&desk).await, ["Player"]);
    assert_eq!(names(&laptop).await, ["Player"]);
    assert!(owns(&laptop, &laptop_admin, "Couch").await);

    // Set up on the Desk: the Laptop's default admin joins it, with what it
    // owned, and whoever was signed in as it is now.
    desk.accounts
        .update(
            &desk_admin,
            caskade_core::accounts::AccountUpdate {
                name: "Ada".into(),
                avatar: "avatar:default".into(),
                access: None,
            },
        )
        .await
        .unwrap();
    sync(&desk, &laptop).await;
    assert_eq!(names(&laptop).await, ["Ada"]);
    assert_eq!(names(&desk).await, ["Ada"]);
    assert!(owns(&laptop, &desk_admin, "Couch").await);
    assert!(owns(&desk, &desk_admin, "Couch").await);
    tokio::task::yield_now().await;
    for _ in 0..50 {
        if laptop.accounts.active_id().as_deref() == Some(desk_admin.as_str()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(laptop.accounts.active_id(), Some(desk_admin));
}
