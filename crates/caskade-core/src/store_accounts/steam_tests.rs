//! Steam against the documented Web API answers, served by wiremock.

use super::*;
use crate::{
    bus::Bus,
    db::LibraryStore,
    error::{ErrorCode, ErrorReport},
    model::{Game, GameEntry, GameFilter, InstallLocation, InstallState, ListGames},
    store_accounts::{RefreshError, StoreAccounts, StoreConnectionState},
};
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const KEY: &str = "0123456789ABCDEF0123456789ABCDEF";
const STEAM_ID: &str = "76561197960435530";

fn fixture(name: &str) -> serde_json::Value {
    let text = match name {
        "owned_games" => include_str!("fixtures/steam/owned_games.json"),
        "private" => include_str!("fixtures/steam/private.json"),
        "empty" => include_str!("fixtures/steam/empty.json"),
        "player_summaries" => include_str!("fixtures/steam/player_summaries.json"),
        "resolve_vanity" => include_str!("fixtures/steam/resolve_vanity.json"),
        "resolve_vanity_missing" => include_str!("fixtures/steam/resolve_vanity_missing.json"),
        _ => unreachable!(),
    };
    serde_json::from_str(text).unwrap()
}

async fn answer(server: &MockServer, method_path: &str, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(method_path))
        .and(query_param("key", KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

struct World {
    server: MockServer,
    store: LibraryStore,
    accounts: StoreAccounts,
}

async fn world() -> World {
    let server = MockServer::start().await;
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(Arc::new(SteamProvider::with_base(
        Url::parse(&server.uri()).unwrap(),
    )));
    World {
        server,
        store,
        accounts,
    }
}

fn fields(profile: &str, key: &str) -> AuthInput {
    AuthInput::Fields {
        fields: BTreeMap::from([
            ("profile".to_owned(), profile.to_owned()),
            ("key".to_owned(), key.to_owned()),
        ]),
    }
}

async fn sign_in(world: &World, profile: &str) -> Result<StoreAccount, super::super::ConnectError> {
    let flow = world.accounts.begin_connect(ID, None).await.unwrap();
    world
        .accounts
        .complete_connect(&flow.flow_id, &fields(profile, KEY), None)
        .await
}

async fn signed_in(world: &World) -> StoreAccount {
    answer(
        &world.server,
        "/ISteamUser/GetPlayerSummaries/v2/",
        fixture("player_summaries"),
    )
    .await;
    sign_in(world, STEAM_ID).await.unwrap()
}

#[tokio::test]
async fn the_sign_in_asks_for_a_profile_and_a_key_and_explains_privacy() {
    let challenge = SteamProvider::new().begin_auth("flow").await.unwrap();
    let ids: Vec<_> = challenge
        .fields
        .iter()
        .map(|f| (f.id.as_str(), f.secret))
        .collect();
    assert_eq!(ids, [("profile", false), ("key", true)]);
    assert_eq!(challenge.url.as_deref(), Some(KEY_PAGE));
    assert!(challenge.instructions.unwrap().contains("public"));
}

#[tokio::test]
async fn a_custom_url_name_or_profile_link_signs_in() {
    let world = world().await;
    answer(
        &world.server,
        "/ISteamUser/ResolveVanityURL/v1/",
        fixture("resolve_vanity"),
    )
    .await;
    answer(
        &world.server,
        "/ISteamUser/GetPlayerSummaries/v2/",
        fixture("player_summaries"),
    )
    .await;
    let account = sign_in(&world, "https://steamcommunity.com/id/robinwalker/")
        .await
        .unwrap();
    assert_eq!(
        (account.external_id.as_str(), account.display_name.as_str()),
        (STEAM_ID, "Robin")
    );
    // A profile link names the SteamID: the same account, no lookup.
    let again = sign_in(
        &world,
        &format!("https://steamcommunity.com/profiles/{STEAM_ID}/"),
    )
    .await
    .unwrap();
    assert_eq!(again.id, account.id);
    let requests = world.server.received_requests().await.unwrap();
    let lookups = requests
        .iter()
        .filter(|r| r.url.path().contains("ResolveVanityURL"))
        .count();
    assert_eq!(lookups, 1);
    // The key went in the query only, and never to the account's record.
    let summary = &world.accounts.accounts().await.unwrap()[0];
    assert!(!serde_json::to_string(summary).unwrap().contains(KEY));
}

#[tokio::test]
async fn an_unknown_profile_or_a_refused_key_is_said_plainly() {
    let world = world().await;
    answer(
        &world.server,
        "/ISteamUser/ResolveVanityURL/v1/",
        fixture("resolve_vanity_missing"),
    )
    .await;
    let missing = sign_in(&world, "nobody-here").await.unwrap_err();
    assert!(missing.to_string().contains("nobody-here"), "{missing}");
    assert_eq!(ErrorReport::from_error(&missing).code, ErrorCode::Auth);

    Mock::given(path("/ISteamUser/GetPlayerSummaries/v2/"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&world.server)
        .await;
    let refused = sign_in(&world, STEAM_ID).await.unwrap_err();
    let report = ErrorReport::from_error(&refused);
    assert_eq!(report.code, ErrorCode::Auth);
    assert!(report.summary.contains("Web API key"), "{}", report.summary);
}

#[tokio::test]
async fn owned_games_come_with_play_time_only_where_steam_gives_it() {
    let world = world().await;
    let account = signed_in(&world).await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .and(query_param("steamid", STEAM_ID))
        .and(query_param("include_appinfo", "1"))
        .and(query_param("include_played_free_games", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("owned_games")))
        .mount(&world.server)
        .await;
    let outcome = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!((outcome.added, outcome.partial), (5, false));
    let owned = world.store.entitlements(&account.id).await.unwrap();
    let product = |id: &str| {
        owned
            .iter()
            .find(|e| e.product.product_id == id)
            .unwrap()
            .product
            .clone()
    };
    let portal2 = product("620");
    assert_eq!(portal2.ids, [ExternalId::Steam { app_id: 620 }]);
    assert_eq!(portal2.playtime_minutes, Some(1534));
    assert_eq!(portal2.last_played_ms, Some(1_793_010_000_000));
    // Never played: 0 minutes, and no last play rather than 1970.
    assert_eq!(
        (
            product("400").playtime_minutes,
            product("400").last_played_ms
        ),
        (Some(0), None)
    );
    // Not supplied at all: unknown.
    assert_eq!(product("1172470").playtime_minutes, None);
    assert_eq!(product("999999").title, "Steam app 999999");
    // Steam's play time counts once, refresh after refresh (#169).
    let portal = owned
        .iter()
        .find(|e| e.product.product_id == "620")
        .unwrap()
        .game_id
        .clone()
        .unwrap();
    let playtime = world.store.playtime_breakdown(&portal).await.unwrap();
    assert_eq!(playtime.total_ms, 1534 * 60_000);
    assert_eq!(playtime.last_played_ms, Some(1_793_010_000_000));
    assert_eq!(playtime.sources[0].account_name.as_deref(), Some("Robin"));
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        world
            .store
            .playtime_breakdown(&portal)
            .await
            .unwrap()
            .total_ms,
        1534 * 60_000
    );
    // Unknown stays unknown: no row for a game Steam gave no time for.
    let apex = owned
        .iter()
        .find(|e| e.product.product_id == "1172470")
        .unwrap()
        .game_id
        .clone()
        .unwrap();
    assert!(
        world
            .store
            .playtime_breakdown(&apex)
            .await
            .unwrap()
            .sources
            .is_empty()
    );
    // Owned, not installed.
    assert!(
        world
            .store
            .list_games(&ListGames {
                filter: GameFilter::Installed,
                ..ListGames::default()
            })
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn a_private_library_is_unavailable_not_empty() {
    let world = world().await;
    let account = signed_in(&world).await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("owned_games")))
        .up_to_n_times(1)
        .mount(&world.server)
        .await;
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("private")))
        .mount(&world.server)
        .await;
    let error = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    let report = ErrorReport::from_error(&error);
    assert_eq!(report.code, ErrorCode::Unavailable);
    assert!(report.summary.contains("Privacy"), "{}", report.summary);
    // Nothing was lost.
    let owned = world.store.entitlements(&account.id).await.unwrap();
    assert!(owned.len() == 5 && owned.iter().all(|e| e.lost_at_ms.is_none()));
}

#[tokio::test]
async fn an_empty_library_is_a_real_answer() {
    let world = world().await;
    let account = signed_in(&world).await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("empty")))
        .mount(&world.server)
        .await;
    let outcome = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.added, 0);
}

#[tokio::test]
async fn throttling_and_a_revoked_key_keep_the_library() {
    let world = world().await;
    let account = signed_in(&world).await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
        .up_to_n_times(1)
        .mount(&world.server)
        .await;
    let error = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            RefreshError::Provider(ProviderError::RateLimited {
                retry_after: Some(wait)
            }) if wait.as_secs() == 120
        ),
        "{error:?}"
    );
    let retry_at = world
        .accounts
        .account(&account.id)
        .await
        .unwrap()
        .sync
        .retry_at_ms
        .unwrap();
    assert!(retry_at > crate::now_ms() + 100_000);

    let world = self::world().await;
    let account = signed_in(&world).await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&world.server)
        .await;
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    let summary = world.accounts.accounts().await.unwrap().remove(0);
    assert_eq!(summary.state, StoreConnectionState::Expired);
}

#[tokio::test]
async fn a_large_library_and_a_later_local_install_end_as_one_game_each() {
    let world = world().await;
    let account = signed_in(&world).await;
    let games: Vec<_> = (1..=5000u32)
        .map(|n| {
            serde_json::json!({
                "appid": n * 10,
                "name": format!("Game {n:04}"),
                "playtime_forever": n,
            })
        })
        .collect();
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"response": {"game_count": 5000, "games": games}}),
            ),
        )
        .mount(&world.server)
        .await;
    let started = std::time::Instant::now();
    let first = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(first.added, 5000);
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
    // Steam discovery finds one installed, as its own game.
    let game = Game::new("Game 0062");
    let mut entry = GameEntry::new(&game.id, "steam".to_owned().into(), "620");
    entry.ids = vec![ExternalId::Steam { app_id: 620 }];
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: "/home/robin/.local/share/Steam/steamapps/common/Game".into(),
    });
    world
        .store
        .import_group(&game, &[entry], None)
        .await
        .unwrap();
    assert_eq!(world.accounts.reconcile().await.unwrap(), 1);
    // A second refresh adds nothing and keeps the merged game.
    let second = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!((second.added, second.lost), (0, 0));
    let page = world.store.list_games(&ListGames::default()).await.unwrap();
    assert_eq!(page.total, 5000);
    let installed = world
        .store
        .list_games(&ListGames {
            filter: GameFilter::Installed,
            ..ListGames::default()
        })
        .await
        .unwrap();
    assert_eq!(installed.items.len(), 1);
}

/// A real account: `CASKADE_STEAM_PROFILE` and `CASKADE_STEAM_KEY`.
#[tokio::test]
#[ignore = "needs a Steam account and Web API key"]
async fn live() {
    let (Ok(profile), Ok(key)) = (
        std::env::var("CASKADE_STEAM_PROFILE"),
        std::env::var("CASKADE_STEAM_KEY"),
    ) else {
        panic!("set CASKADE_STEAM_PROFILE and CASKADE_STEAM_KEY");
    };
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(Arc::new(SteamProvider::new()));
    let flow = accounts.begin_connect(ID, None).await.unwrap();
    let account = accounts
        .complete_connect(&flow.flow_id, &fields(&profile, &key), None)
        .await
        .unwrap();
    let outcome = accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    println!("{} signed in; {outcome:?}", account.display_name);
    assert!(outcome.added > 0);
}
