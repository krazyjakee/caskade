//! The permission gate over typed IPC on the mock runtime (#205): every
//! registered command is classified, refusals cross as structured errors,
//! real commands are checked behind it, and install requests from deep links, drops and pastes from an account
//! that may not install are refused with nothing queued.
use super::*;
use crate::commands::{
    install_requests::{InstallRefused, InstallRequested, InstallRequests},
    test_ipc::invoke,
};
use caskade_core::{
    accounts::{
        Account, AccountAccess, AccountRole, AccountUpdate, DEFAULT_AVATAR, NewAccount, Permissions,
    },
    bus::Bus,
    db::LibraryStore,
    install::InstallOptions,
    settings::SettingsService,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Listener,
    test::{MockRuntime, mock_builder, mock_context, noop_assets},
};
use tauri_specta::Event;

const PIN: &str = "2468";

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(future)
}

/// An admin with a PIN, signed in, and a standard account with nothing.
fn setup() -> (LibraryStore, AccountService, Account, Account) {
    let store = block(LibraryStore::in_memory()).unwrap();
    let accounts = block(AccountService::open(store.clone(), Bus::default())).unwrap();
    let admin = accounts.signed_in().unwrap();
    block(accounts.set_pin(&admin.id, Some(PIN.into()))).unwrap();
    let kid = block(accounts.create(NewAccount {
        name: "Kid".into(),
        avatar: DEFAULT_AVATAR.into(),
        access: AccountAccess {
            role: AccountRole::Standard,
            permissions: Permissions::default(),
            max_age: None,
            unrated_needs_pin: false,
        },
        pin: None,
    }))
    .unwrap();
    assert_eq!(kid.permissions, Permissions::default(), "starts with none");
    (store, accounts, admin, kid)
}

/// Signs the admin in, gives `kid` `permissions` and signs it in.
fn grant(accounts: &AccountService, admin: &Account, kid: &Account, permissions: Permissions) {
    block(accounts.sign_in(&admin.id, Some(PIN.into()))).unwrap();
    block(accounts.update(
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
    ))
    .unwrap();
    block(accounts.sign_in(&kid.id, None)).unwrap();
}

/// A refusal's summary; refusals cross as `ErrorReport`s coded
/// `forbidden` (#160).
fn forbidden(reply: Result<Value, Value>) -> String {
    let reply = reply.expect_err("refused");
    assert_eq!(reply["code"], "forbidden", "{reply}");
    let error = reply["summary"].as_str().unwrap().to_owned();
    let lower = error.to_lowercase();
    assert!(
        lower.contains("admin") || lower.contains("sign in"),
        "{error}"
    );
    error
}

#[test]
fn every_registered_command_has_a_permission_group() {
    let bindings = crate::bindings::render().unwrap();
    let commands: Vec<&str> = bindings
        .split("__TAURI_INVOKE(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert!(commands.len() > 100, "{}", commands.len());
    let missing: Vec<_> = commands
        .iter()
        .filter(|command| access(command).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "classify these commands in `permissions::access`: {missing:?}"
    );
}

/// Stream quality is every account's, for its own device; hosting and the
/// rest of `core.streaming` stay an admin's (#66). The real commands, behind
/// the gate.
#[test]
fn a_standard_account_changes_stream_quality_and_is_refused_hosting() {
    let (store, accounts, _admin, kid) = setup();
    let temp = tempfile::tempdir().unwrap();
    let settings = SettingsService::new(store);
    settings
        .register_core(&caskade_core::AppPaths::under(temp.path()))
        .unwrap();
    let app = mock_builder()
        .manage(accounts.clone())
        .manage(settings.clone())
        .invoke_handler(gate(tauri::generate_handler![
            crate::commands::settings::settings_set,
            crate::commands::streaming::streaming_set_quality,
        ]))
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "quality", Default::default())
        .build()
        .unwrap();
    block(accounts.sign_in(&kid.id, None)).unwrap();

    let quality = json!({"quality": {"resolution": "1280x720", "fps": 30, "bitrateKbps": 5000}});
    let saved = invoke(&window, "streaming_set_quality", quality.clone()).unwrap();
    assert_eq!(saved["value"]["quality"]["fps"], 30);
    let streaming: caskade_core::settings::StreamingSettings =
        block(settings.read_or_default("core.streaming"));
    assert_eq!(streaming.quality.bitrate_kbps, Some(5000));
    assert!(!streaming.hosting);

    forbidden(invoke(
        &window,
        "streaming_set_hosting",
        json!({"on": true}),
    ));
    forbidden(invoke(&window, "streaming_use_existing", json!({})));
    forbidden(invoke(
        &window,
        "settings_set",
        json!({"namespace": "core.streaming", "value": {"hosting": true}, "expectedRevision": null}),
    ));

    accounts.sign_out();
    forbidden(invoke(&window, "streaming_set_quality", quality));
}

#[test]
fn moving_a_locations_games_needs_both_storage_and_installs() {
    let (store, accounts, admin, kid) = setup();
    let temp = tempfile::tempdir().unwrap();
    let paths = caskade_core::AppPaths::under(temp.path());
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let launch = caskade_core::launch::LaunchService::new(store.clone());
    let storage = caskade_core::storage::StorageService::new(store.clone(), settings, launch)
        .unwrap()
        .with_paths(paths);
    let install = InstallService::new(store, Bus::default(), InstallOptions::default());
    let app = mock_builder()
        .manage(accounts.clone())
        .manage(storage)
        .manage(install)
        .invoke_handler(gate(tauri::generate_handler![
            crate::commands::storage::storage_move_and_remove,
        ]))
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "location", Default::default())
        .build()
        .unwrap();
    let call = || {
        invoke(
            &window,
            "storage_move_and_remove",
            json!({"id": "a", "targetId": "b"}),
        )
    };
    let granted = |settings: &[&str], install| Permissions {
        settings: settings
            .iter()
            .map(|section| (*section).to_owned())
            .collect(),
        install,
    };
    grant(&accounts, &admin, &kid, granted(&["storage"], false));
    forbidden(call());
    grant(&accounts, &admin, &kid, granted(&[], true));
    forbidden(call());
    // Past both: the core answers that `a` is no location.
    grant(&accounts, &admin, &kid, granted(&["storage"], true));
    assert_eq!(call().unwrap_err()["code"], "invalid");
}

/// The gate answers a refusal as a structured `forbidden` error and lets
/// an allowed call through (the rules are `caskade_app::permissions`').
#[test]
fn the_gate_refuses_with_a_structured_error() {
    let (store, accounts, _admin, kid) = setup();
    let install = InstallService::new(store, Bus::default(), InstallOptions::default());
    let app = mock_builder()
        .manage(accounts.clone())
        .manage(install)
        // The gate alone: a command it lets through answers "ran".
        .invoke_handler(gate(|invoke: Invoke<MockRuntime>| {
            invoke.resolver.resolve("ran");
            true
        }))
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "permissions", Default::default())
        .build()
        .unwrap();
    let set = json!({"namespace": "core.appearance", "value": {}, "expectedRevision": null});
    assert_eq!(
        invoke(&window, "settings_set", set.clone()),
        Ok(json!("ran"))
    );
    block(accounts.sign_in(&kid.id, None)).unwrap();
    let error = forbidden(invoke(&window, "settings_set", set));
    assert!(error.contains("Appearance"), "{error}");
    assert_eq!(invoke(&window, "list_games", json!({})), Ok(json!("ran")));
}

/// Deep links, drops and pastes from an account that may not install are
/// refused with the reason, and nothing is queued; once it may, they queue.
#[test]
fn install_requests_need_the_install_permission() {
    let (_store, accounts, admin, kid) = setup();
    let queue = InstallRequests::default();
    let specta = tauri_specta::Builder::<MockRuntime>::new()
        .commands(tauri_specta::collect_commands![
            crate::commands::install_requests::request_install_input::<MockRuntime>,
            crate::commands::install_requests::request_dropped_install::<MockRuntime>,
            crate::commands::install_requests::pending_install_requests,
        ])
        .events(tauri_specta::collect_events![
            InstallRequested,
            InstallRefused
        ]);
    let app = mock_builder()
        .manage(accounts.clone())
        .manage(queue.clone())
        .invoke_handler(gate(specta.invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let window = tauri::WebviewWindowBuilder::new(&app, "permissions", Default::default())
        .build()
        .unwrap();
    let refused = Arc::new(Mutex::new(Vec::<Value>::new()));
    {
        let refused = refused.clone();
        app.listen(InstallRefused::NAME, move |event| {
            refused
                .lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
    }
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("game.zip");
    std::fs::write(&file, b"not executed").unwrap();
    let path = file.to_string_lossy().into_owned();
    let paste = json!({"value": "https://example.invalid/game.zip"});
    let dropped = json!({"path": path});
    let link = || tauri::Url::parse_with_params("caskade://install", [("path", &path)]).unwrap();

    block(accounts.sign_in(&kid.id, None)).unwrap();
    let error = forbidden(invoke(&window, "request_install_input", paste.clone()));
    assert!(error.to_lowercase().contains("install"), "{error}");
    forbidden(invoke(&window, "request_dropped_install", dropped.clone()));
    assert!(block(crate::deep_link::open(app.handle(), [link()])).is_empty());
    assert!(queue.pending().is_empty(), "nothing is queued");
    assert_eq!(
        invoke(&window, "pending_install_requests", json!({})).unwrap(),
        json!([])
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while refused.lock().unwrap().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let said = refused.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].as_str().unwrap().contains("ask an admin"),
        "{said:?}"
    );

    grant(
        &accounts,
        &admin,
        &kid,
        Permissions {
            settings: Vec::new(),
            install: true,
        },
    );
    invoke(&window, "request_install_input", paste).unwrap();
    invoke(&window, "request_dropped_install", dropped).unwrap();
    assert_eq!(
        block(crate::deep_link::open(app.handle(), [link()])).len(),
        1
    );
    assert_eq!(queue.pending().len(), 3);
}
