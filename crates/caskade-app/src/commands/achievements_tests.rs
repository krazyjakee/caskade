//! Achievements: a game's sets before and after a refresh.
use super::*;
use caskade_achievements::achievements::{
    AccountAccess, AchievementCapabilities, AchievementDefinition, AchievementProvider,
    AchievementState, FetchError, Fetched, SetStatus,
};
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    model::{ExternalId, Game, GameEntry, SourceId},
    store_accounts::{ProviderError, StoreAccount, StoreCredentials, SyncState},
};
use std::sync::Arc;

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

#[tokio::test(flavor = "multi_thread")]
async fn a_game_s_achievements_are_listed_and_refreshed() {
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
    let bus = Bus::default();
    let achievements = Achievements::new(
        store.clone(),
        bus.clone(),
        Arc::new(Accounts(store.clone())),
    );
    achievements.register_provider(Arc::new(Steam));
    let accounts = AccountService::open(store.clone(), bus.clone())
        .await
        .unwrap();
    let sets = game_achievements(&achievements, &accounts, game.clone())
        .await
        .unwrap();
    assert_eq!(sets[0].sync.status, SetStatus::NotFetched);
    assert_eq!(sets[0].account.provider_name, "Steam");
    let sets = game_achievements_refresh(&achievements, &accounts, game.clone())
        .await
        .unwrap();
    assert_eq!(sets[0].sync.status, SetStatus::Ready);
    assert_eq!(
        sets[0].achievements[0].state.unlocked_at_ms,
        Some(1_700_000_000_000)
    );
    assert!(!game_achievements_cancel(&achievements, game).await.unwrap());
}
