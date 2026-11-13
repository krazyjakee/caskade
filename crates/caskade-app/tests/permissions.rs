//! The permission gate in front of real commands (#205), as a shell holds
//! it: `Guard::check` before the command function runs. Install requests
//! from deep links, drops and pastes from an account that may not install
//! are refused with nothing queued. (The rules themselves are
//! `permissions_tests`' in the crate.)
use caskade_app::{
    commands::{
        install_requests::{
            InstallRequests, pending_install_requests, request_dropped_install,
            request_install_input,
        },
        settings::settings_set,
        storage::storage_move_and_remove,
        streaming::streaming_set_quality,
    },
    error::{CommandError, ErrorCode},
    events::{Event, InstallRefused},
    permissions::Guard,
};
use caskade_core::{
    accounts::{
        Account, AccountAccess, AccountRole, AccountService, AccountUpdate, DEFAULT_AVATAR,
        NewAccount, Permissions,
    },
    bus::Bus,
    db::LibraryStore,
    install::{InstallOptions, InstallService},
    settings::{SettingsService, StreamQuality, StreamingSettings},
};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

const PIN: &str = "2468";

/// An admin with a PIN, signed in, and a standard account with nothing.
async fn setup() -> (LibraryStore, AccountService, Account, Account) {
    let store = LibraryStore::in_memory().await.unwrap();
    let accounts = AccountService::open(store.clone(), Bus::default())
        .await
        .unwrap();
    let admin = accounts.signed_in().unwrap();
    accounts.set_pin(&admin.id, Some(PIN.into())).await.unwrap();
    let kid = accounts
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
    assert_eq!(kid.permissions, Permissions::default(), "starts with none");
    (store, accounts, admin, kid)
}

/// Signs the admin in, gives `kid` `permissions` and signs it in.
async fn grant(
    accounts: &AccountService,
    admin: &Account,
    kid: &Account,
    permissions: Permissions,
) {
    accounts.sign_in(&admin.id, Some(PIN.into())).await.unwrap();
    accounts
        .update(
            &kid.id,
            AccountUpdate {
                name: kid.name.clone(),
                avatar: kid.avatar.clone(),
                access: Some(AccountAccess {
                    role: AccountRole::Standard,
                    permissions,
                    max_age: None,
                    unrated_needs_pin: false,
                }),
            },
        )
        .await
        .unwrap();
    accounts.sign_in(&kid.id, None).await.unwrap();
}

/// A refusal's summary: coded `forbidden` (#160), telling the user to ask
/// an admin or sign in.
fn forbidden<T: std::fmt::Debug>(reply: Result<T, CommandError>) -> String {
    let reply = reply.expect_err("refused");
    assert_eq!(reply.code, ErrorCode::Forbidden, "{reply:?}");
    let lower = reply.summary.to_lowercase();
    assert!(
        lower.contains("admin") || lower.contains("sign in"),
        "{}",
        reply.summary
    );
    reply.summary
}

/// `run` behind the gate, as a shell calls `command` with `args`.
async fn gated<T>(
    guard: &Guard,
    command: &str,
    args: Value,
    run: impl Future<Output = Result<T, CommandError>>,
) -> Result<T, CommandError> {
    guard.check(command, &args)?;
    run.await
}

/// Stream quality is every account's, for its own device; hosting and the
/// rest of `core.streaming` stay an admin's (#66). The real command, behind
/// the gate.
#[tokio::test]
async fn a_standard_account_changes_stream_quality_and_is_refused_hosting() {
    let (store, accounts, _admin, kid) = setup().await;
    let temp = tempfile::tempdir().unwrap();
    let settings = SettingsService::new(store);
    settings
        .register_core(&caskade_core::AppPaths::under(temp.path()))
        .unwrap();
    let guard = Guard::new(accounts.clone());
    accounts.sign_in(&kid.id, None).await.unwrap();

    let quality = StreamQuality {
        resolution: Some("1280x720".into()),
        fps: Some(30),
        bitrate_kbps: Some(5000),
    };
    let args = json!({"quality": {"resolution": "1280x720", "fps": 30, "bitrateKbps": 5000}});
    let saved = gated(
        &guard,
        "streaming_set_quality",
        args.clone(),
        streaming_set_quality(&settings, quality.clone()),
    )
    .await
    .unwrap();
    assert_eq!(saved.value["quality"]["fps"], 30);
    let streaming: StreamingSettings = settings.read_or_default("core.streaming").await;
    assert_eq!(streaming.quality.bitrate_kbps, Some(5000));
    assert!(!streaming.hosting);

    // Refused before they'd run.
    forbidden(guard.check("streaming_set_hosting", &json!({"on": true})));
    forbidden(guard.check("streaming_use_existing", &json!({})));
    forbidden(
        gated(
            &guard,
            "settings_set",
            json!({"namespace": "core.streaming", "value": {"hosting": true}, "expectedRevision": null}),
            settings_set(&settings, "core.streaming", json!({"hosting": true}), None),
        )
        .await,
    );
    let streaming: StreamingSettings = settings.read_or_default("core.streaming").await;
    assert!(!streaming.hosting);

    accounts.sign_out();
    forbidden(
        gated(
            &guard,
            "streaming_set_quality",
            args,
            streaming_set_quality(&settings, quality),
        )
        .await,
    );
}

#[tokio::test]
async fn moving_a_locations_games_needs_both_storage_and_installs() {
    let (store, accounts, admin, kid) = setup().await;
    let temp = tempfile::tempdir().unwrap();
    let paths = caskade_core::AppPaths::under(temp.path());
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let launch = caskade_core::launch::LaunchService::new(store.clone());
    let storage = caskade_core::storage::StorageService::new(store.clone(), settings, launch)
        .unwrap()
        .with_paths(paths);
    let install = InstallService::new(store, Bus::default(), InstallOptions::default());
    let guard = Guard::new(accounts.clone());
    guard.set_install(install.clone());
    let call = || {
        gated(
            &guard,
            "storage_move_and_remove",
            json!({"id": "a", "targetId": "b"}),
            storage_move_and_remove(&accounts, &install, &storage, "a".into(), "b".into()),
        )
    };
    let granted = |settings: &[&str], install| Permissions {
        settings: settings
            .iter()
            .map(|section| (*section).to_owned())
            .collect(),
        install,
    };
    // The gate lets Storage through; the command asks for installs itself.
    grant(&accounts, &admin, &kid, granted(&["storage"], false)).await;
    forbidden(call().await);
    grant(&accounts, &admin, &kid, granted(&[], true)).await;
    forbidden(call().await);
    // Past both: the core answers that `a` is no location.
    grant(&accounts, &admin, &kid, granted(&["storage"], true)).await;
    assert_eq!(call().await.unwrap_err().code, ErrorCode::Invalid);
}

/// Deep links, drops and pastes from an account that may not install are
/// refused with the reason, and nothing is queued; once it may, they queue.
#[tokio::test(flavor = "multi_thread")]
async fn install_requests_need_the_install_permission() {
    let (_store, accounts, admin, kid) = setup().await;
    let queue = InstallRequests::default();
    let sink = crate::Sink::default();
    let events = sink.events();
    let guard = Guard::new(accounts.clone());
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("game.zip");
    std::fs::write(&file, b"not executed").unwrap();
    let path = file.to_string_lossy().into_owned();
    let paste = "https://example.invalid/game.zip".to_owned();
    let link = || Url::parse_with_params("caskade://install", [("path", &path)]).unwrap();
    // A paste and a drop as a shell calls them, behind the gate; their
    // refusals come back as the command's error.
    let pasted = || async {
        guard.check("request_install_input", &json!({"value": paste}))?;
        request_install_input(&events, &queue, Some(&accounts), None, paste.clone())
            .await
            .map_err(|reason| CommandError::new(ErrorCode::Invalid, reason))
    };
    let dropped = || async {
        guard.check("request_dropped_install", &json!({"path": path}))?;
        request_dropped_install(&events, &queue, path.clone())
            .await
            .map_err(|reason| CommandError::new(ErrorCode::Invalid, reason))
    };
    let opened = || {
        caskade_app::deep_link::queue(
            &events,
            Some(&queue),
            Some(&accounts),
            vec![caskade_app::deep_link::install_input(&link())],
        )
    };

    accounts.sign_in(&kid.id, None).await.unwrap();
    let error = forbidden(pasted().await);
    assert!(error.to_lowercase().contains("install"), "{error}");
    forbidden(dropped().await);
    // A link has no caller to answer: the UI is told why instead, and the
    // window comes up to show it.
    let (queued, raise) = opened().await;
    assert!(queued.is_empty());
    assert!(raise);
    assert!(queue.pending().is_empty(), "nothing is queued");
    assert!(pending_install_requests(&queue).is_empty());
    let said = sink.wait_for(Duration::from_secs(5), |event| match event {
        Event::InstallRefused(InstallRefused(reason)) => Some(reason.clone()),
        _ => None,
    });
    let said = said.expect("the UI heard why");
    assert!(said.contains("ask an admin"), "{said}");
    let refusals = sink
        .taken()
        .iter()
        .filter(|event| matches!(event, Event::InstallRefused(_)))
        .count();
    assert_eq!(refusals, 1);

    grant(
        &accounts,
        &admin,
        &kid,
        Permissions {
            settings: Vec::new(),
            install: true,
        },
    )
    .await;
    pasted().await.unwrap();
    dropped().await.unwrap();
    assert_eq!(opened().await.0.len(), 1);
    assert_eq!(queue.pending().len(), 3);
}
