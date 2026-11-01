use caskade_discovery::{
    ExternalId, LaunchTarget,
    uri::{self, UriIdentity},
};
use std::collections::BTreeMap;
fn has(uri: &str, id: ExternalId) {
    assert!(
        uri::identities(uri).contains(&UriIdentity::External(id)),
        "{uri}"
    );
}
#[test]
fn every_launcher_scheme_and_current_heroic_form() {
    let cases = [
        ("steam://rungameid/620", ExternalId::Steam { app_id: 620 }),
        ("steam://run/620//", ExternalId::Steam { app_id: 620 }),
        ("steam://uninstall/620", ExternalId::Steam { app_id: 620 }),
        (
            "origin://LaunchGame/Origin.OFR.50.0001380",
            ExternalId::Ea {
                offer: "Origin.OFR.50.0001380".into(),
            },
        ),
        (
            "com.epicgames.launcher://apps/ns%3Aitem%3Aapp?action=launch&silent=true",
            ExternalId::Epic {
                namespace: "ns".into(),
                catalog_item: "item".into(),
                app_name: "app".into(),
            },
        ),
        (
            "goggalaxy://openGameView/1207658924",
            ExternalId::Gog {
                product_id: 1207658924,
            },
        ),
        (
            "goggalaxy://launchGame/1207658924",
            ExternalId::Gog {
                product_id: 1207658924,
            },
        ),
        ("uplay://launch/44/0", ExternalId::Ubisoft { id: 44 }),
        (
            "battlenet://WoW",
            ExternalId::BattleNet { code: "WOW".into() },
        ),
        (
            "battlenet://launch/WoW",
            ExternalId::BattleNet { code: "WOW".into() },
        ),
        (
            "amazon-games://play/a-guid",
            ExternalId::Amazon {
                id: "a-guid".into(),
            },
        ),
        (
            "link2ea://launchgame/OFB-EAST%3A109552154",
            ExternalId::Ea {
                offer: "OFB-EAST:109552154".into(),
            },
        ),
        (
            "origin2://game/launch?offerIds=OFB-EAST%3A109552154",
            ExternalId::Ea {
                offer: "OFB-EAST:109552154".into(),
            },
        ),
        (
            "heroic://launch/gog/1207658924",
            ExternalId::Gog {
                product_id: 1207658924,
            },
        ),
        (
            "heroic://launch?appName=1207658924&runner=gog",
            ExternalId::Gog {
                product_id: 1207658924,
            },
        ),
        (
            "heroic://launch?runner=nile&appName=a-guid",
            ExternalId::Amazon {
                id: "a-guid".into(),
            },
        ),
        (
            "lutris:rungame/my-game",
            ExternalId::Lutris {
                slug: "my-game".into(),
            },
        ),
        (
            r"shell:AppsFolder\Publisher.Game_123!App",
            ExternalId::Xbox {
                pfn: "publisher.game_123".into(),
            },
        ),
    ];
    for (uri, id) in cases {
        has(uri, id);
    }
}
#[test]
fn local_launcher_rows_are_not_store_ids() {
    for uri in [
        "itch://caves/42/launch",
        "lutris:rungameid/42",
        "heroic://launch/sideload/42",
    ] {
        let ids = uri::identities(uri);
        assert_eq!(ids.len(), 1);
        assert!(matches!(ids[0], UriIdentity::Launcher { .. }));
    }
    assert_eq!(
        uri::identities("itchio://caves/42/launch"),
        uri::identities("itch://caves/42/launch")
    );
    assert_ne!(
        uri::identities("itch://caves/42/launch"),
        uri::identities("lutris:rungameid/42")
    );
    let id = 0x80000123u64;
    has(
        &format!("steam://rungameid/{}", (id << 32) | 0x02000000),
        ExternalId::SteamShortcut { app_id: id as u32 },
    );
}
#[test]
fn epic_partial_links_and_wrapped_targets() {
    let a = uri::identities("heroic://launch?appName=AppName&runner=legendary");
    let b = uri::external_identities(&ExternalId::Epic {
        namespace: "ns".into(),
        catalog_item: "item".into(),
        app_name: "AppName".into(),
    });
    assert!(a.iter().any(|id| b.contains(id)));
    let wrapped = LaunchTarget::Exec {
        program: "/usr/bin/flatpak".into(),
        args: vec![
            "run".into(),
            "com.heroicgameslauncher.hgl".into(),
            "--no-gui".into(),
            "heroic://launch?appName=120&runner=gog".into(),
        ],
        cwd: None,
        env: BTreeMap::new(),
    };
    assert!(
        uri::target_identities(&wrapped)
            .contains(&UriIdentity::External(ExternalId::Gog { product_id: 120 }))
    );
}
#[test]
fn malformed_ambiguous_and_non_launch_values_never_add_identity() {
    for value in [
        "steam://install/620",
        "https://store.steampowered.com/app/620",
        "steam://rungameid/-1",
        "steam://rungameid/18446744073709551616",
        "heroic://launch?appName=1&runner=gog&runner=legendary",
        "heroic://launch?appName=%FF&runner=gog",
        "heroic://launch?appName=%00&runner=gog",
        "heroic://launch?appName=%xx&runner=gog",
        "origin2://game/launch?offerIds=a,b",
        "steam://user@rungameid/620",
        "steam://rungameid:42/620",
        r"shell:AppsFolder\!App",
        "/usr/bin/steam steam://rungameid/620",
    ] {
        assert!(uri::identities(value).is_empty(), "{value}");
    }
    assert!(uri::identities(&"x".repeat(16_385)).is_empty());
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(512))]
    #[test]
    fn arbitrary_unicode_is_bounded_and_never_panics(value in ".{0,512}") { let _ = uri::identities(&value); }
}

#[test]
fn galaxy_command_line_carries_product_identity_without_a_shared_client_path() {
    use caskade_discovery::uri::{UriIdentity, target_identities};
    let launch = |args: Vec<String>| caskade_discovery::LaunchTarget::Exec {
        program: "/Applications/GOG Galaxy.app/Contents/MacOS/Client".into(),
        args,
        cwd: None,
        env: std::collections::BTreeMap::new(),
    };
    let ids = target_identities(&launch(vec![
        "/command=runGame".into(),
        "/gameId=42".into(),
        "/path=/Games/Fixture".into(),
    ]));
    assert!(
        ids.contains(&UriIdentity::External(caskade_discovery::ExternalId::Gog {
            product_id: 42
        }))
    );
    for args in [
        vec!["/gameId=42".into()],
        vec!["/command=install".into(), "/gameId=42".into()],
        vec!["/command=runGame".into(), "/gameId=0".into()],
        vec![
            "/command=runGame".into(),
            "/gameId=1".into(),
            "/gameId=2".into(),
        ],
    ] {
        assert!(target_identities(&launch(args)).is_empty());
    }
}
