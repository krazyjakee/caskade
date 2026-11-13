//! Milestone acceptance (#194) for achievements, through the app's own
//! commands over IPC on the real core: Steam and RetroAchievements accounts
//! connected through the store-account sign-in, a PC game and an emulated
//! game verified by ROM hash, then each account's and release's progress,
//! private, unsupported and stale states, hidden achievements and another
//! local account's view. The services are local copies (wiremock) of their
//! documented answers.

use crate::ipc::invoke;
use caskade_achievements::achievements::{
    Achievements, retroachievements::RetroAchievements, steam::SteamAchievements,
};
use caskade_core::{
    accounts::{
        AccountAccess, AccountRole, AccountService, DEFAULT_AVATAR, NewAccount, Permissions,
    },
    bus::Bus,
    db::LibraryStore,
    emulation::{ArgsOverride, EmulatorTarget, GameRom},
    model::{ExternalId, Game, GameEntry, LaunchSpec, SourceId},
    store_accounts::{AuthInput, StoreAccounts, steam::SteamProvider},
};
use caskade_desktop::commands::achievements as commands;
use serde_json::{Value, json};
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const STEAM_ID: &str = "76561197960287930";

async fn steam_service() -> MockServer {
    let server = MockServer::start().await;
    let get = |at: &str| Mock::given(method("GET")).and(path(at.to_owned()));
    get("/ISteamUser/GetPlayerSummaries/v2/")
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"response": {"players": [
                {"steamid": STEAM_ID, "personaname": "Gordon", "communityvisibilitystate": 3}
            ]}})),
        )
        .mount(&server)
        .await;
    let schema = json!({"game": {"availableGameStats": {"achievements": [
        {"name": "WAKE", "displayName": "Wake Up Call", "hidden": 0, "description": "Wake up."},
        {"name": "SECRET", "displayName": "The Secret", "hidden": 1, "description": "Spoiler!"}
    ]}}});
    for app in ["620", "10", "30"] {
        get("/ISteamUserStats/GetSchemaForGame/v2/")
            .and(query_param("appid", app))
            .respond_with(ResponseTemplate::new(200).set_body_json(schema.clone()))
            .mount(&server)
            .await;
    }
    get("/ISteamUserStats/GetPlayerAchievements/v1/")
        .and(query_param("appid", "620"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"playerstats": {
            "success": true, "achievements": [
                {"apiname": "WAKE", "achieved": 1, "unlocktime": 1_300_000_000},
                {"apiname": "SECRET", "achieved": 0, "unlocktime": 0}
            ]}})),
        )
        .mount(&server)
        .await;
    get("/ISteamUserStats/GetPlayerAchievements/v1/")
        .and(query_param("appid", "10"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"playerstats": {
            "error": "Profile is not public", "success": false}})),
        )
        .mount(&server)
        .await;
    get("/ISteamUserStats/GetPlayerAchievements/v1/")
        .and(query_param("appid", "30"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({"playerstats": {
            "error": "Requested app has no stats", "success": false}})),
        )
        .mount(&server)
        .await;
    get("/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/")
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"achievementpercentages": {
            "achievements": [{"name": "WAKE", "percent": "88.1"}, {"name": "SECRET", "percent": 2.5}]}})))
        .mount(&server)
        .await;
    server
}

async fn retro_service(rom_hash: &str) -> MockServer {
    let server = MockServer::start().await;
    let get = |at: &str| Mock::given(method("GET")).and(path(at.to_owned()));
    get("/API/API_GetUserProfile.php")
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"User": "Player", "ULID": "01HXYZ"})),
        )
        .mount(&server)
        .await;
    get("/API/API_GetGameList.php")
        .and(query_param("i", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"ID": 1446, "Title": "Super Mario Bros.", "Hashes": [rom_hash]}
        ])))
        .mount(&server)
        .await;
    get("/API/API_GetGameInfoAndUserProgress.php")
        .and(query_param("g", "1446"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "ID": 1446, "NumDistinctPlayers": 4, "Achievements": {
            "1": {"ID": 1, "Title": "World 1", "Points": 5, "BadgeName": "1", "DisplayOrder": 1,
                  "NumAwarded": 2, "DateEarned": "2024-01-01 10:00:00",
                  "DateEarnedHardcore": "2024-01-01 10:00:00"},
            "2": {"ID": 2, "Title": "World 2", "Points": 10, "BadgeName": "2", "DisplayOrder": 2,
                  "NumAwarded": 1, "DateEarned": "2024-01-02 10:00:00"},
            "3": {"ID": 3, "Title": "World 8", "Points": 25, "BadgeName": "3", "DisplayOrder": 3,
                  "NumAwarded": 0}
        }})))
        .mount(&server)
        .await;
    server
}

fn base(server: &MockServer) -> Url {
    Url::parse(&format!("{}/", server.uri())).unwrap()
}

fn set<'a>(sets: &'a Value, provider: &str) -> &'a Value {
    sets.as_array()
        .unwrap()
        .iter()
        .find(|set| set["account"]["provider"] == provider)
        .unwrap_or_else(|| panic!("no {provider} set in {sets}"))
}

#[test]
fn each_account_s_progress_shows_with_its_states_through_the_app() {
    let temp = tempfile::tempdir().unwrap();
    let rom = temp.path().join("smb.nes");
    let body = vec![9u8; 8192];
    let mut file = b"NES\x1a".to_vec();
    file.extend([0u8; 12]);
    file.extend(&body);
    std::fs::write(&rom, &file).unwrap();
    // The hash RetroAchievements lists for it: the ROM without its header.
    let rom_hash = caskade_achievements::achievements::retroachievements::hash::hash_bytes(
        &body,
        caskade_achievements::achievements::retroachievements::hash::Rule::Whole,
    )
    .unwrap();

    let (store, bus, local, achievements, steam, games, admin, kid) =
        tauri::async_runtime::block_on(async {
            let steam = steam_service().await;
            let retro = retro_service(&rom_hash).await;
            let store = LibraryStore::in_memory().await.unwrap();
            let bus = Bus::default();
            // The household: an admin and a child.
            let local = AccountService::open(store.clone(), bus.clone())
                .await
                .unwrap();
            let admin = local.signed_in().unwrap();
            local.set_pin(&admin.id, Some("2468".into())).await.unwrap();
            let kid = local
                .create(NewAccount {
                    name: "Kid".into(),
                    avatar: DEFAULT_AVATAR.into(),
                    access: AccountAccess {
                        role: AccountRole::Standard,
                        permissions: Permissions::default(),
                        max_age: None,
                        unrated_needs_pin: false,
                    },
                    pin: None,
                })
                .await
                .unwrap();
            // Store accounts, connected through their sign-ins: the admin's
            // Steam account, and a RetroAchievements account for everyone.
            let stores = StoreAccounts::new(store.clone(), bus.clone());
            stores.register_provider(Arc::new(SteamProvider::with_base(base(&steam))));
            let ra = Arc::new(RetroAchievements::with_base(
                base(&retro),
                "https://media.example",
            ));
            stores.register_provider(ra.clone());
            let connect =
                |provider: &'static str, fields: Vec<(&str, &str)>, owner: Option<String>| {
                    let stores = stores.clone();
                    let fields = fields
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect();
                    async move {
                        let flow = stores.begin_connect(provider, None).await.unwrap();
                        stores
                            .complete_connect(&flow.flow_id, &AuthInput::Fields { fields }, owner)
                            .await
                            .unwrap()
                    }
                };
            connect(
                "steam",
                vec![("profile", STEAM_ID), ("key", "KEY")],
                Some(admin.id.clone()),
            )
            .await;
            connect(
                "retroachievements",
                vec![("username", "Player"), ("key", "RAKEY")],
                None,
            )
            .await;
            let achievements =
                Achievements::new(store.clone(), bus.clone(), Arc::new(stores.clone()));
            achievements.register_provider(Arc::new(SteamAchievements::with_base(base(&steam))));
            achievements.register_provider(ra.clone());
            achievements.register_linker(ra);
            // The library: three Steam games and an emulated NES game.
            let mut games = vec![];
            for (title, app) in [("Portal 2", 620u32), ("Private", 10), ("No Stats", 30)] {
                let game = Game::new(title);
                let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), app.to_string());
                entry.ids = vec![ExternalId::Steam { app_id: app }];
                games.push(
                    store
                        .import_group(&game, &[entry], None)
                        .await
                        .unwrap()
                        .game_id,
                );
            }
            let game = Game::new("Super Mario Bros.");
            let mut entry = GameEntry::new(&game.id, SourceId::new("roms"), "smb");
            entry.launch = Some(LaunchSpec::Emulator {
                platform_id: "nintendo_nes".into(),
                target: EmulatorTarget::PlatformDefault,
                roms: vec![GameRom::absolute(&rom)],
                args: ArgsOverride::default(),
            });
            games.push(
                store
                    .import_group(&game, &[entry], None)
                    .await
                    .unwrap()
                    .game_id,
            );
            std::mem::forget(retro);
            (store, bus, local, achievements, steam, games, admin, kid)
        });
    let app = mock_builder()
        .manage(store.clone())
        .manage(bus)
        .manage(local.clone())
        .manage(achievements)
        .invoke_handler(tauri::generate_handler![
            commands::game_achievements,
            commands::game_achievements_refresh,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let [portal, private, no_stats, smb] = games.as_slice() else {
        unreachable!()
    };
    let refresh = |game: &str| {
        invoke(
            &window,
            "game_achievements_refresh",
            json!({"gameId": game}),
        )
        .unwrap()
    };

    // Before any fetch: a set, not yet fetched (not an empty list).
    let first = invoke(&window, "game_achievements", json!({"gameId": portal})).unwrap();
    assert_eq!(set(&first, "steam")["sync"]["status"], "not_fetched");

    // A PC game: the admin's Steam account, attributed, with a hidden one.
    let sets = refresh(portal);
    let steam_set = set(&sets, "steam");
    assert_eq!(steam_set["account"]["providerName"], "Steam");
    assert_eq!(steam_set["account"]["displayName"], "Gordon");
    assert_eq!(steam_set["sync"]["status"], "ready");
    assert_eq!(
        (steam_set["unlocked"].clone(), steam_set["total"].clone()),
        (json!(1), json!(2))
    );
    let secret = &steam_set["achievements"][1];
    assert_eq!(
        secret["definition"]["hidden"], true,
        "the UI conceals it until unlocked"
    );
    assert_eq!(secret["state"]["unlocked"], false);
    assert_eq!(
        steam_set["achievements"][0]["state"]["unlockedAtMs"],
        1_300_000_000_000_i64
    );
    let rarity = steam_set["achievements"][0]["definition"]["rarityPercent"]
        .as_f64()
        .unwrap();
    assert!((rarity - 88.1).abs() < 0.001, "{rarity}");
    assert_eq!(steam_set["capabilities"]["hardcore"], false);

    // Private and unsupported are their own states.
    assert_eq!(set(&refresh(private), "steam")["sync"]["status"], "private");
    assert_eq!(
        set(&refresh(no_stats), "steam")["sync"]["status"],
        "unsupported"
    );

    // An emulated game: RetroAchievements by verified ROM hash, with
    // hardcore unlocks kept apart.
    let sets = refresh(smb);
    let ra_set = set(&sets, "retroachievements");
    assert_eq!(ra_set["productId"], "1446");
    assert_eq!(
        (
            ra_set["unlocked"].clone(),
            ra_set["hardcoreUnlocked"].clone(),
            ra_set["total"].clone()
        ),
        (json!(2), json!(1), json!(3))
    );

    // Stale: Steam can't be reached; the last answer stays, with why.
    tauri::async_runtime::block_on(async {
        steam.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&steam)
            .await;
    });
    let stale = refresh(portal);
    let stale = set(&stale, "steam");
    assert_eq!(stale["sync"]["status"], "ready");
    assert!(!stale["sync"]["error"].is_null(), "stale data says why");
    assert_eq!(stale["unlocked"], 1, "progress already seen stays");

    // The child doesn't see the admin's Steam account; the shared
    // RetroAchievements account shows to everyone.
    tauri::async_runtime::block_on(local.sign_in(&kid.id, None)).unwrap();
    let theirs = invoke(&window, "game_achievements", json!({"gameId": portal})).unwrap();
    assert_eq!(theirs, json!([]));
    let shared = invoke(&window, "game_achievements", json!({"gameId": smb})).unwrap();
    assert_eq!(set(&shared, "retroachievements")["unlocked"], 2);
    let _ = admin;
}
