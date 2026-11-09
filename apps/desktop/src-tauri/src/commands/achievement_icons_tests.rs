//! Achievement icons through IPC (#190): only an achievement of the game's
//! own sets has an icon to load, through the artwork cache.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_achievements::achievements::{
    AccountAccess, AchievementCapabilities, AchievementDefinition, AchievementProvider,
    AchievementState, FetchError, Fetched,
};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    model::{ExternalId, Game, GameEntry, SourceId},
    store_accounts::{ProviderError, StoreAccount, StoreCredentials, SyncState},
};
use serde_json::json;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

struct Steam {
    icon: String,
}

#[caskade_achievements::achievements::async_trait]
impl AchievementProvider for Steam {
    fn id(&self) -> String {
        "steam".into()
    }
    fn capabilities(&self) -> AchievementCapabilities {
        AchievementCapabilities::default()
    }
    fn product(&self, id: &ExternalId) -> Option<String> {
        match id {
            ExternalId::Steam { app_id } => Some(app_id.to_string()),
            _ => None,
        }
    }
    async fn fetch(
        &self,
        _: &StoreAccount,
        _: Option<&StoreCredentials>,
        _: &str,
        _: &caskade_achievements::achievements::CancellationToken,
    ) -> Result<Fetched, FetchError> {
        Ok(Fetched {
            definitions: vec![AchievementDefinition {
                id: "WIN".into(),
                name: "Win".into(),
                description: None,
                hidden: false,
                icon_url: Some(self.icon.clone()),
                locked_icon_url: None,
                progress_max: None,
                rarity_percent: None,
                points: None,
            }],
            states: [("WIN".to_owned(), AchievementState::default())].into(),
            complete: true,
        })
    }
}

struct Accounts(LibraryStore);

#[caskade_achievements::achievements::async_trait]
impl AccountAccess for Accounts {
    async fn accounts(&self) -> caskade_core::Result<Vec<StoreAccount>> {
        self.0.store_accounts().await
    }
    async fn credentials(
        &self,
        _: &StoreAccount,
    ) -> Result<Option<StoreCredentials>, ProviderError> {
        Ok(None)
    }
    fn provider_name(&self, _: &str) -> Option<String> {
        Some("Steam".into())
    }
}

/// An 8×8 PNG.
fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2,
        0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 112, 104, 56, 128,
        21, 49, 12, 45, 9, 0, 98, 243, 96, 1, 116, 160, 148, 195, 0, 0, 0, 0, 73, 69, 78, 68, 174,
        66, 96, 130,
    ]
}

#[test]
fn an_achievement_s_icon_loads_through_the_cache_and_nothing_else_does() {
    let temp = tempfile::tempdir().unwrap();
    let (server, store, game) = tauri::async_runtime::block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/win.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png()))
            .mount(&server)
            .await;
        let store = LibraryStore::in_memory().await.unwrap();
        store
            .save_store_account(&StoreAccount {
                id: "acct".into(),
                provider: "steam".into(),
                external_id: "7656".into(),
                display_name: "Player".into(),
                owner_id: None,
                enabled: true,
                sync: SyncState::default(),
                created_at_ms: 0,
            })
            .await
            .unwrap();
        let game = Game::new("Portal 2");
        let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "p2");
        entry.ids = vec![ExternalId::Steam { app_id: 620 }];
        let game = store
            .import_group(&game, &[entry], None)
            .await
            .unwrap()
            .game_id;
        (server, store, game)
    });
    let bus = Bus::default();
    let achievements = Achievements::new(
        store.clone(),
        bus.clone(),
        Arc::new(Accounts(store.clone())),
    );
    achievements.register_provider(Arc::new(Steam {
        icon: format!("{}/win.png", server.uri()),
    }));
    tauri::async_runtime::block_on(achievements.refresh_game_now(&game, None)).unwrap();
    let accounts =
        tauri::async_runtime::block_on(AccountService::open(store.clone(), bus.clone())).unwrap();
    // The test server is local, which a real catalogue of icons never is.
    let cache = ArtworkCache::new(temp.path().join("artwork"))
        .unwrap()
        .allowing_local_metadata();
    let app = mock_builder()
        .manage(achievements)
        .manage(accounts)
        .manage(AchievementIcons(Some(cache)))
        .invoke_handler(tauri::generate_handler![game_achievement_icon])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let icon = |achievement: &str, account: &str, locked: bool| {
        invoke(
            &window,
            "game_achievement_icon",
            json!({"gameId": game, "storeAccountId": account, "productId": "620",
                "achievementId": achievement, "locked": locked}),
        )
        .unwrap()
    };
    let loaded = icon("WIN", "acct", false);
    assert!(
        loaded["thumbnails"]
            .as_array()
            .is_some_and(|thumbnails| !thumbnails.is_empty()),
        "{loaded}"
    );
    // Locked without a locked icon: its own icon (the page greys it).
    assert!(icon("WIN", "acct", true).is_object());
    // Nothing outside the game's sets.
    assert!(icon("LOSE", "acct", false).is_null());
    assert!(icon("WIN", "someone-else", false).is_null());
}
