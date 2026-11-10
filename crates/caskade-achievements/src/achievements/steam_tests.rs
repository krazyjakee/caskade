//! Steam achievements against a local copy of the Web API (#186), with
//! sanitized answers in the documented shapes.

use super::*;
use crate::achievements::{AccountAccess, Achievements, SetStatus};
use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
    model::{Game, GameEntry, SourceId},
    store_accounts::SyncState,
};
use serde_json::json;
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const SCHEMA: &str = "/ISteamUserStats/GetSchemaForGame/v2/";
const PLAYER: &str = "/ISteamUserStats/GetPlayerAchievements/v1/";
const RARITY: &str = "/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/";

fn credentials() -> StoreCredentials {
    StoreCredentials {
        access_token: "KEY".into(),
        refresh_token: None,
        expires_at_ms: None,
        extra: [("steamid".to_owned(), "76561197960287930".to_owned())].into(),
    }
}
fn account() -> StoreAccount {
    StoreAccount {
        id: "steam-account".into(),
        provider: "steam".into(),
        external_id: "76561197960287930".into(),
        display_name: "Player".into(),
        owner_id: None,
        enabled: true,
        sync: SyncState::default(),
        created_at_ms: 0,
    }
}
fn schema() -> serde_json::Value {
    json!({"game": {"gameName": "Portal 2", "availableGameStats": {"achievements": [
        {"name": "ACH_WAKE_UP", "displayName": "Wake Up Call", "hidden": 0,
         "description": "Survive the manual override.", "icon": "https://cdn.example/a.jpg",
         "icongray": "https://cdn.example/a_gray.jpg"},
        {"name": "ACH_SECRET", "displayName": "Secret", "hidden": 1,
         "icon": "https://cdn.example/s.jpg", "icongray": "https://cdn.example/s_gray.jpg"},
        {"name": "ACH_LAST", "displayName": "Last", "hidden": 0, "description": "Finish."}
    ]}}})
}
fn player(unlocked: &[(&str, i64)]) -> serde_json::Value {
    let achievements: Vec<_> = ["ACH_WAKE_UP", "ACH_SECRET", "ACH_LAST"]
        .iter()
        .map(|name| {
            let at = unlocked
                .iter()
                .find(|(id, _)| id == name)
                .map(|(_, at)| *at);
            json!({"apiname": name, "achieved": at.is_some() as u8, "unlocktime": at.unwrap_or(0)})
        })
        .collect();
    json!({"playerstats": {"steamID": "76561197960287930", "gameName": "Portal 2",
        "achievements": achievements, "success": true}})
}
async fn answer(server: &MockServer, at: &str, app: &str, status: u16, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(at))
        .and(query_param(
            if at == RARITY { "gameid" } else { "appid" },
            app,
        ))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}
fn steam(server: &MockServer) -> SteamAchievements {
    SteamAchievements::with_base(Url::parse(&format!("{}/", server.uri())).unwrap())
}

#[tokio::test]
async fn definitions_and_unlocks_join_by_api_name_with_times_in_milliseconds() {
    let server = MockServer::start().await;
    answer(&server, SCHEMA, "620", 200, schema()).await;
    answer(
        &server,
        PLAYER,
        "620",
        200,
        player(&[("ACH_WAKE_UP", 1_300_000_000), ("ACH_LAST", 0)]),
    )
    .await;
    answer(
        &server,
        RARITY,
        "620",
        200,
        json!({"achievementpercentages": {"achievements": [
            {"name": "ACH_WAKE_UP", "percent": 81.5}, {"name": "ACH_SECRET", "percent": "4.2"}
        ]}}),
    )
    .await;
    let fetched = steam(&server)
        .fetch(
            &account(),
            Some(&credentials()),
            "620",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(fetched.complete);
    assert_eq!(fetched.definitions.len(), 3);
    let wake = &fetched.states["ACH_WAKE_UP"];
    assert!(wake.unlocked);
    assert_eq!(wake.unlocked_at_ms, Some(1_300_000_000_000));
    // Unlocked with no time: the time stays unknown, not 1970.
    let last = &fetched.states["ACH_LAST"];
    assert!(last.unlocked);
    assert_eq!(last.unlocked_at_ms, None);
    assert!(!fetched.states["ACH_SECRET"].unlocked);
    let secret = &fetched.definitions[1];
    assert!(secret.hidden);
    assert_eq!(secret.description, None);
    assert_eq!(
        secret.locked_icon_url.as_deref(),
        Some("https://cdn.example/s_gray.jpg")
    );
    assert_eq!(fetched.definitions[0].rarity_percent, Some(81.5));
    assert_eq!(secret.rarity_percent, Some(4.2));
    // Steam reports no progress or points here: they stay unknown.
    assert!(
        fetched
            .definitions
            .iter()
            .all(|d| d.progress_max.is_none() && d.points.is_none())
    );
    assert!(
        fetched
            .states
            .values()
            .all(|s| s.progress.is_none() && s.hardcore.is_none())
    );
}

#[tokio::test]
async fn private_profiles_missing_keys_unsupported_and_empty_games_differ() {
    let server = MockServer::start().await;
    answer(&server, SCHEMA, "620", 200, schema()).await;
    answer(
        &server,
        PLAYER,
        "620",
        403,
        json!({"playerstats": {"error": "Profile is not public", "success": false}}),
    )
    .await;
    // A game Steam keeps no achievements for, and one with a schema but no
    // stats for the player.
    answer(&server, SCHEMA, "10", 200, json!({"game": {}})).await;
    answer(&server, SCHEMA, "20", 400, json!({})).await;
    answer(&server, SCHEMA, "30", 200, schema()).await;
    answer(
        &server,
        PLAYER,
        "30",
        400,
        json!({"playerstats": {"error": "Requested app has no stats", "success": false}}),
    )
    .await;
    let steam = steam(&server);
    let cancel = CancellationToken::new();
    let fetch = |product: &'static str, creds: Option<StoreCredentials>| {
        let steam = steam.clone();
        let cancel = cancel.clone();
        async move {
            steam
                .fetch(&account(), creds.as_ref(), product, &cancel)
                .await
        }
    };
    assert!(matches!(
        fetch("620", Some(credentials())).await,
        Err(FetchError::Provider(ProviderError::Unavailable(_)))
    ));
    for empty in ["10", "20"] {
        let fetched = fetch(empty, Some(credentials())).await.unwrap();
        assert!(
            fetched.complete && fetched.definitions.is_empty(),
            "{empty}"
        );
    }
    assert!(matches!(
        fetch("30", Some(credentials())).await,
        Err(FetchError::Unsupported(_))
    ));
    assert!(matches!(
        fetch("620", None).await,
        Err(FetchError::Provider(ProviderError::Expired(_)))
    ));
}

#[tokio::test]
async fn a_missing_rarity_leaves_the_achievements_and_a_failed_player_list_keeps_nothing() {
    let server = MockServer::start().await;
    answer(&server, SCHEMA, "620", 200, schema()).await;
    answer(&server, PLAYER, "620", 200, player(&[])).await;
    answer(&server, RARITY, "620", 500, json!({})).await;
    answer(&server, SCHEMA, "400", 200, schema()).await;
    answer(&server, PLAYER, "400", 503, json!({})).await;
    let steam = steam(&server);
    let cancel = CancellationToken::new();
    let fetched = steam
        .fetch(&account(), Some(&credentials()), "620", &cancel)
        .await
        .unwrap();
    assert!(fetched.complete);
    assert!(
        fetched
            .definitions
            .iter()
            .all(|d| d.rarity_percent.is_none())
    );
    assert!(matches!(
        steam
            .fetch(&account(), Some(&credentials()), "400", &cancel)
            .await,
        Err(FetchError::Provider(ProviderError::Offline(_)))
    ));
}

/// The store account, signed in.
struct Access(LibraryStore);
#[async_trait]
impl AccountAccess for Access {
    async fn accounts(&self) -> caskade_core::Result<Vec<StoreAccount>> {
        self.0.store_accounts().await
    }
    async fn credentials(
        &self,
        _: &StoreAccount,
    ) -> Result<Option<StoreCredentials>, ProviderError> {
        Ok(Some(credentials()))
    }
    fn provider_name(&self, _: &str) -> Option<String> {
        Some(STEAM.into())
    }
}

#[tokio::test]
async fn one_set_for_discovery_s_copy_and_the_owned_copy_and_a_later_unlock_is_new() {
    let server = MockServer::start().await;
    answer(&server, SCHEMA, "620", 200, schema()).await;
    let store = LibraryStore::in_memory().await.unwrap();
    store.save_store_account(&account()).await.unwrap();
    // Discovery's installed copy and the account's owned copy: one game,
    // two releases with the same app id.
    let game = Game::new("Portal 2");
    let installed = {
        let mut entry = GameEntry::new(&game.id, SourceId::new("steam"), "620");
        entry.ids = vec![ExternalId::Steam { app_id: 620 }];
        entry
    };
    let owned = {
        let mut entry = GameEntry::new(
            &game.id,
            caskade_core::store_accounts::source_id("steam-account"),
            "620",
        );
        entry.ids = vec![ExternalId::Steam { app_id: 620 }];
        entry
    };
    let game = store
        .import_group(&game, &[installed, owned], None)
        .await
        .unwrap()
        .game_id;
    let bus = Bus::default();
    let mut events = bus.stream();
    let service = Achievements::new(store.clone(), bus, Arc::new(Access(store.clone())));
    service.register_provider(Arc::new(steam(&server)));
    let sets = service.sets(&game, None).await.unwrap();
    assert_eq!(sets.len(), 1, "one account, one app: one set");
    assert_eq!(sets[0].entry_ids.len(), 2);

    let player_mock = |unlocked: &'static [(&'static str, i64)]| {
        Mock::given(method("GET"))
            .and(path(PLAYER))
            .respond_with(ResponseTemplate::new(200).set_body_json(player(unlocked)))
            .up_to_n_times(1)
    };
    player_mock(&[("ACH_WAKE_UP", 1_300_000_000)])
        .mount(&server)
        .await;
    let cancel = CancellationToken::new();
    let first = service
        .refresh_game(&game, None, &cancel)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(
        (first.sync.status, first.unlocked, first.total),
        (SetStatus::Ready, 1, 3)
    );
    player_mock(&[
        ("ACH_WAKE_UP", 1_300_000_000),
        ("ACH_SECRET", 1_400_000_000),
    ])
    .mount(&server)
    .await;
    let second = service
        .refresh_game(&game, None, &cancel)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(second.unlocked, 2);
    let mut new = vec![];
    while let Ok(event) = events.try_recv() {
        if let Event::AchievementsUnlocked(unlocks) = event {
            new.extend(unlocks.names);
        }
    }
    assert_eq!(new, ["Secret"]);
}

/// Against Steam itself, with `CASKADE_STEAM_KEY`, `CASKADE_STEAM_ID` and
/// optionally `CASKADE_STEAM_APP` (620 by default). Read only.
#[tokio::test]
#[ignore = "needs a Steam Web API key: CASKADE_STEAM_KEY, CASKADE_STEAM_ID"]
async fn live() {
    let key = std::env::var("CASKADE_STEAM_KEY").expect("CASKADE_STEAM_KEY");
    let id = std::env::var("CASKADE_STEAM_ID").expect("CASKADE_STEAM_ID");
    let app = std::env::var("CASKADE_STEAM_APP").unwrap_or_else(|_| "620".into());
    let credentials = StoreCredentials {
        access_token: key,
        refresh_token: None,
        expires_at_ms: None,
        extra: [("steamid".to_owned(), id)].into(),
    };
    let fetched = SteamAchievements::new()
        .fetch(
            &account(),
            Some(&credentials),
            &app,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    println!(
        "{} achievements, {} unlocked",
        fetched.definitions.len(),
        fetched.states.values().filter(|s| s.unlocked).count()
    );
}
