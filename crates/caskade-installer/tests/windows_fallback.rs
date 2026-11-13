#![cfg(feature = "windows-formats")]
use crate::fixtures;
use caskade_installer::{
    exec::{CancellationToken, ExecConfig, ExecRequest, Executor},
    formats::{
        archive::ArchiveExtractor,
        windows::{
            burn::*, generic::GenericHandler, installshield::InstallShieldHandler,
            wise::WiseHandler,
        },
    },
    host::HostCaps,
    plan::{
        ExecutionPolicy, PlanEngine, Policy,
        presets::{InstallerFamily, SwitchSettings},
    },
    probe::{ProbeEngine, ProbeLimits},
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
    *,
};
use std::{collections::BTreeMap, io::Cursor, path::Path, sync::Arc};
fn fixture() -> fixtures::Fixture {
    fixtures::load_burn(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .remove(0)
}
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(BurnProbe));
    engine
}
fn host(name: &str) -> HostCaps {
    caskade_installer::pipeline::input::host(Some(name)).unwrap()
}
fn node(format: Format) -> ArtifactNode {
    ArtifactNode {
        id: ArtifactId("source:0".into()),
        path: "setup.exe".into(),
        format,
        platform: PayloadPlatform::Windows,
        role: Role::Installer,
        size: Some(4096),
        facts: BTreeMap::new(),
        children: vec![],
        children_complete: false,
    }
}
fn planner() -> PlanEngine {
    let mut p = PlanEngine::new();
    p.register(Box::new(BurnHandler)).unwrap();
    p.register(Box::new(GenericHandler)).unwrap();
    p.register(Box::new(InstallShieldHandler)).unwrap();
    p.register(Box::new(WiseHandler)).unwrap();
    p
}
#[tokio::test]
async fn genuine_burn_embedded_msi_extracts_through_existing_safe_msi_path_and_matches_tree() {
    let fixture = fixture();
    let path = fixture.input();
    let metadata = inspect(&mut std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(metadata.payloads.len(), 1);
    assert_eq!(metadata.payloads[0].path, "setup.msi");
    let node = engine().probe_path(&path).unwrap().root;
    assert_eq!(node.format, Format::Burn);
    assert!(node.facts.contains_key("burn.metadata"));
    for profile in ["linux", "windows", "macos"] {
        let policy = Policy {
            execution: ExecutionPolicy::Never,
            ..Policy::default()
        };
        let plans = planner()
            .plan(std::slice::from_ref(&node), &host(profile), &policy)
            .unwrap();
        assert_eq!(
            plans.candidates[plans.selected.unwrap() as usize].strategy,
            Strategy::Extract
        );
        let root = TestRoot::new().unwrap();
        let sources = BTreeMap::from([(node.id.clone(), path.clone())]);
        let report = Executor::new(
            Arc::new(FakeRuntime::default()),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            Arc::new(RecordingSink::default()),
        )
        .with_extractor(Arc::new(ArchiveExtractor::new(sources.clone())))
        .with_config(ExecConfig {
            disk_margin_bytes: 0,
            ..ExecConfig::default()
        })
        .execute(
            ExecRequest {
                job_id: "burn-extract".into(),
                root: root.path().into(),
                relative_dir: "Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        fixture.assert_tree(&report.dir).unwrap();
        root.assert_sentinel_untouched();
        std::fs::remove_dir_all(report.dir).unwrap();
        root.assert_clean();
    }
}
#[test]
fn burn_silent_and_interactive_argv_are_family_specific_and_execution_policy_is_enforced() {
    let node = engine().probe_path(&fixture().input()).unwrap().root;
    for profile in ["windows", "linux", "macos"] {
        let plans = planner()
            .plan(
                std::slice::from_ref(&node),
                &host(profile),
                &Policy::default(),
            )
            .unwrap();
        for candidate in &plans.candidates {
            for step in &candidate.steps {
                match step {
                    Step::Run { args, .. } => {
                        assert_eq!(&args[..2], ["/quiet", "/norestart"]);
                        assert!(args.iter().any(|arg| arg
                            == &format!("InstallFolder={}", caskade_installer::exec::INSTALL_DIR)));
                    }
                    Step::RunInteractive { args, .. } => {
                        assert_eq!(args[0], "/norestart");
                        assert!(!args.iter().any(|arg| arg == "/quiet"));
                    }
                    _ => {}
                }
            }
        }
    }
}
#[test]
fn unknown_and_legacy_installers_use_no_unverified_silent_switches_and_record_warning() {
    for format in [
        Format::Pe,
        Format::InstallShield,
        Format::Wise,
        Format::SetupFactory,
    ] {
        let node = node(format);
        let plans = planner()
            .plan(&[node], &host("windows"), &Policy::default())
            .unwrap();
        assert_eq!(plans.candidates.len(), 1);
        let candidate = &plans.candidates[0];
        assert_eq!(candidate.strategy, Strategy::RunInteractive);
        assert!(
            candidate
                .warnings
                .iter()
                .any(|w| w.code == "installer.interactive_fallback")
        );
        let Step::RunInteractive { args, .. } = &candidate.steps[0] else {
            panic!()
        };
        assert!(args.is_empty());
    }
    let mut application = node(Format::Pe);
    application.role = Role::Application;
    assert!(
        planner()
            .plan(&[application], &host("windows"), &Policy::default())
            .unwrap()
            .candidates
            .is_empty()
    );
}
#[test]
fn installshield_basic_msi_requires_explicit_variant_and_preserves_nested_msi_argument() {
    let mut node = node(Format::InstallShield);
    node.facts
        .insert("installshield.variant".into(), "basic_msi".into());
    let plans = planner()
        .plan(&[node], &host("windows"), &Policy::default())
        .unwrap();
    let silent = plans
        .candidates
        .iter()
        .find(|c| c.strategy == Strategy::RunSilent)
        .unwrap();
    let Step::Run { args, .. } = &silent.steps[0] else {
        panic!()
    };
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "/s");
    assert_eq!(
        args[1],
        format!(
            "/v/qn /norestart INSTALLDIR=\"{}\"",
            caskade_installer::exec::INSTALL_DIR
        )
    );
    let settings = SwitchSettings {
        install_dir: "C:\\Game \"injected".into(),
        ..SwitchSettings::default()
    };
    assert!(
        caskade_installer::formats::windows::installshield::identified_msi_args(&settings).is_err()
    );
    assert!(
        !caskade_installer::plan::presets::switch_catalog()[&InstallerFamily::InstallShield]
            .silent_supported
    );
}
#[test]
fn an_available_sidecar_is_a_bounded_overlay_attempt_before_interactive_fallback() {
    let mut host = host("windows");
    host.seven_zip = Some("7zz".into());
    let mut node = node(Format::Pe);
    node.facts.insert("pe.overlay_size".into(), 1000.into());
    let plans = planner().plan(&[node], &host, &Policy::default()).unwrap();
    assert!(
        plans
            .candidates
            .iter()
            .any(|c| c.strategy == Strategy::Extract)
    );
    assert!(
        plans
            .candidates
            .iter()
            .any(|c| c.strategy == Strategy::RunInteractive)
    );
}
#[test]
fn truncated_and_lying_burn_metadata_reject_without_touching_the_root() {
    let bytes = std::fs::read(fixture().input()).unwrap();
    let root = TestRoot::new().unwrap();
    for percent in [25, 50, 75, 99] {
        assert!(inspect(&mut Cursor::new(&bytes[..bytes.len() * percent / 100])).is_err());
        root.assert_sentinel_untouched();
    }
    for xml in [b"<!DOCTYPE x [<!ENTITY a SYSTEM 'file:///etc/passwd'>]><BurnManifest>&a;</BurnManifest>".as_slice(),b"<BurnManifest><Payload Packaging='embedded' SourcePath='a0' FilePath='../outside' FileSize='1' Hash='0000000000000000000000000000000000000000'/></BurnManifest>"] {assert!(parse_manifest(xml).is_err());root.assert_sentinel_untouched();}
    root.assert_clean();
}
#[test]
fn builtin_probe_classifies_the_genuine_burn_fixture() {
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    assert_eq!(
        engine.probe_path(&fixture().input()).unwrap().root.format,
        Format::Burn
    );
}

#[tokio::test]
async fn unknown_interactive_plan_executes_and_retains_its_fallback_warning() {
    let root = TestRoot::new().unwrap();
    let node = node(Format::Pe);
    let plans = planner()
        .plan(&[node], &host("windows"), &Policy::default())
        .unwrap();
    let runtime = Arc::new(FakeRuntime::new([FakeScript {
        actions: vec![FakeAction::WriteFile {
            path: "game.exe".into(),
            contents: b"payload".to_vec(),
        }],
    }]));
    let report = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_config(ExecConfig {
        disk_margin_bytes: 0,
        ..ExecConfig::default()
    })
    .execute(
        ExecRequest {
            job_id: "unknown-interactive".into(),
            root: root.path().into(),
            relative_dir: "Game".into(),
            plans,
            sources: BTreeMap::from([(ArtifactId("source:0".into()), "setup.exe".into())]),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(report.candidate.strategy, Strategy::RunInteractive);
    assert!(
        report
            .candidate
            .warnings
            .iter()
            .any(|warning| warning.code == "installer.interactive_fallback")
    );
    assert!(runtime.processes()[0].command.args.is_empty());
    std::fs::remove_dir_all(report.dir).unwrap();
    root.assert_clean();
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "native-profile: actual WiX Burn silent install; Windows runner #22"]
async fn native_burn_bundle_installs_silently_and_actual_fixture_game_runs() {
    struct Uninstall(std::path::PathBuf);
    impl Drop for Uninstall {
        fn drop(&mut self) {
            let _ = std::process::Command::new(&self.0)
                .args(["/uninstall", "/quiet", "/norestart"])
                .status();
        }
    }
    let fixture = fixture();
    let source = dunce::canonicalize(fixture.input()).unwrap();
    let _cleanup = Uninstall(source.clone());
    let mut node = engine().probe_path(&source).unwrap().root;
    node.facts.insert(
        "installer.expect".into(),
        serde_json::json!(["FixtureGame.exe"]),
    );
    let mut plans = planner()
        .plan(
            std::slice::from_ref(&node),
            &host("windows"),
            &Policy::default(),
        )
        .unwrap();
    plans.selected = Some(
        plans
            .candidates
            .iter()
            .position(|c| c.strategy == Strategy::RunSilent)
            .unwrap() as u32,
    );
    let root = TestRoot::new().unwrap();
    let report = Executor::new(
        Arc::new(caskade_installer::runtime::NativeRuntime::new()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_config(ExecConfig {
        disk_margin_bytes: 0,
        ..ExecConfig::default()
    })
    .execute(
        ExecRequest {
            job_id: "burn-native".into(),
            root: root.path().into(),
            relative_dir: "Café Space Burn".into(),
            plans,
            sources: BTreeMap::from([(node.id.clone(), source)]),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    fixture.assert_tree(&report.dir).unwrap();
    let token = root.path().join("native-burn-token.json");
    assert!(
        std::process::Command::new(report.dir.join("FixtureGame.exe"))
            .arg("--token")
            .arg(&token)
            .current_dir(&report.dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(token.is_file());
    std::fs::remove_file(token).unwrap();
    drop(_cleanup);
    if report.dir.exists() {
        std::fs::remove_dir_all(report.dir).unwrap();
    }
    root.assert_clean();
}

#[test]
fn burn_table_cannot_overlap_pe_headers_or_claim_unbounded_attached_cabinets() {
    let bytes = std::fs::read(fixture().input()).unwrap();
    let root = TestRoot::new().unwrap();
    let offset = bytes
        .windows(8)
        .position(|part| part == [0, 0x43, 0xf1, 0, 2, 0, 0, 0])
        .unwrap();
    for (field, value) in [(24, 0_u32), (44, u32::MAX), (52, u32::MAX), (32, 1)] {
        let mut bad = bytes.clone();
        bad[offset + field..offset + field + 4].copy_from_slice(&value.to_le_bytes());
        assert!(inspect(&mut Cursor::new(bad)).is_err());
        root.assert_sentinel_untouched();
    }
    root.assert_clean();
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(256))]
    #[test]
    fn arbitrary_burn_manifest_bytes_fail_cleanly(bytes in proptest::collection::vec(proptest::num::u8::ANY,0..8192)) {
        let root=TestRoot::new().unwrap();let _=parse_manifest(&bytes);root.assert_sentinel_untouched();root.assert_clean();
    }
}
