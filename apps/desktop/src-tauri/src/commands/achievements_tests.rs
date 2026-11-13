//! Achievements through IPC: a game's sets before and after a refresh.
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

struct Steam;
#[caskade_achievements::achievements::async_trait]
impl AchievementProvider for Steam {
    fn id(&self) -> String {
        "steam".into()
    }
    fn capabilities(&self) -> AchievementCapabilities {
        AchievementCapabilities {
            unlock_times: true,
            ..Default::default()
        }
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
                icon_url: None,
                locked_icon_url: None,
                progress_max: None,
                rarity_percent: None,
                points: None,
            }],
            states: [(
                "WIN".to_owned(),
                AchievementState {
                    unlocked: true,
                    unlocked_at_ms: Some(1_700_000_000_000),
                    ..Default::default()
                },
            )]
            .into(),
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

#[test]
fn a_game_s_achievements_are_listed_and_refreshed_through_ipc() {
    let (store, game) = tauri::async_runtime::block_on(async {
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
        (store, game)
    });
    let bus = Bus::default();
    let achievements = Achievements::new(
        store.clone(),
        bus.clone(),
        Arc::new(Accounts(store.clone())),
    );
    achievements.register_provider(Arc::new(Steam));
    let accounts =
        tauri::async_runtime::block_on(AccountService::open(store.clone(), bus.clone())).unwrap();
    let app = mock_builder()
        .manage(achievements)
        .manage(accounts)
        .invoke_handler(tauri::generate_handler![
            game_achievements,
            game_achievements_refresh,
            game_achievements_cancel
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let sets = invoke(&window, "game_achievements", json!({"gameId": game})).unwrap();
    assert_eq!(sets[0]["sync"]["status"], "not_fetched");
    assert_eq!(sets[0]["account"]["providerName"], "Steam");
    let sets = invoke(
        &window,
        "game_achievements_refresh",
        json!({"gameId": game}),
    )
    .unwrap();
    assert_eq!(sets[0]["sync"]["status"], "ready");
    assert_eq!(
        sets[0]["achievements"][0]["state"]["unlockedAtMs"],
        1_700_000_000_000_i64
    );
    assert_eq!(
        invoke(&window, "game_achievements_cancel", json!({"gameId": game})).unwrap(),
        false
    );
}
