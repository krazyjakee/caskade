//! Signing in, sign-ins kept and renewed, and background refreshes (#165),
//! against a fake browser store and fake keychains.

use super::*;
use crate::secrets::{MemorySecrets, SecretStore};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const TOKEN: &str = "secret-access-token-0123456789";

/// A store that signs in through the browser: the callback's `code` names
/// the account, its tokens expire, and its library needs a live token.
#[derive(Default)]
struct BrowserStore {
    calls: AtomicU32,
    revoked: AtomicBool,
    /// Device sign-ins report pending this many times first.
    pending: AtomicU32,
    /// The next library answer, instead of the default.
    answer: Mutex<Option<std::result::Result<LibrarySnapshot, ProviderError>>>,
    /// Tokens the library saw.
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl AccountProvider for BrowserStore {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "browser".into(),
            name: "Browser Store".into(),
            auth: AuthKind::Browser,
            library: true,
            playtime: false,
            install: false,
        }
    }
    async fn begin_auth(&self, flow_id: &str) -> std::result::Result<AuthChallenge, ProviderError> {
        Ok(AuthChallenge {
            url: Some(format!("https://store.example/login?state={flow_id}")),
            user_code: None,
            ..AuthChallenge::default()
        })
    }
    async fn complete_auth(
        &self,
        _flow_id: &str,
        input: &AuthInput,
    ) -> std::result::Result<SignedIn, ProviderError> {
        let who = match input {
            AuthInput::Callback { params } => params.get("code").cloned().unwrap_or_default(),
            AuthInput::Code { code } if code == "wrong" => {
                return Err(ProviderError::Rejected("unknown key".into()));
            }
            AuthInput::Code { code } => code.clone(),
            AuthInput::Poll => {
                if self.pending.load(Ordering::SeqCst) > 0 {
                    self.pending.fetch_sub(1, Ordering::SeqCst);
                    return Err(ProviderError::Pending);
                }
                "device".into()
            }
            AuthInput::Launcher | AuthInput::Fields { .. } => {
                return Err(ProviderError::Other("not this way".into()));
            }
        };
        Ok(SignedIn {
            external_id: who.clone(),
            display_name: who.to_uppercase(),
            credentials: Some(StoreCredentials {
                access_token: TOKEN.into(),
                refresh_token: Some("refresh".into()),
                expires_at_ms: Some(crate::now_ms() + 3_600_000),
                extra: Default::default(),
            }),
        })
    }
    async fn refresh_credentials(
        &self,
        credentials: &StoreCredentials,
    ) -> std::result::Result<StoreCredentials, ProviderError> {
        match credentials.refresh_token.as_deref() {
            Some("refresh") => Ok(StoreCredentials {
                access_token: "renewed".into(),
                expires_at_ms: Some(crate::now_ms() + 3_600_000),
                ..credentials.clone()
            }),
            _ => Err(ProviderError::Expired("refresh token revoked".into())),
        }
    }
    async fn revoke(
        &self,
        _credentials: &StoreCredentials,
    ) -> std::result::Result<(), ProviderError> {
        self.revoked.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn library(
        &self,
        _account: &StoreAccount,
        credentials: Option<&StoreCredentials>,
        _cancel: &CancellationToken,
    ) -> std::result::Result<LibrarySnapshot, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        if let Some(answer) = crate::lock(&self.answer).take() {
            return answer;
        }
        let token = credentials
            .map(|c| c.access_token.clone())
            .unwrap_or_default();
        crate::lock(&self.seen).push(token);
        Ok(LibrarySnapshot {
            products: vec![OwnedProduct {
                product_id: "1".into(),
                title: "Bought".into(),
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

/// A keychain that is never there (no Secret Service on this desktop).
struct NoKeychain;

#[async_trait]
impl SecretStore for NoKeychain {
    async fn get(&self, _: &str) -> Result<Option<String>> {
        Err(Error::Invalid("no keychain".into()))
    }
    async fn set(&self, _: &str, _: &str) -> Result<()> {
        Err(Error::Invalid("no keychain".into()))
    }
    async fn delete(&self, _: &str) -> Result<()> {
        Err(Error::Invalid("no keychain".into()))
    }
}

struct World {
    store: LibraryStore,
    keychain: MemorySecrets,
    accounts: StoreAccounts,
    provider: Arc<BrowserStore>,
}

async fn world() -> World {
    let store = LibraryStore::in_memory().await.unwrap();
    let keychain = MemorySecrets::default();
    let accounts =
        StoreAccounts::with_secrets(store.clone(), Bus::default(), Arc::new(keychain.clone()));
    let provider = Arc::new(BrowserStore::default());
    accounts.register_provider(provider.clone());
    World {
        store,
        keychain,
        accounts,
        provider,
    }
}

fn callback(state: &str, code: &str) -> AuthInput {
    AuthInput::Callback {
        params: [("state", state), ("code", code)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
    }
}

async fn signed_in(world: &World, who: &str) -> StoreAccount {
    let flow = world.accounts.begin_connect("browser", None).await.unwrap();
    world
        .accounts
        .complete_connect(&flow.flow_id, &callback(&flow.flow_id, who), None)
        .await
        .unwrap()
}

async fn summary(world: &World, id: &str) -> StoreAccountSummary {
    world
        .accounts
        .accounts()
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.account.id == id)
        .unwrap()
}

#[tokio::test]
async fn a_browser_sign_in_checks_its_state_and_keeps_the_token_in_the_keychain() {
    let world = world().await;
    let flow = world.accounts.begin_connect("browser", None).await.unwrap();
    assert!(
        flow.challenge
            .url
            .as_deref()
            .unwrap()
            .contains(&flow.flow_id)
    );
    assert_eq!(world.accounts.flows().len(), 1);
    // A callback for another flow is refused, and cancels this one.
    let forged = world
        .accounts
        .complete_connect(&flow.flow_id, &callback("someone-else", "me"), None)
        .await
        .unwrap_err();
    assert!(matches!(forged, ConnectError::Mismatch), "{forged:?}");
    assert!(world.accounts.flows().is_empty());
    assert!(world.store.store_accounts().await.unwrap().is_empty());

    let account = signed_in(&world, "me").await;
    assert_eq!(
        (account.external_id.as_str(), account.display_name.as_str()),
        ("me", "ME")
    );
    let kept = world.keychain.get(&account.id).await.unwrap().unwrap();
    assert!(kept.contains(TOKEN));
    // Nothing a webview or a backup sees holds the token.
    let summary = summary(&world, &account.id).await;
    assert_eq!(summary.state, StoreConnectionState::Connected);
    assert!(!summary.session_only);
    assert!(!serde_json::to_string(&summary).unwrap().contains(TOKEN));
    assert!(
        !format!(
            "{:?}",
            world.accounts.inner.credentials.get(&account.id).await
        )
        .contains(TOKEN)
    );
    let rows = world.store.store_accounts().await.unwrap();
    assert!(!serde_json::to_string(&rows).unwrap().contains(TOKEN));

    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(*crate::lock(&world.provider.seen), [TOKEN]);
}

#[tokio::test]
async fn sign_ins_expire_and_cancel_cleanly() {
    let world = world().await;
    let flow = world.accounts.begin_connect("browser", None).await.unwrap();
    world.accounts.cancel_connect(&flow.flow_id);
    let cancelled = world
        .accounts
        .complete_connect(&flow.flow_id, &callback(&flow.flow_id, "me"), None)
        .await
        .unwrap_err();
    assert!(matches!(cancelled, ConnectError::Expired));
    let flow = world.accounts.begin_connect("browser", None).await.unwrap();
    world.accounts.expire_flows();
    let expired = world
        .accounts
        .complete_connect(&flow.flow_id, &callback(&flow.flow_id, "me"), None)
        .await
        .unwrap_err();
    assert!(matches!(expired, ConnectError::Expired));
    assert_eq!(
        ErrorReport::from_error(&expired).code,
        crate::error::ErrorCode::Invalid
    );
    // A wrong key leaves the flow open to try again.
    let flow = world.accounts.begin_connect("browser", None).await.unwrap();
    let wrong = world
        .accounts
        .complete_connect(
            &flow.flow_id,
            &AuthInput::Code {
                code: "wrong".into(),
            },
            None,
        )
        .await
        .unwrap_err();
    let report = ErrorReport::from_error(&wrong);
    assert_eq!(report.code, crate::error::ErrorCode::Auth);
    world
        .accounts
        .complete_connect(
            &flow.flow_id,
            &AuthInput::Code {
                code: "key-user".into(),
            },
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_device_sign_in_waits_until_its_done() {
    let world = world().await;
    world.provider.pending.store(2, Ordering::SeqCst);
    let flow = world.accounts.begin_connect("browser", None).await.unwrap();
    for _ in 0..2 {
        let pending = world
            .accounts
            .complete_connect(&flow.flow_id, &AuthInput::Poll, None)
            .await
            .unwrap_err();
        assert!(matches!(
            pending,
            ConnectError::Provider(ProviderError::Pending)
        ));
    }
    let account = world
        .accounts
        .complete_connect(&flow.flow_id, &AuthInput::Poll, None)
        .await
        .unwrap();
    assert_eq!(account.external_id, "device");
}

#[tokio::test]
async fn reconnecting_never_takes_another_accounts_sign_in() {
    let world = world().await;
    let mine = signed_in(&world, "me").await;
    let before = world.keychain.get(&mine.id).await.unwrap();
    let flow = world
        .accounts
        .begin_connect("browser", Some(mine.id.clone()))
        .await
        .unwrap();
    assert_eq!(
        summary(&world, &mine.id).await.state,
        StoreConnectionState::Authenticating
    );
    let other = world
        .accounts
        .complete_connect(&flow.flow_id, &callback(&flow.flow_id, "sibling"), None)
        .await
        .unwrap_err();
    assert!(
        matches!(other, ConnectError::OtherAccount { .. }),
        "{other:?}"
    );
    assert_eq!(world.store.store_accounts().await.unwrap().len(), 1);
    assert_eq!(world.keychain.get(&mine.id).await.unwrap(), before);
    // Signing in as the same account again reuses its record.
    let flow = world
        .accounts
        .begin_connect("browser", Some(mine.id.clone()))
        .await
        .unwrap();
    let again = world
        .accounts
        .complete_connect(&flow.flow_id, &callback(&flow.flow_id, "me"), None)
        .await
        .unwrap();
    assert_eq!(again.id, mine.id);
}

#[tokio::test]
async fn an_expiring_token_is_renewed_and_a_dead_one_asks_to_reconnect() {
    let world = world().await;
    let account = signed_in(&world, "me").await;
    let mut credentials = world
        .accounts
        .inner
        .credentials
        .get(&account.id)
        .await
        .unwrap();
    credentials.expires_at_ms = Some(0);
    world
        .accounts
        .inner
        .credentials
        .set(&account.id, &credentials)
        .await
        .unwrap();
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(*crate::lock(&world.provider.seen), ["renewed"]);
    assert!(
        world
            .keychain
            .get(&account.id)
            .await
            .unwrap()
            .unwrap()
            .contains("renewed")
    );

    credentials.refresh_token = Some("revoked".into());
    world
        .accounts
        .inner
        .credentials
        .set(&account.id, &credentials)
        .await
        .unwrap();
    let error = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        ErrorReport::from_error(&error).code,
        crate::error::ErrorCode::Auth
    );
    assert_eq!(
        summary(&world, &account.id).await.state,
        StoreConnectionState::Expired
    );
    // The library it brought stays.
    assert_eq!(
        world.store.entitlements(&account.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn without_a_keychain_a_sign_in_lasts_the_session() {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = StoreAccounts::with_secrets(store, Bus::default(), Arc::new(NoKeychain));
    let provider = Arc::new(BrowserStore::default());
    accounts.register_provider(provider.clone());
    let flow = accounts.begin_connect("browser", None).await.unwrap();
    let account = accounts
        .complete_connect(&flow.flow_id, &callback(&flow.flow_id, "me"), None)
        .await
        .unwrap();
    let summary = accounts.accounts().await.unwrap().remove(0);
    assert!(summary.session_only);
    assert_eq!(summary.state, StoreConnectionState::Connected);
    accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(*crate::lock(&provider.seen), [TOKEN]);
}

#[tokio::test]
async fn after_a_restart_offline_the_sign_in_and_library_remain() {
    let world = world().await;
    let account = signed_in(&world, "me").await;
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    // A new process, same database and keychain, and no network.
    let restarted = StoreAccounts::with_secrets(
        world.store.clone(),
        Bus::default(),
        Arc::new(world.keychain.clone()),
    );
    let provider = Arc::new(BrowserStore::default());
    *crate::lock(&provider.answer) = Some(Err(ProviderError::Offline("no route".into())));
    restarted.register_provider(provider);
    assert_eq!(
        restarted.accounts().await.unwrap()[0].state,
        StoreConnectionState::Connected
    );
    let error = restarted
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        ErrorReport::from_error(&error).code,
        crate::error::ErrorCode::Offline
    );
    let summary = restarted.accounts().await.unwrap().remove(0);
    assert_eq!(
        (summary.state, summary.owned),
        (StoreConnectionState::Offline, 1)
    );
}

#[tokio::test]
async fn retry_after_is_waited_out_by_every_refresh() {
    let world = world().await;
    let account = signed_in(&world, "me").await;
    *crate::lock(&world.provider.answer) = Some(Err(ProviderError::RateLimited {
        retry_after: Some(Duration::from_secs(7200)),
    }));
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    let calls = world.provider.calls.load(Ordering::SeqCst);
    let error = world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RefreshError::Provider(ProviderError::RateLimited { .. })
    ));
    assert_eq!(
        world.provider.calls.load(Ordering::SeqCst),
        calls,
        "not asked again"
    );
    let retry_at = world
        .accounts
        .account(&account.id)
        .await
        .unwrap()
        .sync
        .retry_at_ms
        .unwrap();
    assert!(retry_at > crate::now_ms() + 7_000_000);
    assert!(world.accounts.run_due(crate::now_ms()).await.is_empty());
}

#[tokio::test]
async fn background_refreshes_are_opt_in_and_share_the_running_one() {
    let world = world().await;
    let settings = crate::settings::SettingsService::new(world.store.clone());
    world.accounts.set_settings(settings.clone());
    let account = signed_in(&world, "me").await;
    assert!(
        world.accounts.run_due(crate::now_ms()).await.is_empty(),
        "off by default"
    );
    settings
        .set(
            NAMESPACE,
            serde_json::json!({"backgroundRefresh": true, "intervalHours": 6, "playtimeImport": "always"}),
            None,
        )
        .await
        .unwrap();
    // A manual refresh and a due one at once: one request.
    let cancel = CancellationToken::new();
    let (manual, due) = tokio::join!(world.accounts.refresh(&account.id, &cancel), async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        world.accounts.run_due(crate::now_ms()).await
    });
    manual.unwrap();
    assert_eq!(due, std::slice::from_ref(&account.id));
    assert_eq!(world.provider.calls.load(Ordering::SeqCst), 1);
    // Just refreshed: not due for six hours.
    assert!(world.accounts.run_due(crate::now_ms()).await.is_empty());
    assert_eq!(
        world
            .accounts
            .run_due(crate::now_ms() + 7 * 3_600_000)
            .await,
        std::slice::from_ref(&account.id)
    );
    // A signed-out account waits for the user.
    world
        .accounts
        .inner
        .credentials
        .delete(&account.id)
        .await
        .unwrap();
    assert!(
        world
            .accounts
            .run_due(crate::now_ms() + 30 * 3_600_000)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn disconnecting_revokes_and_forgets_the_sign_in_but_keeps_the_games() {
    let world = world().await;
    let account = signed_in(&world, "me").await;
    world
        .accounts
        .refresh(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    world.accounts.disconnect(&account.id).await.unwrap();
    assert!(world.provider.revoked.load(Ordering::SeqCst));
    assert_eq!(world.keychain.get(&account.id).await.unwrap(), None);
    let summary = summary(&world, &account.id).await;
    assert_eq!(
        (summary.state, summary.owned),
        (StoreConnectionState::NotConnected, 1)
    );
    // Connecting again brings the same account back.
    assert_eq!(signed_in(&world, "me").await.id, account.id);
    assert_eq!(
        summary_state(&world, &account.id).await,
        StoreConnectionState::Connected
    );
}

async fn summary_state(world: &World, id: &str) -> StoreConnectionState {
    summary(world, id).await.state
}
