//! The permission gate over typed IPC on the mock runtime (#205): every
//! registered command is classified, each group is allowed for an admin,
//! refused for a standard account without it and allowed once granted,
//! and install requests from deep links, drops and pastes from an account
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

#[test]
fn each_group_is_allowed_for_admins_refused_without_it_and_allowed_once_granted() {
    let (store, accounts, admin, kid) = setup();
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
    let call = |command: &str, body: Value| invoke(&window, command, body);
    let set =
        |namespace: &str| json!({"namespace": namespace, "value": {}, "expectedRevision": null});
    // Each group: its section's settings, and commands that change its state.
    let groups: Vec<(&str, Vec<(&str, Value)>)> = vec![
        (
            "general",
            vec![
                ("settings_set", set("core.general")),
                ("settings_reset", set("core.appUpdates")),
                ("settings_reset", set("core.updates")),
                ("app_update_install", json!({})),
            ],
        ),
        (
            "library-discovery",
            vec![
                ("settings_set", set("core.discovery")),
                ("discovery_scan", json!({})),
                ("discovery_review", json!({})),
            ],
        ),
        ("launchers", vec![("settings_set", set("core.launchers"))]),
        (
            "emulators",
            vec![
                ("settings_set", set("core.emulation")),
                ("emulator_save", json!({})),
                ("emulator_setup_install", json!({})),
                ("rom_scanner_save", json!({})),
                ("romm_server_save", json!({})),
            ],
        ),
        (
            "appearance",
            vec![
                ("settings_set", set("core.appearance")),
                ("theme_load_unpacked", json!({})),
            ],
        ),
        (
            "storage",
            vec![
                ("settings_set", set("storage.roots")),
                ("storage_add", json!({})),
                ("storage_remove", json!({})),
            ],
        ),
        ("installer", vec![("settings_set", set("core.installer"))]),
        (
            "plugins",
            vec![
                ("settings_set", set("plugins.example.pack")),
                ("plugin_install", json!({})),
                ("plugin_enable", json!({})),
            ],
        ),
    ];
    let installs: Vec<(&str, Value)> = vec![
        ("request_install_input", json!({})),
        ("request_dropped_install", json!({})),
        ("confirm_install_request", json!({})),
        ("install_start", json!({})),
        ("uninstall_game", json!({})),
        ("remove_game", json!({})),
        ("move_game", json!({})),
        ("rom_scan_start", json!({})),
        ("romm_sync", json!({})),
        ("job_retry", json!({"id": "missing"})),
    ];
    // Reading, always allowed.
    let reads = [
        ("settings_get", json!({"namespace": "core.general"})),
        ("settings_sections", json!({})),
        ("list_games", json!({})),
        ("account_sign_in", json!({})),
    ];
    // What every account shares: allowed whatever was granted.
    let shared = [
        ("launch_game", json!({})),
        ("game_update_install", json!({})),
        ("game_update_before_play", json!({})),
        ("set_game_favorite", json!({})),
    ];
    let ran = json!("ran");

    // The admin may do everything, unclassified commands included.
    for (command, body) in groups
        .iter()
        .flat_map(|(_, calls)| calls)
        .chain(&installs)
        .chain(&reads)
        .chain(&shared)
    {
        assert_eq!(call(command, body.clone()), Ok(ran.clone()), "{command}");
    }
    assert_eq!(call("not_classified", json!({})), Ok(ran.clone()));
    assert_eq!(call("settings_set", set("core.unknown")), Ok(ran.clone()));

    // A new standard account may only read and use what is shared.
    block(accounts.sign_in(&kid.id, None)).unwrap();
    for (command, body) in groups.iter().flat_map(|(_, calls)| calls).chain(&installs) {
        forbidden(call(command, body.clone()));
    }
    for (command, body) in reads.iter().chain(&shared) {
        assert_eq!(call(command, body.clone()), Ok(ran.clone()), "{command}");
    }
    let error = forbidden(call("settings_set", set("core.appearance")));
    assert!(error.contains("Appearance"), "{error}");
    forbidden(call("not_classified", json!({})));
    forbidden(call("settings_set", set("core.unknown")));

    // One group granted: that group's calls, and only those.
    for (section, _) in &groups {
        grant(
            &accounts,
            &admin,
            &kid,
            Permissions {
                settings: vec![(*section).to_owned()],
                install: false,
            },
        );
        for (other, other_calls) in &groups {
            for (command, body) in other_calls {
                let reply = call(command, body.clone());
                if other == section {
                    assert_eq!(reply, Ok(ran.clone()), "{section}: {command}");
                } else {
                    forbidden(reply);
                }
            }
        }
        for (command, body) in &installs {
            forbidden(call(command, body.clone()));
        }
    }
    grant(
        &accounts,
        &admin,
        &kid,
        Permissions {
            settings: Vec::new(),
            install: true,
        },
    );
    for (command, body) in &installs {
        assert_eq!(call(command, body.clone()), Ok(ran.clone()), "{command}");
    }
    forbidden(call("settings_set", set("core.general")));

    // Pairing, forgetting and what syncs (#231): an admin's alone, even for
    // an account given every settings section and installs.
    grant(
        &accounts,
        &admin,
        &kid,
        Permissions {
            settings: groups
                .iter()
                .map(|(section, _)| (*section).to_owned())
                .collect(),
            install: true,
        },
    );
    forbidden(call("settings_set", set("core.sync")));
    forbidden(call("settings_reset", set("core.sync")));
    for command in [
        "devices_pair_nearby",
        "devices_confirm_pairing",
        "devices_cancel_pairing",
        "devices_pairing_link",
        "devices_accept_pairing_link",
        "devices_accept_opened_link",
        "devices_forget",
        "devices_rename",
        "devices_set_kind",
        "devices_create_identity",
    ] {
        forbidden(call(command, json!({})));
    }
    // Seeing them and syncing now are everyone's.
    for command in ["devices_status", "devices_sync_now", "devices_opened_link"] {
        assert_eq!(call(command, json!({})), Ok(ran.clone()), "{command}");
    }

    // Signed out: only reads and signing in.
    accounts.sign_out();
    for (command, body) in groups
        .iter()
        .flat_map(|(_, calls)| calls)
        .chain(&installs)
        .chain(&shared)
    {
        let error = forbidden(call(command, body.clone()));
        assert!(
            error.to_lowercase().contains("sign in"),
            "{command}: {error}"
        );
    }
    for (command, body) in &reads {
        assert_eq!(call(command, body.clone()), Ok(ran.clone()), "{command}");
    }
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
