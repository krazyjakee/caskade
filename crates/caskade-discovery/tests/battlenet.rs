//! Battle.net (#87) on a Windows machine built at test time; product.db is
//! encoded with the reader's own protobuf messages.

use crate::fixtures::write;
use std::path::Path;

use caskade_discovery::{
    ExternalId, HostEnv, InstallState, LaunchTarget, ScanCtx, Source,
    parsers::protobuf::{
        BaseProductState, CachedProductState, ProductDb, ProductInstall, UserSettings,
    },
    sources::battlenet::BattleNet,
};
use prost::Message;

fn product(uid: &str, code: &str, path: &str) -> ProductInstall {
    ProductInstall {
        uid: uid.into(),
        product_code: code.into(),
        settings: Some(UserSettings {
            install_path: path.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn machine(root: &Path) {
    write(
        root,
        "machine.toml",
        b"schema_version = 1\nos = \"windows\"\n",
    );
    let db = ProductDb {
        products: vec![
            product("agent", "agent", "C:/ProgramData/Battle.net/Agent"),
            product("battle.net", "bna", "C:/Program Files (x86)/Battle.net"),
            product("wow", "wow", "C:/Program Files (x86)/World of Warcraft"),
            product("fxtr_enus", "fxtr", "D:/Games/Fixture Frontier"),
            product("gone", "gone", "D:/Games/Gone"),
            product("noinstall", "noinstall", ""),
            ProductInstall {
                cached_product_state: Some(CachedProductState {
                    base_product_state: Some(BaseProductState {
                        installed: false,
                        ..Default::default()
                    }),
                }),
                ..product("pending_enus", "pending", "D:/Games/Pending")
            },
        ],
    };
    let c = root.join("root/drives/c");
    let d = root.join("root/drives/d");
    write(
        &c,
        "ProgramData/Battle.net/Agent/product.db",
        db.encode_to_vec(),
    );
    write(
        &c,
        "Program Files (x86)/Battle.net/Battle.net.exe",
        b"inert",
    );
    write(
        &c,
        "Program Files (x86)/World of Warcraft/Wow.exe",
        b"inert",
    );
    write(&d, "Games/Fixture Frontier/frontier.exe", b"inert");
    write(
        root,
        "registry/hklm32.reg",
        b"Windows Registry Editor Version 5.00\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\World of Warcraft]\n\
\"DisplayName\"=\"World of Warcraft\"\n\
\"InstallLocation\"=\"C:\\\\Program Files (x86)\\\\World of Warcraft\"\n\
\"UninstallString\"=\"\\\"C:\\\\ProgramData\\\\Battle.net\\\\Agent\\\\Blizzard Uninstaller.exe\\\" --lang=enUS --uid=wow\"\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Fixture Frontier]\n\
\"DisplayName\"=\"Fixture Frontier\"\n\
\"InstallLocation\"=\"D:\\\\Games\\\\Fixture Frontier\\\\\"\n",
    );
}

#[test]
fn product_db_games_without_the_agent_or_battle_net() {
    let temp = tempfile::tempdir().unwrap();
    machine(temp.path());
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = BattleNet.scan(&env, &ctx).unwrap();
    assert_eq!(ctx.take_warnings(), Vec::<String>::new());
    let titles: Vec<_> = games
        .iter()
        .map(|game| (game.key.as_str(), game.title.as_str()))
        .collect();
    assert_eq!(
        titles,
        [
            ("fxtr", "Fixture Frontier"),
            ("gone", "Gone"),
            ("wow", "World of Warcraft")
        ]
    );
    let wow = &games[2];
    assert_eq!(wow.ids, [ExternalId::BattleNet { code: "wow".into() }]);
    assert_eq!(
        wow.launch,
        [LaunchTarget::Uri {
            uri: "battlenet://WoW".into()
        }]
    );
    assert_eq!(wow.state, InstallState::Installed);
    let uninstall = wow.uninstall.as_ref().unwrap();
    let LaunchTarget::Exec { program, args, .. } = &uninstall.target else {
        panic!("{uninstall:?}");
    };
    assert!(program.ends_with("Battle.net/Agent/Blizzard Uninstaller.exe"));
    assert_eq!(args, &["--lang=enUS", "--uid=wow"]);
    // Unknown product code: the uninstall key titles it; launch by code.
    assert_eq!(
        games[0].launch,
        [LaunchTarget::Uri {
            uri: "battlenet://fxtr".into()
        }]
    );
    assert!(games[0].uninstall.is_none());
    assert!(matches!(games[1].state, InstallState::Broken { .. }));

    let launcher = &BattleNet.detect(&env)[0];
    assert!(launcher.found);
    assert!(
        launcher
            .path
            .as_ref()
            .unwrap()
            .ends_with("Battle.net/Battle.net.exe")
    );
    assert!(
        BattleNet.watch_paths(&env)[0]
            .path
            .ends_with("Agent/product.db")
    );
}

#[test]
fn corrupt_product_db_is_a_source_error() {
    let temp = tempfile::tempdir().unwrap();
    machine(temp.path());
    write(
        &temp.path().join("root/drives/c"),
        "ProgramData/Battle.net/Agent/product.db",
        [0x0a, 0xff, 0xff, 0xff, 0xff, 0x0f],
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    assert!(BattleNet.scan(&env, &ScanCtx::default()).is_err());
}
