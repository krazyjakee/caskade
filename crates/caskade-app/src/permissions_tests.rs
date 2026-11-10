//! The permission rules (#205): every group is allowed for an admin,
//! refused for a standard account without it and allowed once granted;
//! signed out, only reads and signing in.
use super::*;
use caskade_core::{
    accounts::{
        Account, AccountAccess, AccountRole, AccountUpdate, DEFAULT_AVATAR, NewAccount, Permissions,
    },
    bus::Bus,
    db::LibraryStore,
    install::InstallOptions,
};
use serde_json::json;

const PIN: &str = "2468";

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    crate::runtime::block_on(future)
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

/// A refusal's reason, which tells the user to ask an admin or sign in.
fn forbidden<T: std::fmt::Debug>(reply: Result<T, String>) -> String {
    let error = reply.expect_err("refused");
    let lower = error.to_lowercase();
    assert!(
        lower.contains("admin") || lower.contains("sign in"),
        "{error}"
    );
    error
}

#[test]
fn each_scope_needs_its_own_permission() {
    let body = |scope: Value| json!({"scope": scope});
    assert_eq!(
        script_scope(&body(json!({"type": "game", "gameId": "g"}))),
        Permission::Install
    );
    assert_eq!(
        script_scope(&body(json!({"type": "profile", "profileId": "p"}))),
        Permission::Settings("emulators")
    );
    assert_eq!(
        script_scope(&body(json!({"type": "global"}))),
        Permission::Admin
    );
    assert_eq!(script_scope(&json!({})), Permission::Admin);
}

#[test]
fn without_accounts_only_reads_are_allowed() {
    assert_eq!(check("list_games", &json!({}), None, None), Ok(()));
    assert!(check("launch_game", &json!({}), None, None).is_err());
}

#[test]
fn each_group_is_allowed_for_admins_refused_without_it_and_allowed_once_granted() {
    let (store, accounts, admin, kid) = setup();
    let install = InstallService::new(store, Bus::default(), InstallOptions::default());
    let call = |command: &str, body: Value| {
        check(command, &body, Some(&accounts), Some(&install)).map(|()| "ran")
    };
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
                ("storage_move_and_remove", json!({})),
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
        ("streaming_set_quality", json!({})),
    ];
    let ran = "ran";

    // The admin may do everything, unclassified commands included.
    for (command, body) in groups
        .iter()
        .flat_map(|(_, calls)| calls)
        .chain(&installs)
        .chain(&reads)
        .chain(&shared)
    {
        assert_eq!(call(command, body.clone()), Ok(ran), "{command}");
    }
    assert_eq!(call("not_classified", json!({})), Ok(ran));
    assert_eq!(call("settings_set", set("core.unknown")), Ok(ran));

    // A new standard account may only read and use what is shared.
    block(accounts.sign_in(&kid.id, None)).unwrap();
    for (command, body) in groups.iter().flat_map(|(_, calls)| calls).chain(&installs) {
        forbidden(call(command, body.clone()));
    }
    for (command, body) in reads.iter().chain(&shared) {
        assert_eq!(call(command, body.clone()), Ok(ran), "{command}");
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
                    assert_eq!(reply, Ok(ran), "{section}: {command}");
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
        assert_eq!(call(command, body.clone()), Ok(ran), "{command}");
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
        assert_eq!(call(command, json!({})), Ok(ran), "{command}");
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
        assert_eq!(call(command, body.clone()), Ok(ran), "{command}");
    }
}
