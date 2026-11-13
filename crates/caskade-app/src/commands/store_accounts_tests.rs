//! Store account commands with a fake provider.
use super::*;
use crate::error::ErrorCode;
use caskade_core::{
    bus::Bus,
    store_accounts::{
        AccountProvider, AuthChallenge, AuthKind, LibrarySnapshot, OwnedProduct, ProviderError,
        SignedIn, StoreCredentials,
    },
};

struct Fake;

#[caskade_core::store_accounts::async_trait]
impl AccountProvider for Fake {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "fake".into(),
            name: "Fake Store".into(),
            auth: AuthKind::Launcher,
            library: true,
            playtime: false,
            install: false,
        }
    }
    async fn begin_auth(&self, _flow_id: &str) -> Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge::default())
    }
    async fn complete_auth(
        &self,
        _flow_id: &str,
        _input: &AuthInput,
    ) -> Result<SignedIn, ProviderError> {
        Ok(SignedIn {
            external_id: "me".into(),
            display_name: "Me".into(),
            credentials: None,
        })
    }
    async fn library(
        &self,
        _account: &StoreAccount,
        _credentials: Option<&StoreCredentials>,
        _cancel: &caskade_backup::CancellationToken,
    ) -> Result<LibrarySnapshot, ProviderError> {
        Ok(LibrarySnapshot {
            products: vec![OwnedProduct {
                product_id: "1".into(),
                title: "Owned Game".into(),
                ids: vec![],
                platforms: vec![],
                playtime_minutes: None,
                last_played_ms: None,
                systems: Vec::new(),
            }],
            complete: true,
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn accounts_their_games_and_removal() {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(std::sync::Arc::new(Fake));
    let account = accounts.connect("fake", "me", "Me", None).await.unwrap();

    let providers = store_providers(&accounts);
    assert_eq!(providers[0].name, "Fake Store");
    assert_eq!(providers[0].auth, AuthKind::Launcher);
    let refreshed = store_account_refresh(&accounts, &account.id).await.unwrap();
    assert_eq!(refreshed.added, 1);
    let listed = store_accounts_list(&accounts).await.unwrap();
    assert_eq!(listed[0].owned, 1);
    assert!(listed[0].account.sync.last_ok_ms.is_some());
    let game = store.entitlements(&account.id).await.unwrap()[0]
        .game_id
        .clone()
        .unwrap();

    store_account_disconnect(&accounts, &account.id)
        .await
        .unwrap();
    let refused = store_account_refresh(&accounts, &account.id)
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Invalid);
    let preview = store_account_removal_preview(&accounts, &account.id)
        .await
        .unwrap();
    assert_eq!(preview.removed[0].title, "Owned Game");
    store_account_remove(&accounts, &account.id).await.unwrap();
    assert!(store_accounts_list(&accounts).await.unwrap().is_empty());
    assert!(store.game(&game).await.unwrap().is_none());
}

#[test]
fn only_store_auth_links_are_sign_in_callbacks() {
    for (link, callback) in [
        ("caskade://store-auth?state=1&code=2", true),
        ("caskade:store-auth?state=1", true),
        ("caskade://install?url=https://example.com", false),
        ("caskade://store-auth-evil?state=1", false),
    ] {
        assert_eq!(is_callback(&link.parse().unwrap()), callback, "{link}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_never_answers_with_a_token() {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(std::sync::Arc::new(Fake));
    let flow = store_connect_begin(&accounts, "fake", None).await.unwrap();
    assert_eq!(store_auth_flows(&accounts)[0].flow_id, flow.flow_id);
    let account = store_connect_complete(&accounts, None, &flow.flow_id, &AuthInput::Launcher)
        .await
        .unwrap();
    assert_eq!(account.external_id, "me");
    assert!(store_auth_flows(&accounts).is_empty());
    let again = store_connect_complete(&accounts, None, &flow.flow_id, &AuthInput::Launcher)
        .await
        .unwrap_err();
    assert_eq!(again.code, ErrorCode::Invalid);
    let listed = store_accounts_list(&accounts).await.unwrap();
    let listed = serde_json::to_string(&listed).unwrap();
    assert!(!listed.to_lowercase().contains("token"));
}
