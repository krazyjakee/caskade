use caskade_discovery::{
    Cancel, Confidence, Config, DetectedGame, Evidence, ExternalId, HostEnv, LaunchTarget,
    LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, UninstallInfo,
    merge::{MergeConfig, merge},
    orchestrator::{Exclusions, SourceKey},
    scan_with_sources,
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
fn fixture() -> (tempfile::TempDir, HostEnv) {
    let t = crate::fixtures::tempdir();
    fs::create_dir(t.path().join("root")).unwrap();
    let env = HostEnv::fixture(t.path()).unwrap();
    (t, env)
}
fn game(
    source: &'static str,
    key: &str,
    id: Option<ExternalId>,
    dir: Option<&str>,
) -> DetectedGame {
    let mut g = DetectedGame::new(SourceId::new(source), key, "Portal 2");
    g.ids = id.into_iter().collect();
    g.install_dir = dir.map(Into::into);
    g
}
fn uri(value: &str) -> LaunchTarget {
    LaunchTarget::Uri { uri: value.into() }
}
fn exe(program: &str, args: &[&str]) -> LaunchTarget {
    LaunchTarget::Exec {
        program: program.into(),
        args: args.iter().map(|v| (*v).into()).collect(),
        cwd: None,
        env: BTreeMap::new(),
    }
}
fn groups(env: &HostEnv, entries: Vec<DetectedGame>) -> Vec<caskade_discovery::GameGroup> {
    merge(env, entries, &MergeConfig::default()).unwrap()
}
#[test]
fn section_8_1_steam_manifest_uninstall_desktop_shortcut_and_contained_exe() {
    let (_t, env) = fixture();
    let steam = game(
        "steam",
        "620",
        Some(ExternalId::Steam { app_id: 620 }),
        Some("/games/Portal2"),
    );
    let mut registry = game(
        "windows.uninstall",
        "Steam App 620",
        Some(ExternalId::Steam { app_id: 620 }),
        None,
    );
    registry.icon = Some("/icons/portal.ico".into());
    registry.size = Some(12345);
    let mut desktop = game("linux.desktop", "portal.desktop", None, None);
    desktop.launch.push(uri("steam://rungameid/620"));
    let mut shortcut = game("windows.shortcuts", "portal.lnk", None, None);
    shortcut
        .launch
        .push(exe("/usr/bin/steam", &["steam://rungameid/620"]));
    let mut deep = game("deep.folders", "/games/Portal2/bin/portal", None, None);
    deep.launch.push(exe("/games/Portal2/bin/portal", &[]));
    let result = groups(&env, vec![desktop, deep, registry, shortcut, steam]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entries.len(), 5);
    let primary = result[0].primary_entry().unwrap();
    assert_eq!(primary.source.as_str(), "steam");
    assert_eq!(
        primary.icon.as_deref(),
        Some(Path::new("/icons/portal.ico"))
    );
    assert_eq!(primary.size, Some(12345));
}
#[test]
fn section_8_1_gog_five_sources_share_one_id() {
    let (_t, env) = fixture();
    let id = ExternalId::Gog {
        product_id: 1207658924,
    };
    let entries = [
        ("gog", "1207658924"),
        ("heroic.gog", "1207658924"),
        ("minigalaxy", "1207658924"),
        ("windows.uninstall", "1207658924_is1"),
        ("deep.gog", "goggame-1207658924.info"),
    ]
    .into_iter()
    .map(|(s, k)| game(s, k, Some(id.clone()), None))
    .collect();
    let result = groups(&env, entries);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entries.len(), 5);
}
#[test]
fn section_8_1_same_and_containing_dirs_and_never_title_alone() {
    let (_t, env) = fixture();
    let mut a = game("one", "first", None, Some("/games/Doom"));
    a.title = "Doom".into();
    let mut b = game("two", "second", None, Some("/games/Doom-remaster"));
    b.title = "Doom".into();
    let same = game("three", "same", None, Some("/games/Doom/./"));
    let inside = game("four", "bin", None, Some("/games/Doom/bin"));
    let mut outside = game("five", "executable", None, None);
    outside.launch.push(exe("/games/Doom-remaster2/game", &[]));
    let result = groups(&env, vec![a, b, same, inside, outside]);
    assert_eq!(result.len(), 3);
    assert_eq!(result.iter().map(|g| g.entries.len()).max(), Some(3));
}
#[test]
fn uri_identity_in_uninstall_and_all_wrappers_does_not_merge_other_games() {
    let (_t, env) = fixture();
    let a = game(
        "steam",
        "620",
        Some(ExternalId::Steam { app_id: 620 }),
        None,
    );
    let b = game(
        "steam",
        "730",
        Some(ExternalId::Steam { app_id: 730 }),
        None,
    );
    let mut first = game("linux.desktop", "first", None, None);
    first
        .launch
        .push(exe("/usr/bin/custom-wrapper", &["steam://rungameid/620"]));
    let mut second = game("linux.desktop", "second", None, None);
    second
        .launch
        .push(exe("/usr/bin/custom-wrapper", &["steam://rungameid/730"]));
    let mut uninstall = game("windows.uninstall", "other", None, None);
    uninstall.uninstall = Some(UninstallInfo {
        target: uri("steam://uninstall/620"),
        quiet: None,
        registry_key: None,
    });
    let result = groups(&env, vec![first, second, a, b, uninstall]);
    assert_eq!(result.len(), 2);
    assert_eq!(result.iter().map(|g| g.entries.len()).sum::<usize>(), 5);
}
#[test]
fn primary_priority_and_filling_preserve_original_alternatives() {
    let (_t, env) = fixture();
    let id = ExternalId::Gog { product_id: 42 };
    let launcher = game("gog", "42", Some(id.clone()), None);
    let mut registry = game(
        "windows.uninstall",
        "42_is1",
        Some(id.clone()),
        Some("/games/Game"),
    );
    registry.size = Some(5);
    registry.confidence = Confidence::Likely;
    let shortcut = game("windows.shortcuts", "game.lnk", Some(id), None);
    let entries = vec![shortcut.clone(), registry.clone(), launcher.clone()];
    let result = groups(&env, entries.clone());
    let p = result[0].primary_entry().unwrap();
    assert_eq!(p.source.as_str(), "gog");
    assert_eq!(p.size, Some(5));
    assert_eq!(p.install_dir, registry.install_dir);
    let config = MergeConfig {
        source_priority: vec![SourceId::new("windows.shortcuts")],
        ..Default::default()
    };
    let result = merge(&env, entries, &config).unwrap();
    assert_eq!(
        result[0].primary_entry().unwrap().source.as_str(),
        "windows.shortcuts"
    );
    assert_eq!(result[0].entries[1], launcher); // source preference keeps original alternate data.
}
#[test]
fn user_exclusion_keys_ids_and_globs_remove_the_whole_game() {
    let (_t, env) = fixture();
    let a = game(
        "steam",
        "620",
        Some(ExternalId::Steam { app_id: 620 }),
        Some("/games/Portal"),
    );
    let mut b = game("linux.desktop", "portal", None, None);
    b.launch.push(uri("steam://rungameid/620"));
    for exclusions in [
        Exclusions {
            external_ids: vec![ExternalId::Steam { app_id: 620 }],
            ..Default::default()
        },
        Exclusions {
            paths: vec!["/games/Por*".into()],
            ..Default::default()
        },
        Exclusions {
            source_keys: vec![SourceKey {
                source: SourceId::new("linux.desktop"),
                key: "portal".into(),
            }],
            ..Default::default()
        },
    ] {
        assert!(
            merge(
                &env,
                vec![a.clone(), b.clone()],
                &MergeConfig {
                    exclusions,
                    ..Default::default()
                }
            )
            .unwrap()
            .is_empty()
        );
    }
    assert!(
        merge(
            &env,
            vec![a],
            &MergeConfig {
                exclusions: Exclusions {
                    paths: vec!["[".into()],
                    ..Default::default()
                },
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn built_in_exclusions_leave_caskade_storage_and_non_games_out() {
    let (_t, env) = fixture();
    fs::create_dir_all(env.fs_root().join("managed/.caskade")).unwrap();
    fs::write(env.fs_root().join("managed/.caskade/install.json"), "{}").unwrap();
    let managed = game("deep", "owned", None, Some("/managed/bin"));
    let mut launcher = game("windows.uninstall", "steam", None, None);
    launcher.launch.push(exe("/apps/Steam/steam.exe", &[]));
    let mut emulator = game("linux.desktop", "retroarch", None, None);
    emulator.launch.push(exe("/usr/bin/retroarch", &[]));
    let mut redist = game("windows.uninstall", "redist", None, None);
    redist
        .launch
        .push(exe("C:/Installers/vc_redist.x64.exe", &[]));
    let mut classified = game("classifier", "negative", None, Some("/not-a-game"));
    classified.evidence.push(Evidence::Rule {
        rule: "category.sdk".into(),
        score: -100,
    });
    let real = game(
        "gog",
        "real",
        Some(ExternalId::Gog { product_id: 42 }),
        Some("/real"),
    );
    let result = groups(
        &env,
        vec![managed, launcher, emulator, redist, classified, real],
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entries[0].key, "real");
}
#[test]
fn proxy_shortcuts_attach_only_to_an_existing_owner_and_legendary_argv_is_identity_only() {
    let (_t, env) = fixture();
    let mut proxy = game(
        "steam.shortcuts",
        "9",
        Some(ExternalId::SteamShortcut { app_id: 9 }),
        None,
    );
    proxy
        .launch
        .push(exe("/usr/bin/legendary", &["launch", "app"]));
    proxy.evidence.push(Evidence::Rule {
        rule: "steam.shortcut.proxy".into(),
        score: 0,
    });
    assert!(groups(&env, vec![proxy.clone()]).is_empty());
    let owner = game(
        "heroic.epic",
        "app",
        Some(ExternalId::Epic {
            namespace: "ns".into(),
            catalog_item: "item".into(),
            app_name: "app".into(),
        }),
        None,
    );
    let result = groups(&env, vec![proxy, owner]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entries.len(), 2);
    assert_eq!(result[0].entries[0].source.as_str(), "heroic.epic");
}
#[test]
fn low_confidence_and_transitive_multi_id_groups() {
    let (_t, env) = fixture();
    let mut a = game("one", "a", Some(ExternalId::Steam { app_id: 620 }), None);
    a.ids.push(ExternalId::Gog { product_id: 42 });
    a.confidence = Confidence::Maybe;
    let b = game("two", "b", Some(ExternalId::Gog { product_id: 42 }), None);
    let c = game("three", "c", Some(ExternalId::Steam { app_id: 620 }), None);
    assert_eq!(groups(&env, vec![a.clone(), b, c])[0].entries.len(), 3);
    assert!(
        merge(
            &env,
            vec![a],
            &MergeConfig {
                min_confidence: Confidence::Likely,
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty()
    );
}
#[test]
fn output_is_stable_when_input_completion_order_changes() {
    let (_t, env) = fixture();
    let entries = vec![
        game("steam", "a", Some(ExternalId::Steam { app_id: 1 }), None),
        game("steam", "b", Some(ExternalId::Steam { app_id: 2 }), None),
        game(
            "windows.uninstall",
            "a",
            Some(ExternalId::Steam { app_id: 1 }),
            None,
        ),
    ];
    let expected = groups(&env, entries.clone());
    let mut reverse = entries;
    reverse.reverse();
    assert_eq!(groups(&env, reverse), expected);
}
struct Fake(Vec<DetectedGame>);
impl Source for Fake {
    fn id(&self) -> SourceId {
        SourceId::new("fake")
    }
    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, _: &HostEnv) -> Vec<caskade_discovery::WatchPath> {
        vec![]
    }
    fn scan(&self, _: &HostEnv, _: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        Ok(self.0.clone())
    }
}
#[test]
fn orchestrator_uses_merge_priority_and_exclusions() {
    let (_t, env) = fixture();
    let a = game(
        "steam",
        "620",
        Some(ExternalId::Steam { app_id: 620 }),
        None,
    );
    let b = game(
        "windows.uninstall",
        "key",
        Some(ExternalId::Steam { app_id: 620 }),
        None,
    );
    let sources: Vec<Arc<dyn Source>> = vec![Arc::new(Fake(vec![a, b]))];
    let config = Config {
        source_priority: vec![SourceId::new("windows.uninstall")],
        ..Default::default()
    };
    let report = scan_with_sources(&env, &config, &Cancel::new(), &sources).unwrap();
    assert_eq!(report.groups.len(), 1);
    assert_eq!(
        report.groups[0].entries[0].source.as_str(),
        "windows.uninstall"
    );
    let config = Config {
        exclusions: Exclusions {
            external_ids: vec![ExternalId::Steam { app_id: 620 }],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(
        scan_with_sources(&env, &config, &Cancel::new(), &sources)
            .unwrap()
            .groups
            .is_empty()
    );
}
#[test]
fn many_unrelated_folders_and_sibling_prefixes_are_not_coalesced() {
    let (_t, env) = fixture();
    let entries = (0..2000)
        .map(|i| {
            game(
                "deep.folders",
                &i.to_string(),
                None,
                Some(&format!("/games/Game{i}")),
            )
        })
        .collect();
    assert_eq!(groups(&env, entries).len(), 2000);
}

#[test]
fn real_steam_shortcut_fixture_proxies_join_synthetic_store_owners() {
    let scenario =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/machines/steam-shortcuts");
    let env = HostEnv::fixture(scenario).unwrap();
    let source = caskade_discovery::sources::steam::shortcuts::SteamShortcuts;
    let shortcuts = source.scan(&env, &ScanCtx::new(Cancel::new())).unwrap();
    assert_eq!(shortcuts.len(), 6);
    let unmatched = groups(&env, shortcuts.clone());
    assert_eq!(unmatched.len(), 3);
    let mut entries = shortcuts;
    for app in ["FixtureEpicApp", "FixtureEpicTwo"] {
        entries.push(game(
            "heroic.epic",
            app,
            Some(ExternalId::Epic {
                namespace: "ns".into(),
                catalog_item: "item".into(),
                app_name: app.into(),
            }),
            None,
        ));
    }
    entries.push(game(
        "steam",
        "620",
        Some(ExternalId::Steam { app_id: 620 }),
        None,
    ));
    let result = groups(&env, entries);
    assert_eq!(result.len(), 6);
    assert_eq!(result.iter().filter(|g| g.entries.len() == 2).count(), 3);
    assert!(
        result
            .iter()
            .filter(|g| g.entries.len() == 2)
            .all(|g| g.entries[0].source.as_str() != "steam.shortcuts")
    );
}

#[test]
fn identity_only_proxies_cannot_join_unrelated_games_via_a_shared_working_folder() {
    let (_t, env) = fixture();
    let a = game(
        "steam",
        "620",
        Some(ExternalId::Steam { app_id: 620 }),
        Some("/games/Portal"),
    );
    let b = game(
        "steam",
        "730",
        Some(ExternalId::Steam { app_id: 730 }),
        Some("/games/Other"),
    );
    let mut proxy = game("linux.desktop", "proxy", None, Some("/games"));
    proxy.launch.push(uri("steam://rungameid/620"));
    caskade_discovery::uri::mark_proxy(&mut proxy);
    let result = groups(&env, vec![a, b, proxy]);
    assert_eq!(result.len(), 2);
    assert_eq!(result.iter().filter(|g| g.entries.len() == 2).count(), 1);
}

#[test]
fn ambiguous_epic_app_names_do_not_override_distinct_complete_catalog_ids() {
    let (_t, env) = fixture();
    let a = game(
        "epic",
        "a",
        Some(ExternalId::Epic {
            namespace: "namespace-a".into(),
            catalog_item: "item".into(),
            app_name: "shared-app-name".into(),
        }),
        None,
    );
    let b = game(
        "epic",
        "b",
        Some(ExternalId::Epic {
            namespace: "namespace-b".into(),
            catalog_item: "item".into(),
            app_name: "shared-app-name".into(),
        }),
        None,
    );
    let mut proxy = game("linux.desktop", "proxy", None, None);
    proxy
        .launch
        .push(uri("heroic://launch/legendary/shared-app-name"));
    caskade_discovery::uri::mark_proxy(&mut proxy);
    let result = groups(&env, vec![a.clone(), b.clone(), proxy]);
    assert_eq!(result.len(), 2);
    let config = MergeConfig {
        exclusions: Exclusions {
            external_ids: a.ids.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = merge(&env, vec![a, b], &config).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entries[0].key, "b");
}
#[test]
fn windows_path_globs_obey_volume_case_and_original_machine_spelling() {
    let t = crate::fixtures::tempdir();
    fs::create_dir(t.path().join("root")).unwrap();
    fs::write(t.path().join("machine.toml"), "os = 'windows'\n").unwrap();
    let env = HostEnv::fixture(t.path()).unwrap();
    let entry = game("deep.folders", "cafe", None, Some("C:/Games/Café"));
    let config = MergeConfig {
        exclusions: Exclusions {
            paths: vec!["c:/games/café".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(merge(&env, vec![entry], &config).unwrap().is_empty());
}

#[test]
fn explicit_game_category_is_not_overruled_by_a_launcher_named_executable() {
    let (_t, env) = fixture();
    let mut entry = game("linux.desktop", "real-game", None, Some("/games/MyGame"));
    entry.launch.push(exe("/games/MyGame/steam.exe", &[]));
    entry.evidence.push(Evidence::Rule {
        rule: "metadata.game_category".into(),
        score: 65,
    });
    assert_eq!(groups(&env, vec![entry]).len(), 1);
}

#[test]
fn proxy_roles_and_metadata_never_replace_or_mark_a_real_owner() {
    let (_t, env) = fixture();
    let owner = game(
        "heroic.epic",
        "app",
        Some(ExternalId::Epic {
            namespace: "ns".into(),
            catalog_item: "item".into(),
            app_name: "app".into(),
        }),
        None,
    );
    let mut proxy = game(
        "steam.shortcuts",
        "proxy",
        Some(ExternalId::SteamShortcut { app_id: 7 }),
        Some("/wrong-wrapper-dir"),
    );
    proxy.launch.push(uri("heroic://launch/legendary/app"));
    proxy.icon = Some("/wrapper-icon".into());
    caskade_discovery::uri::mark_proxy(&mut proxy);
    let config = MergeConfig {
        source_priority: vec![SourceId::new("steam.shortcuts")],
        ..Default::default()
    };
    let result = merge(&env, vec![proxy, owner], &config).unwrap();
    let primary = &result[0].entries[0];
    assert_eq!(primary.source.as_str(), "heroic.epic");
    assert_eq!(primary.install_dir, None);
    assert_eq!(primary.icon, None);
    assert!(
        primary
            .ids
            .contains(&ExternalId::SteamShortcut { app_id: 7 })
    );
    assert!(!primary.evidence.iter().any(
        |e| matches!(e,Evidence::Rule { rule,.. } if rule==caskade_discovery::uri::PROXY_RULE)
    ));
}

#[test]
fn native_and_flatpak_database_row_numbers_do_not_bridge_store_games() {
    let (_t, env) = fixture();
    let mut native = game(
        "lutris",
        "native:1",
        Some(ExternalId::Steam { app_id: 620 }),
        Some("/games/SteamGame"),
    );
    native.launch.push(uri("lutris:rungameid/1"));
    let mut flatpak = game(
        "lutris",
        "flatpak:1",
        Some(ExternalId::Gog { product_id: 42 }),
        Some("/games/GogGame"),
    );
    flatpak.launch.push(LaunchTarget::Flatpak {
        app_id: "net.lutris.Lutris".into(),
        args: vec!["lutris:rungameid/1".into()],
    });
    let mut native_proxy = game("linux.desktop", "native", None, None);
    native_proxy.launch.push(uri("lutris:rungameid/1"));
    caskade_discovery::uri::mark_proxy(&mut native_proxy);
    let mut flatpak_proxy = game("steam.shortcuts", "flatpak", None, None);
    flatpak_proxy.launch = vec![
        uri("lutris:rungameid/1"),
        LaunchTarget::Flatpak {
            app_id: "net.lutris.Lutris".into(),
            args: vec!["lutris:rungameid/1".into()],
        },
    ];
    caskade_discovery::uri::mark_proxy(&mut flatpak_proxy);
    let result = groups(&env, vec![native, flatpak, native_proxy, flatpak_proxy]);
    assert_eq!(result.len(), 2);
    assert!(result.iter().all(|g| g.entries.len() == 2));
    for g in result {
        let ids = &g.entries[0].ids;
        assert!(
            !(ids.contains(&ExternalId::Steam { app_id: 620 })
                && ids.contains(&ExternalId::Gog { product_id: 42 }))
        );
    }
}

#[test]
fn launcher_owned_alias_attaches_only_missing_uninstall_and_ids_and_never_stands_alone() {
    let (_temp, env) = fixture();
    let id = ExternalId::Ubisoft { id: 42 };
    let mut owner = game("ubisoft", "42", Some(id.clone()), Some("/games/Fixture"));
    owner.title = "Fixture Ubisoft".into();
    let mut alias = game(
        "windows.uninstall",
        "Uplay Install 42",
        Some(id),
        Some("/wrong/wrapper"),
    );
    alias.title = "Wrong alias title".into();
    alias.evidence.push(Evidence::Rule {
        rule: "alias.launcher_owned".into(),
        score: 0,
    });
    alias.icon = Some("/wrong/icon".into());
    alias.uninstall = Some(UninstallInfo {
        target: uri("uplay://uninstall/42"),
        quiet: None,
        registry_key: Some("HKLM/Uplay Install 42".into()),
    });
    assert!(groups(&env, vec![alias.clone()]).is_empty());
    let merged = groups(&env, vec![alias.clone(), owner.clone()]);
    let primary = merged[0].primary_entry().unwrap();
    assert_eq!(primary.title, "Fixture Ubisoft");
    assert_eq!(primary.install_dir, owner.install_dir);
    assert_eq!(primary.icon, None);
    assert!(primary.evidence.is_empty());
    assert_eq!(primary.uninstall, alias.uninstall);
    owner.uninstall = Some(UninstallInfo {
        target: uri("uplay://uninstall/owner"),
        quiet: None,
        registry_key: None,
    });
    let merged = groups(&env, vec![owner.clone(), alias]);
    assert_eq!(
        merged[0].primary_entry().unwrap().uninstall,
        owner.uninstall
    );
}

#[test]
fn idless_uninstall_aliases_use_only_exact_owner_directories() {
    let (_temp, env) = fixture();
    let owner = game(
        "ea",
        "game",
        Some(ExternalId::Ea {
            offer: "Fixture".into(),
        }),
        Some("/Games/Fixture"),
    );
    let other = game(
        "gog",
        "other",
        Some(ExternalId::Gog { product_id: 42 }),
        Some("/Games/Other"),
    );
    let make_alias = |key: &str, dir: &str| {
        let mut alias = game("uninstall", key, None, Some(dir));
        alias.evidence.push(Evidence::Rule {
            rule: "alias.launcher_owned".into(),
            score: 0,
        });
        alias.uninstall = Some(UninstallInfo {
            target: exe("/tools/uninstall", &[key]),
            quiet: None,
            registry_key: Some(key.into()),
        });
        alias
    };
    let merged = groups(
        &env,
        vec![
            owner.clone(),
            other.clone(),
            make_alias("exact", "/Games/Fixture"),
            make_alias("parent", "/Games"),
            make_alias("child", "/Games/Fixture/bin"),
        ],
    );
    assert_eq!(merged.len(), 2);
    let group = merged
        .iter()
        .find(|g| g.primary_entry().unwrap().source.as_str() == "ea")
        .unwrap();
    assert_eq!(group.entries.len(), 2);
    assert_eq!(
        group
            .primary_entry()
            .unwrap()
            .uninstall
            .as_ref()
            .unwrap()
            .registry_key
            .as_deref(),
        Some("exact")
    );
    assert_eq!(merged.iter().map(|g| g.entries.len()).sum::<usize>(), 3);
    let mut wrong = make_alias("wrong ID", "/Games/Other");
    wrong.ids = owner.ids.clone();
    // Its typed ID wins; the inconsistent path cannot bridge to the other game.
    assert_eq!(groups(&env, vec![owner, other, wrong]).len(), 2);
}

/// Counts how often the volume list is read.
struct CountingDrives(std::sync::atomic::AtomicUsize);
impl caskade_discovery::env::DriveProvider for CountingDrives {
    fn volumes(&self) -> std::io::Result<Vec<caskade_discovery::env::Volume>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(vec![])
    }
}

#[test]
fn a_merge_reads_the_volume_list_once_not_per_game() {
    let (_t, env) = fixture();
    let drives = Arc::new(CountingDrives(Default::default()));
    let env = env.with_drives(drives.clone());
    let entries = (0..50)
        .map(|i| {
            game(
                "deep.folders",
                &i.to_string(),
                None,
                Some(&format!("/games/Game{i}")),
            )
        })
        .collect();
    assert_eq!(groups(&env, entries).len(), 50);
    assert_eq!(drives.0.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn network_filesystems_are_recognised() {
    use caskade_discovery::env::network_filesystem;
    for fs in [
        "fuse.sshfs",
        "fuse.rclone",
        "fuse.s3fs",
        "nfs4",
        "cifs",
        "smbfs",
    ] {
        assert!(network_filesystem(fs), "{fs}");
    }
    for fs in [
        "ext4",
        "btrfs",
        "fuse.mergerfs",
        "fuseblk",
        "ntfs3",
        "apfs",
        "9p",
    ] {
        assert!(!network_filesystem(fs), "{fs}");
    }
}
