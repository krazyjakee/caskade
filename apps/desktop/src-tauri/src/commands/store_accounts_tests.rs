//! Store account commands on Tauri's mock runtime, with a fake provider.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    store_accounts::{
        AccountProvider, AuthChallenge, AuthInput, AuthKind, LibrarySnapshot, OwnedProduct,
        ProviderError, SignedIn, StoreAccount, StoreCredentials,
    },
};
use serde_json::json;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

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

#[test]
fn accounts_their_games_and_removal_over_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(std::sync::Arc::new(Fake));
    let account =
        tauri::async_runtime::block_on(accounts.connect("fake", "me", "Me", None)).unwrap();
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            store_providers,
            store_accounts_list,
            store_account_refresh,
            store_account_disconnect,
            store_account_removal_preview,
            store_account_remove,
            game_ownership
        ]);
    let app = mock_builder()
        .manage(accounts)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let providers = invoke(&window, "store_providers", json!({})).unwrap();
    assert_eq!(providers[0]["name"], "Fake Store");
    assert_eq!(providers[0]["auth"], "launcher");
    let refreshed = invoke(&window, "store_account_refresh", json!({"id": account.id})).unwrap();
    assert_eq!(refreshed["added"], 1);
    let listed = invoke(&window, "store_accounts_list", json!({})).unwrap();
    assert_eq!(listed[0]["owned"], 1);
    assert!(listed[0]["account"]["sync"]["lastOkMs"].is_number());
    let game = tauri::async_runtime::block_on(store.entitlements(&account.id)).unwrap()[0]
        .game_id
        .clone()
        .unwrap();
    let owners = invoke(&window, "game_ownership", json!({"gameId": game})).unwrap();
    assert_eq!(owners[0]["displayName"], "Me");

    invoke(
        &window,
        "store_account_disconnect",
        json!({"id": account.id}),
    )
    .unwrap();
    let refused = invoke(&window, "store_account_refresh", json!({"id": account.id})).unwrap_err();
    assert_eq!(refused["code"], "invalid");
    let preview = invoke(
        &window,
        "store_account_removal_preview",
        json!({"id": account.id}),
    )
    .unwrap();
    assert_eq!(preview["removed"][0]["title"], "Owned Game");
    invoke(&window, "store_account_remove", json!({"id": account.id})).unwrap();
    assert_eq!(
        invoke(&window, "store_accounts_list", json!({})).unwrap(),
        json!([])
    );
    assert!(
        tauri::async_runtime::block_on(store.game(&game))
            .unwrap()
            .is_none()
    );
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

#[test]
fn a_sign_in_over_ipc_never_answers_with_a_token() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let accounts = StoreAccounts::new(store.clone(), Bus::default());
    accounts.register_provider(std::sync::Arc::new(Fake));
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            store_connect_begin,
            store_connect_complete::<MockRuntime>,
            store_connect_cancel,
            store_auth_flows,
            store_accounts_list
        ]);
    let app = mock_builder()
        .manage(accounts)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let flow = invoke(
        &window,
        "store_connect_begin",
        json!({"provider": "fake", "reconnect": null}),
    )
    .unwrap();
    let flow_id = flow["flowId"].as_str().unwrap().to_owned();
    assert_eq!(
        invoke(&window, "store_auth_flows", json!({})).unwrap()[0]["flowId"],
        flow_id
    );
    let account = invoke(
        &window,
        "store_connect_complete",
        json!({"flowId": flow_id, "input": {"type": "launcher"}}),
    )
    .unwrap();
    assert_eq!(account["externalId"], "me");
    assert!(
        invoke(&window, "store_auth_flows", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let again = invoke(
        &window,
        "store_connect_complete",
        json!({"flowId": flow_id, "input": {"type": "launcher"}}),
    )
    .unwrap_err();
    assert_eq!(again["code"], "invalid");
    let listed = invoke(&window, "store_accounts_list", json!({})).unwrap();
    assert!(!listed.to_string().to_lowercase().contains("token"));
}
