use std::{collections::BTreeMap, fmt::Debug, path::PathBuf};

use caskade_discovery::types::*;
use proptest::prelude::*;
use schemars::{JsonSchema, schema_for};
use serde::{Serialize, de::DeserializeOwned};

fn roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let encoded = serde_json::to_string(value).expect("serialize");
    let decoded: T = serde_json::from_str(&encoded).expect("deserialize");
    assert_eq!(&decoded, value, "wire value: {encoded}");
}

/// Compare tested tags with the derived schema so new variants cannot silently
/// miss round-trip coverage.
fn tagged_variants<T>(cases: Vec<T>)
where
    T: Serialize + DeserializeOwned + JsonSchema + PartialEq + Debug,
{
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    let mut expected: Vec<String> = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            variant["properties"]["type"]["const"]
                .as_str()
                .unwrap()
                .into()
        })
        .collect();
    let mut seen: Vec<String> = cases
        .iter()
        .map(|case| {
            roundtrip(case);
            serde_json::to_value(case).unwrap()["type"]
                .as_str()
                .unwrap()
                .into()
        })
        .collect();
    expected.sort();
    seen.sort();
    assert_eq!(seen, expected);
}

fn external_ids() -> Vec<ExternalId> {
    vec![
        ExternalId::Steam { app_id: 620 },
        ExternalId::SteamShortcut {
            app_id: 3_000_000_001,
        },
        ExternalId::Gog {
            product_id: 1_207_658_924,
        },
        ExternalId::Epic {
            namespace: "ns".into(),
            catalog_item: "item:1".into(),
            app_name: "App%".into(),
        },
        ExternalId::Itch { game_id: 42 },
        ExternalId::Amazon {
            id: "amzn1.adg.product.x".into(),
        },
        ExternalId::Ubisoft { id: 635 },
        ExternalId::BattleNet { code: "wow".into() },
        ExternalId::Ea {
            offer: "Origin.OFR.50.0002694".into(),
        },
        ExternalId::Xbox {
            pfn: "Microsoft.Game_8wekyb3d8bbwe".into(),
        },
        ExternalId::MicrosoftStore {
            store_id: "9NBLGGH4R315".into(),
        },
        ExternalId::Riot {
            product: "valorant".into(),
        },
        ExternalId::Rockstar {
            title: "gta5".into(),
        },
        ExternalId::Lutris {
            slug: "celeste".into(),
        },
    ]
}

fn exec() -> LaunchTarget {
    LaunchTarget::Exec {
        program: PathBuf::from("/games/celeste/Celeste"),
        args: vec!["--fullscreen".into()],
        cwd: Some(PathBuf::from("/games/celeste")),
        env: BTreeMap::from([("SDL_VIDEODRIVER".into(), "wayland".into())]),
    }
}

fn sample_game() -> DetectedGame {
    let mut game = DetectedGame::new(SourceId::new("steam"), "620", "Portal 2");
    game.ids = vec![ExternalId::Steam { app_id: 620 }];
    game.install_dir = Some(PathBuf::from("/steam/steamapps/common/Portal 2"));
    game.launch = vec![LaunchTarget::Uri {
        uri: "steam://rungameid/620".into(),
    }];
    game.runtime = GameRuntime::Windows {
        prefix: Some(Prefix {
            path: PathBuf::from("/steam/steamapps/compatdata/620/pfx"),
            arch: Some(PrefixArch::Win64),
            owner: Some(SourceId::new("steam")),
        }),
        runner: Some(RunnerHint {
            kind: RunnerKind::Proton,
            name: Some("proton_experimental".into()),
            path: None,
        }),
    };
    game.size = Some(12_000_000_000);
    game.playtime_secs = Some(3_600);
    game.last_played_unix = Some(1_761_523_200);
    game.uninstall = Some(UninstallInfo {
        target: LaunchTarget::Uri {
            uri: "steam://uninstall/620".into(),
        },
        quiet: None,
        registry_key: None,
    });
    game.evidence = vec![Evidence::File {
        path: PathBuf::from("/steam/steamapps/appmanifest_620.acf"),
    }];
    game.tracking = Some(TrackingHint {
        steam_app_id: Some(620),
        install_dir: game.install_dir.clone(),
        exe_names: vec!["portal2.exe".into()],
    });
    game
}

#[test]
fn scan_report_schema() {
    insta::assert_json_snapshot!(schema_for!(ScanReport));
}

#[test]
fn scan_report_roundtrips() {
    let report = ScanReport {
        groups: vec![GameGroup {
            primary: 0,
            entries: vec![sample_game()],
        }],
        launchers: vec![
            LauncherStatus {
                source: SourceId::new("steam"),
                found: true,
                kind: Some(LauncherKind::Flatpak {
                    app_id: "com.valvesoftware.Steam".into(),
                }),
                path: Some(PathBuf::from(
                    "/var/lib/flatpak/app/com.valvesoftware.Steam",
                )),
                data_dir: Some(PathBuf::from("/home/u/.var/app/com.valvesoftware.Steam")),
                version: Some("1.0.0.82".into()),
            },
            LauncherStatus::not_found(SourceId::new("heroic")),
        ],
        sources: vec![SourceReport {
            source: SourceId::new("steam"),
            duration_ms: 12,
            count: 1,
            warnings: vec!["unknown StateFlags 1024".into()],
            error: None,
        }],
    };
    roundtrip(&report);
    assert_eq!(report.groups[0].primary_entry().unwrap().title, "Portal 2");
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["groups"][0]["entries"][0]["source"], "steam");
    assert_eq!(
        json["groups"][0]["entries"][0]["ids"][0],
        serde_json::json!({ "type": "steam", "value": { "app_id": 620 } })
    );
}

#[test]
fn every_tagged_variant_roundtrips() {
    tagged_variants(external_ids());
    tagged_variants(vec![
        LaunchTarget::Uri {
            uri: "itch://caves/1/launch".into(),
        },
        exec(),
        LaunchTarget::Flatpak {
            app_id: "com.example.Game".into(),
            args: vec![],
        },
    ]);
    tagged_variants(vec![sample_game().runtime, GameRuntime::Native]);
    tagged_variants(vec![
        InstallState::Installed,
        InstallState::Updating,
        InstallState::Partial,
        InstallState::Broken {
            reason: "missing exe".into(),
        },
    ]);
    tagged_variants(vec![
        Evidence::File {
            path: PathBuf::from("a.acf"),
        },
        Evidence::Registry {
            key: r"HKLM\SOFTWARE\x".into(),
            value: Some("InstallLocation".into()),
        },
        Evidence::Rule {
            rule: "engine.unity".into(),
            score: 30,
        },
    ]);
    tagged_variants(vec![
        LauncherKind::Native,
        LauncherKind::Flatpak { app_id: "a".into() },
        LauncherKind::Snap {
            name: "steam".into(),
        },
        LauncherKind::Wine {
            prefix: PathBuf::from("/p"),
        },
    ]);
    tagged_variants(vec![
        SourceError::io("/x", "denied"),
        SourceError::parse("/y", "bad vdf"),
        SourceError::Registry {
            key: "k".into(),
            message: "m".into(),
        },
        SourceError::Cancelled,
        SourceError::TimedOut,
        SourceError::Panicked {
            message: "boom".into(),
        },
        SourceError::Other {
            message: "other".into(),
        },
    ]);
}

#[test]
fn external_id_string_forms() {
    let forms = [
        "steam:620",
        "steam_shortcut:3000000001",
        "gog:1207658924",
        "epic:ns:item%3A1:App%25",
        "itch:42",
        "amazon:amzn1.adg.product.x",
        "ubisoft:635",
        "battle_net:wow",
        "ea:Origin.OFR.50.0002694",
        "xbox:Microsoft.Game_8wekyb3d8bbwe",
        "microsoft_store:9NBLGGH4R315",
        "riot:valorant",
        "rockstar:gta5",
        "lutris:celeste",
    ];
    for (id, form) in external_ids().iter().zip(forms) {
        assert_eq!(id.to_string(), form);
        assert_eq!(form.parse::<ExternalId>().unwrap(), *id);
    }
    for bad in [
        "",
        "steam",
        "steam:",
        "steam:0620",
        "steam:+620",
        "steam:4294967296",
        "steam:1:2",
        "epic:a:b",
        "lutris:a%3a",
        "lutris:a%",
        "unknown:1",
    ] {
        assert!(bad.parse::<ExternalId>().is_err(), "{bad:?} parsed");
    }
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![any::<String>(), "[a-z:%0-9]{0,8}"]
}

fn any_external_id() -> impl Strategy<Value = ExternalId> {
    prop_oneof![
        any::<u32>().prop_map(|app_id| ExternalId::Steam { app_id }),
        any::<u32>().prop_map(|app_id| ExternalId::SteamShortcut { app_id }),
        any::<u64>().prop_map(|product_id| ExternalId::Gog { product_id }),
        (text(), text(), text()).prop_map(|(namespace, catalog_item, app_name)| {
            ExternalId::Epic {
                namespace,
                catalog_item,
                app_name,
            }
        }),
        any::<u64>().prop_map(|game_id| ExternalId::Itch { game_id }),
        text().prop_map(|id| ExternalId::Amazon { id }),
        any::<u64>().prop_map(|id| ExternalId::Ubisoft { id }),
        text().prop_map(|code| ExternalId::BattleNet { code }),
        text().prop_map(|offer| ExternalId::Ea { offer }),
        text().prop_map(|pfn| ExternalId::Xbox { pfn }),
        text().prop_map(|store_id| ExternalId::MicrosoftStore { store_id }),
        text().prop_map(|product| ExternalId::Riot { product }),
        text().prop_map(|title| ExternalId::Rockstar { title }),
        text().prop_map(|slug| ExternalId::Lutris { slug }),
    ]
}

proptest! {
    #[test]
    fn external_id_string_roundtrip(id in any_external_id()) {
        let form = id.to_string();
        prop_assert_eq!(form.parse::<ExternalId>().unwrap(), id);
        // One spelling per id: re-rendering the parsed form is identical.
        prop_assert_eq!(form.parse::<ExternalId>().unwrap().to_string(), form);
    }

    #[test]
    fn external_id_parse_never_panics(input in any::<String>()) {
        if let Ok(id) = input.parse::<ExternalId>() {
            prop_assert_eq!(id.to_string(), input);
        }
    }
}

#[test]
fn os_sets() {
    use caskade_discovery::OperatingSystem::*;
    let unix = OsSet::LINUX.union(OsSet::MACOS);
    assert!(unix.contains(Linux) && unix.contains(Macos) && !unix.contains(Windows));
    assert_eq!(OsSet::NONE.with(Windows), OsSet::WINDOWS);
    assert_eq!(unix.union(OsSet::WINDOWS), OsSet::ALL);
}

#[test]
fn scan_ctx_cancels_and_collects_warnings() {
    let cancel = Cancel::new();
    let ctx = ScanCtx::new(cancel.clone());
    assert!(ctx.check().is_ok());
    ctx.warn("newer manifest version 3");
    cancel.cancel();
    assert_eq!(ctx.check(), Err(SourceError::Cancelled));
    assert_eq!(ctx.take_warnings(), ["newer manifest version 3"]);
    assert!(ctx.take_warnings().is_empty());
}

#[test]
fn builtin_sources_have_unique_ids_and_hosts() {
    let mut ids = std::collections::BTreeSet::new();
    for source in builtin_sources() {
        assert!(ids.insert(source.id()), "duplicate {}", source.id());
        assert_ne!(source.hosts(), OsSet::NONE, "{}", source.id());
        assert!(find_source(source.id().as_str()).is_some());
    }
    assert!(find_source("no.such.source").is_none());
}
