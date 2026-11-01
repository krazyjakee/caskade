#![cfg(feature = "windows-sources")]
use caskade_discovery::{
    registry::*,
    sources::uninstall::{self, UninstallSource},
    *,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
fn scenario() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/machines/uninstall-windows")
}
fn env() -> HostEnv {
    HostEnv::fixture(scenario()).unwrap()
}
fn scan(env: &HostEnv) -> Vec<DetectedGame> {
    UninstallSource::new()
        .scan(env, &ScanCtx::default())
        .unwrap()
}
fn temporary() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("root")).unwrap();
    fs::write(
        t.path().join("machine.toml"),
        "schema_version=1\nos='windows'\n",
    )
    .unwrap();
    t
}
fn key(hive: RegistryHive, view: RegistryView, name: &str) -> RegistryKey {
    RegistryKey::new(hive, view, format!("{}\\{name}", uninstall::REGISTRY_PATH))
}
#[test]
fn hundred_public_product_shapes_find_games_and_reject_apps_without_live_userdata() {
    let env = env();
    let source: serde_json::Value =
        serde_json::from_slice(&fs::read(scenario().join("records.json")).unwrap()).unwrap();
    let labels = source["records"].as_array().unwrap();
    assert_eq!(labels.len(), 100);
    assert!(
        source["provenance"]
            .as_str()
            .unwrap()
            .contains("NOT registry exports")
    );
    let raw = scan(&env);
    let games: BTreeSet<_> = raw
        .iter()
        .filter(|g| {
            !g.evidence
                .iter()
                .any(|e| matches!(e,Evidence::Rule{rule,..}if rule==uninstall::ALIAS_RULE))
        })
        .map(|g| g.title.as_str())
        .collect();
    let expected: BTreeSet<_> = labels
        .iter()
        .filter(|r| r["expect"] == "game")
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert_eq!(games, expected);
    assert_eq!(games.len(), 24);
    assert_eq!(raw.len(), 32);
    for game in raw.iter().filter(|g| games.contains(game_title(g))) {
        assert_eq!(game.confidence, Confidence::Likely);
        assert!(game.uninstall.is_some());
        assert!(!game.launch.is_empty());
        assert_eq!(game.size, Some(4096 * 1024));
    }
    for record in labels
        .iter()
        .filter(|r| r["expect"] == "app" || r["expect"] == "hidden")
    {
        assert!(
            !raw.iter()
                .any(|g| Some(g.title.as_str()) == record["title"].as_str()),
            "{record}"
        );
    }
}
fn game_title(g: &DetectedGame) -> &str {
    g.title.as_str()
}
#[test]
fn steam620_merges_into_real_steam_source_and_orphan_aliases_do_not_create_games() {
    let env = env();
    let report = scan_with_sources(
        &env,
        &Config::default(),
        &Cancel::new(),
        &[
            Arc::new(sources::steam::Steam),
            Arc::new(UninstallSource::new()),
        ],
    )
    .unwrap();
    assert!(
        report.sources.iter().all(|s| s.error.is_none()),
        "{:?}",
        report.sources
    );
    let groups: Vec<_> = report
        .groups
        .iter()
        .filter(|g| {
            g.entries
                .iter()
                .any(|e| e.ids.contains(&ExternalId::Steam { app_id: 620 }))
        })
        .collect();
    assert_eq!(groups.len(), 1);
    let group = groups[0];
    assert_eq!(group.entries.len(), 2);
    let primary = group.primary_entry().unwrap();
    assert_eq!(primary.source, SourceId::new("steam"));
    assert_eq!(primary.title, "Portal 2");
    assert!(primary.uninstall.is_some());
    assert!(!report.groups.iter().any(|g| {
        g.entries
            .iter()
            .any(|e| e.ids.contains(&ExternalId::Steam { app_id: 480 }))
    }));
    assert_eq!(report.groups.len(), 25);
}
#[test]
fn records_retain_every_requested_metadata_field_and_all_four_views() {
    let records = uninstall::read_records(&env(), &ScanCtx::default()).unwrap();
    assert_eq!(records.len(), 92);
    let hollow = records
        .iter()
        .find(|r| r.display_name == "Hollow Knight")
        .unwrap();
    assert_eq!(hollow.publisher.as_deref(), Some("Team Cherry"));
    assert_eq!(hollow.install_date.as_deref(), Some("20241031"));
    assert!(
        hollow
            .display_icon
            .as_deref()
            .unwrap()
            .ends_with("game.exe\",0")
    );
    assert!(
        hollow
            .quiet_uninstall_string
            .as_deref()
            .unwrap()
            .ends_with("/VERYSILENT")
    );
    for hive in ["LocalMachine", "CurrentUser"] {
        for view in ["Registry32", "Registry64"] {
            assert!(
                records
                    .iter()
                    .any(|r| r.key.starts_with(&format!("{hive}:{view}")))
            );
        }
    }
}
#[test]
fn argv_keeps_quotes_env_expansion_and_metacharacters_literal() {
    let t = temporary();
    let dir = t.path().join("root/drives/c/Program Files/Game");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("uninstall.exe"), b"placeholder").unwrap();
    let env = HostEnv::fixture(t.path()).unwrap();
    for command in [
        r#""C:\Program Files\Game\uninstall.exe" /S "a b" "$(touch sentinel)" "&""#,
        r#"C:\Program Files\Game\uninstall.exe /S "a b" "$(touch sentinel)" "&""#,
    ] {
        let target = uninstall::command_target(&env, command).unwrap();
        assert_eq!(
            target,
            LaunchTarget::Exec {
                program: dir.join("uninstall.exe"),
                args: vec![
                    "/S".into(),
                    "a b".into(),
                    "$(touch sentinel)".into(),
                    "&".into()
                ],
                cwd: None,
                env: Default::default()
            }
        );
    }
    assert!(
        matches!(uninstall::command_target(&env,"MsiExec.exe /X{11111111-1111-1111-1111-111111111111} /qn"),Some(LaunchTarget::Exec{program,args,..}) if program==Path::new("MsiExec.exe")&&args.len()==2)
    );
    assert!(
        matches!(uninstall::command_target(&env,r#""%ProgramFiles%\Game\uninstall.exe" /S"#),Some(LaunchTarget::Exec{program,..}) if program==dir.join("uninstall.exe"))
    );
    for bad in ["..\\evil.exe /S", "C:evil.exe", "", "bad\n.exe"] {
        assert!(uninstall::command_target(&env, bad).is_none(), "{bad}");
    }
    assert!(!t.path().join("sentinel").exists());
}
#[test]
fn cancelled_and_small_key_budget_fail_cleanly() {
    let env = env();
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        UninstallSource::new().scan(&env, &ScanCtx::new(cancel)),
        Err(SourceError::Cancelled)
    ));
    let mut config = Config::default();
    config.classifier.max_entries = 1;
    assert!(
        uninstall::read_records(&env, &ScanCtx::with_config(Cancel::new(), Arc::new(config)))
            .is_err()
    );
}
#[test]
fn malformed_fields_hidden_children_dates_overflows_and_access_failures_are_bounded() {
    let t = temporary();
    let mut registry = MemoryRegistry::new();
    let k = key(
        RegistryHive::CurrentUser,
        RegistryView::Registry32,
        "Visible",
    );
    for (name, value) in [
        ("DisplayName", RegistryValue::String("Visible".into())),
        ("EstimatedSize", RegistryValue::Qword(u64::MAX)),
        ("InstallDate", RegistryValue::String("20230229".into())),
    ] {
        registry.insert(k.clone(), name, value);
    }
    let hidden = key(
        RegistryHive::CurrentUser,
        RegistryView::Registry64,
        "Hidden",
    );
    registry.insert(
        hidden.clone(),
        "DisplayName",
        RegistryValue::String("Hidden".into()),
    );
    registry.insert(
        hidden,
        "ParentKeyName",
        RegistryValue::String("Visible".into()),
    );
    let env = HostEnv::fixture(t.path())
        .unwrap()
        .with_registry(Arc::new(registry));
    let records = uninstall::read_records(&env, &ScanCtx::default()).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].estimated_bytes.is_none());
    assert!(records[0].install_date.is_none());
    assert!(scan(&env).is_empty());
    struct Denied;
    impl Registry for Denied {
        fn key_exists(&self, _: &RegistryKey) -> std::io::Result<bool> {
            Ok(true)
        }
        fn subkeys(&self, _: &RegistryKey) -> std::io::Result<Vec<String>> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        fn values(&self, _: &RegistryKey) -> std::io::Result<Vec<RegistryEntry>> {
            unreachable!()
        }
    }
    let ctx = ScanCtx::default();
    let env = env.with_registry(Arc::new(Denied));
    assert!(uninstall::read_records(&env, &ctx).unwrap().is_empty());
    assert_eq!(ctx.take_warnings().len(), 4);
}
#[test]
fn weak_store_evidence_remains_maybe_and_classifier_configuration_is_injected() {
    let t = temporary();
    let folder = t.path().join("root/drives/c/Games/Weak");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("steam_api.dll"), b"marker").unwrap();
    let mut registry = MemoryRegistry::new();
    let k = key(RegistryHive::LocalMachine, RegistryView::Registry64, "Weak");
    registry.insert(
        k.clone(),
        "DisplayName",
        RegistryValue::String("Weak".into()),
    );
    registry.insert(
        k,
        "InstallLocation",
        RegistryValue::String(r"C:\Games\Weak".into()),
    );
    let env = HostEnv::fixture(t.path())
        .unwrap()
        .with_registry(Arc::new(registry));
    assert_eq!(scan(&env)[0].confidence, Confidence::Maybe);
    let mut config = Config::default();
    config.classifier.maybe_score = 40;
    assert!(
        UninstallSource::new()
            .scan(&env, &ScanCtx::with_config(Cancel::new(), Arc::new(config)))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn public_builtin_scan_routes_uninstall_source() {
    let env = env();
    let config = Config {
        sources: SourceSelection::only([SourceId::new("uninstall")]),
        ..Default::default()
    };
    let report = caskade_discovery::scan(&env, &config, &Cancel::new()).unwrap();
    assert_eq!(report.groups.len(), 24);
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].source, SourceId::new("uninstall"));
    assert!(report.sources[0].error.is_none());
}
