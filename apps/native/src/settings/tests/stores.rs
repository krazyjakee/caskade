//! Settings → Store accounts (#104), ported from the webview's
//! `StoreAccountsSettings` tests over the fixtures' in-memory store
//! accounts and a fake store that signs in with an API key: connecting
//! shows the sign-in and adds the account, which reads its health;
//! Disconnect asks first and keeps it; Remove says what leaves and takes
//! it out; the refresh preferences save.

use super::{is_inside, label_of, open};
use crate::frame::tests::{ONE, Window, source};
use caskade_core::store_accounts::{
    AccountProvider, AuthChallenge, AuthInput, AuthKind, LibrarySnapshot, ProviderCapabilities,
    ProviderError, SignedIn, StoreAccount, StoreCredentials,
};
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::{Value, json};
use std::sync::Arc;

use caskade_core::storage::CancellationToken;
use caskade_core::store_accounts::async_trait;

/// A store that signs in with any API key and owns nothing.
struct Fakestore;

#[async_trait]
impl AccountProvider for Fakestore {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "fakestore".into(),
            name: "Fakestore".into(),
            auth: AuthKind::ApiKey,
            library: true,
            playtime: false,
            install: false,
        }
    }

    async fn begin_auth(&self, _flow_id: &str) -> Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge {
            url: Some("https://fakestore.invalid/keys".into()),
            ..AuthChallenge::default()
        })
    }

    async fn complete_auth(
        &self,
        _flow_id: &str,
        input: &AuthInput,
    ) -> Result<SignedIn, ProviderError> {
        match input {
            AuthInput::Code { code } if code == "good-key" => Ok(SignedIn {
                external_id: "player-one".into(),
                display_name: "Player One".into(),
                credentials: Some(StoreCredentials {
                    access_token: code.clone(),
                    refresh_token: None,
                    expires_at_ms: None,
                    extra: Default::default(),
                }),
            }),
            _ => Err(ProviderError::Other("That key isn't right.".into())),
        }
    }

    async fn library(
        &self,
        _account: &StoreAccount,
        _credentials: Option<&StoreCredentials>,
        _cancel: &CancellationToken,
    ) -> Result<LibrarySnapshot, ProviderError> {
        Ok(LibrarySnapshot {
            products: Vec::new(),
            complete: true,
        })
    }
}

fn stores() -> Window {
    let stores = &source().stores;
    if stores.service().providers().is_empty() {
        stores.service().register_provider(Arc::new(Fakestore));
    }
    let mut window = open("/settings?tab=stores");
    window.settle_until("Store accounts", |window| {
        window.find("data-provider", "fakestore").is_some()
    });
    window
}

fn button(window: &Window, within: blitz_dom::NodeId, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| is_inside(window, button, within) && window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

#[test]
fn connects_disconnects_and_removes_an_account() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let mut window = stores();
    let section = window.get("data-provider", "fakestore");
    assert!(
        window
            .text(section)
            .contains("Brings in every game your Fakestore account owns, installed or not.")
    );
    assert!(
        window
            .text(section)
            .contains("No Fakestore account is connected.")
    );
    // Connect starts a sign-in: where to get a key, and a field for it.
    let connect = button(&window, section, "Connect");
    press(&mut window, connect);
    window.settle_until("the sign-in", |window| {
        window.find("data-part", "sign-in").is_some()
    });
    let sign_in = window.get("data-part", "sign-in");
    assert_eq!(
        window.attr(sign_in, "aria-label").as_deref(),
        Some("Sign in to Fakestore")
    );
    assert!(window.text(sign_in).contains("Get a key"));
    let field = window
        .find_all("data-part", "input")
        .into_iter()
        .find(|&input| is_inside(&window, input, sign_in))
        .unwrap();
    // A wrong key says so and stays.
    window.focus(field);
    window.type_text("bad-key");
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("refused", |window| {
        window
            .find("data-part", "sign-in")
            .is_some_and(|sign_in| window.text(sign_in).contains("That key isn't right."))
    });
    let field = window
        .find_all("data-part", "input")
        .into_iter()
        .find(|&input| is_inside(&window, input, window.get("data-part", "sign-in")))
        .unwrap();
    window.focus(field);
    window.key(Key::Named(NamedKey::End));
    for _ in 0..10 {
        window.key(Key::Named(NamedKey::Backspace));
    }
    window.type_text("good-key");
    window.key(Key::Named(NamedKey::Enter));
    window.settle_until("connected", |window| {
        window
            .find_all("data-part", "store-account")
            .iter()
            .any(|&account| window.text(account).contains("Player One"))
            && window.find("data-part", "sign-in").is_none()
    });
    let account = window
        .find_all("data-part", "store-account")
        .into_iter()
        .find(|&account| window.text(account).contains("Player One"))
        .unwrap();
    let refresh = window.get("aria-label", "Refresh Player One");
    assert!(is_inside(&window, refresh, account));
    // Disconnect asks first, then keeps the account, disconnected.
    let disconnect = button(&window, account, "Disconnect…");
    press(&mut window, disconnect);
    window.settle_until("the question", |window| {
        window
            .find("role", "alertdialog")
            .is_some_and(|dialog| window.text(dialog).contains("Disconnect Player One?"))
    });
    let dialog = window.get("role", "alertdialog");
    let confirm = button(&window, dialog, "Disconnect");
    press(&mut window, confirm);
    window.settle_until("disconnected", |window| {
        window.find("role", "alertdialog").is_none()
            && window
                .find_all("data-part", "store-account")
                .iter()
                .any(|&account| window.text(account).contains("Disconnected"))
    });
    // Remove says what leaves, then takes it out.
    let account = window
        .find_all("data-part", "store-account")
        .into_iter()
        .find(|&account| window.text(account).contains("Player One"))
        .unwrap();
    let remove = button(&window, account, "Remove…");
    press(&mut window, remove);
    window.settle_until("the preview", |window| {
        window.find("role", "alertdialog").is_some_and(|dialog| {
            window
                .text(dialog)
                .contains("0 games only this account brought leave your library.")
        })
    });
    let dialog = window.get("role", "alertdialog");
    let confirm = button(&window, dialog, "Remove");
    press(&mut window, confirm);
    window.settle_until("removed", |window| {
        window.find("role", "alertdialog").is_none()
            && window
                .find_all("data-part", "store-account")
                .iter()
                .all(|&account| !window.text(account).contains("Player One"))
    });
}

#[test]
fn the_refresh_preferences_save() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let settings = source().settings.clone();
    let before = caskade_app::runtime::block_on(settings.get("core.storeAccounts"))
        .unwrap()
        .value;
    struct Restore(caskade_ui_data::Settings, Value);
    impl Drop for Restore {
        fn drop(&mut self) {
            let value = self.1.clone();
            let _ =
                caskade_app::runtime::block_on(self.0.update("core.storeAccounts", move |_| value));
        }
    }
    let _restore = Restore(settings.clone(), before.clone());
    let mut window = stores();
    let row = window
        .find_all("data-cs", "setting-row")
        .into_iter()
        .find(|&row| label_of(&window, row) == "Refresh in the background")
        .expect("the refresh row");
    let switch = window
        .find_all("role", "switch")
        .into_iter()
        .find(|&switch| is_inside(&window, switch, row))
        .unwrap();
    window.focus(switch);
    window.key(Key::Character(" ".into()));
    window.settle_until("saved", |_| {
        caskade_app::runtime::block_on(settings.get("core.storeAccounts"))
            .unwrap()
            .value["backgroundRefresh"]
            != before["backgroundRefresh"]
    });
    assert_ne!(before["backgroundRefresh"], json!(null));
}
