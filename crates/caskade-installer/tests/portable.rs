use caskade_installer::{
    discover::DiscoveryHints,
    exec::{CancellationToken, Executor},
    formats::{
        portable::{FolderInstallRequest, FolderSourceHandling, install_folder},
        rom::{RomHandler, RomProbe},
    },
    host::HostCaps,
    manifest::InstallManifest,
    plan::{PlanEngine, Policy},
    probe::{ProbeEngine, ProbeLimits},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink},
    *,
};
use std::{fs, path::Path, sync::Arc};
fn host() -> HostCaps {
    toml::from_str(include_str!("hosts/linux.toml")).unwrap()
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    zip::ZipArchive::new(fs::File::open(zip).unwrap())
        .unwrap()
        .extract(dir.path())
        .unwrap();
    dir
}
fn executor(bytes: u64) -> Executor {
    Executor::new(
        Arc::new(FakeRuntime::new([])),
        Arc::new(FakeDiskSpace::fixed(bytes)),
        Arc::new(RecordingSink::default()),
    )
}
fn request(source: &Path, root: &Path, mode: FolderSourceHandling) -> FolderInstallRequest {
    FolderInstallRequest {
        job_id: "portable-test".into(),
        source: source.into(),
        root: root.into(),
        root_id: "test-root".into(),
        relative_dir: "Fixture Game".into(),
        host: host(),
        hints: DiscoveryHints {
            installer_title: Some("Fixture Game".into()),
            ..DiscoveryHints::default()
        },
        source_handling: mode,
    }
}
#[tokio::test]
async fn unity_copy_uses_executor_discovery_and_durable_manifest() {
    let source = fixture();
    let root = tempfile::tempdir().unwrap();
    let outcome = install_folder(
        executor(u64::MAX),
        request(source.path(), root.path(), FolderSourceHandling::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let InstallOutcome::Installed(installed) = outcome else {
        panic!("expected installed");
    };
    assert_eq!(
        installed.launch_targets[0].path,
        Path::new("FixtureGame.exe")
    );
    assert!(source.path().join("FixtureGame.exe").is_file());
    let game = root.path().join(&installed.dir);
    let manifest = InstallManifest::read(&game).unwrap();
    assert_eq!(manifest.record().launch_targets, installed.launch_targets);
    assert_eq!(manifest.record().plan.relative_dir, installed.dir);
    assert!(manifest.record().files.iter().all(|f| f.sha256.is_some()));
    assert!(manifest.verify(&game).unwrap().is_ok());
    assert!(
        !serde_json::to_string(&manifest)
            .unwrap()
            .contains(source.path().to_str().unwrap())
    );
}
#[tokio::test]
async fn move_removes_original_only_after_success_and_preserves_collision_destination() {
    let source = fixture();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("Fixture Game")).unwrap();
    fs::write(root.path().join("Fixture Game/keep"), "original").unwrap();
    let InstallOutcome::Installed(installed) = install_folder(
        executor(u64::MAX),
        request(source.path(), root.path(), FolderSourceHandling::Move),
        CancellationToken::new(),
    )
    .await
    .unwrap() else {
        panic!()
    };
    assert!(!source.path().exists());
    assert_ne!(installed.dir, Path::new("Fixture Game"));
    assert_eq!(
        fs::read_to_string(root.path().join("Fixture Game/keep")).unwrap(),
        "original"
    );
    assert_eq!(
        InstallManifest::read(&root.path().join(&installed.dir))
            .unwrap()
            .record()
            .plan
            .relative_dir,
        installed.dir
    );
}
#[tokio::test]
async fn disk_failure_and_cancel_retain_original() {
    for cancel in [false, true] {
        let source = fixture();
        let root = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        if cancel {
            token.cancel();
        }
        assert!(
            install_folder(
                executor(if cancel { u64::MAX } else { 0 }),
                request(source.path(), root.path(), FolderSourceHandling::Move),
                token
            )
            .await
            .is_err()
        );
        assert!(source.path().join("FixtureGame.exe").is_file());
        assert!(!root.path().join("Fixture Game").exists());
    }
}
#[tokio::test]
async fn overlapping_roots_are_rejected_before_copying() {
    let source = fixture();
    let root = source.path().join("managed");
    fs::create_dir(&root).unwrap();
    assert!(
        install_folder(
            executor(u64::MAX),
            request(source.path(), &root, FolderSourceHandling::Move),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(source.path().join("FixtureGame.exe").is_file());
    assert!(!root.join(".caskade").exists());
}
#[test]
fn fake_rom_header_and_suffix_produce_a_platform_handoff_on_every_host() {
    for (name, bytes, platform) in [
        ("Game.nes", b"NES\x1aROM".as_slice(), "nintendo_nes"),
        ("Game.sfc", b"fake SNES payload", "nintendo_snes"),
        ("wrong.nes", b"\x80\x37\x12\x40data", "nintendo_64"),
    ] {
        let mut engine = ProbeEngine::new(ProbeLimits::default());
        engine.register(Box::new(RomProbe::default()));
        let artifact = engine
            .probe_reader(&mut std::io::Cursor::new(bytes), Path::new(name))
            .unwrap()
            .root;
        let outcome = RomHandler.handoff(&artifact).unwrap();
        assert_eq!(
            outcome,
            InstallOutcome::HandOff(HandOff::Rom {
                platform: platform.into(),
                files: vec![name.into()]
            })
        );
        let mut planner = PlanEngine::new();
        planner.register(Box::new(RomHandler)).unwrap();
        for text in [
            include_str!("hosts/windows.toml"),
            include_str!("hosts/linux.toml"),
            include_str!("hosts/macos.toml"),
        ] {
            let plans = planner
                .plan(
                    std::slice::from_ref(&artifact),
                    &toml::from_str(text).unwrap(),
                    &Policy::default(),
                )
                .unwrap();
            assert_eq!(plans.candidates[0].strategy, Strategy::HandOff);
            assert_eq!(plans.selected, Some(0));
        }
    }
}
#[test]
fn common_extensions_and_renamed_executables_are_not_roms() {
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    for (name, bytes) in [
        ("readme.md", b"# documentation".as_slice()),
        ("data.bin", b"not a ROM"),
        ("game.iso", b"not a ROM"),
        ("game.nes", b"MZfake executable"),
    ] {
        assert_ne!(
            engine
                .probe_reader(&mut std::io::Cursor::new(bytes), Path::new(name))
                .unwrap()
                .root
                .format,
            Format::Rom
        );
    }
}
#[test]
fn builtin_registry_lists_archives_without_changing_minimal_default() {
    let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/unity.zip");
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    let mut root = engine.probe_path(&zip).unwrap().root;
    let id = root.id.clone();
    engine
        .list_children(&mut fs::File::open(zip).unwrap(), &mut root, &id)
        .unwrap();
    assert!(!root.children.is_empty());
}

#[tokio::test]
async fn changed_source_is_retained_after_move_commits() {
    struct MutateSource(std::path::PathBuf);
    impl caskade_installer::events::EventSink for MutateSource {
        fn emit(&self, event: caskade_installer::events::JobEvent) {
            if event.message == "finalize_completed" {
                fs::write(self.0.join("new-save.dat"), b"new data").unwrap();
            }
        }
    }
    let source = fixture();
    let root = tempfile::tempdir().unwrap();
    let exec = Executor::new(
        Arc::new(FakeRuntime::new([])),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(MutateSource(source.path().into())),
    );
    let InstallOutcome::Installed(installed) = install_folder(
        exec,
        request(source.path(), root.path(), FolderSourceHandling::Move),
        CancellationToken::new(),
    )
    .await
    .unwrap() else {
        panic!()
    };
    assert!(source.path().join("new-save.dat").exists());
    assert!(
        root.path()
            .join(installed.dir)
            .join("FixtureGame.exe")
            .exists()
    );
    assert!(
        installed
            .warnings
            .iter()
            .any(|warning| warning.code == "source_retained")
    );
}
#[tokio::test]
async fn discovery_failure_rolls_back_and_keeps_source() {
    let source = fixture();
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        source.path().join("FixtureGame.exe"),
        source.path().join("Alternate.exe"),
    )
    .unwrap();
    fs::create_dir(source.path().join("Alternate_Data")).unwrap();
    let mut req = request(source.path(), root.path(), FolderSourceHandling::Move);
    req.hints.installer_title = Some("Unrelated Title".into());
    assert!(
        install_folder(executor(u64::MAX), req, CancellationToken::new())
            .await
            .is_err()
    );
    assert!(source.path().join("FixtureGame.exe").is_file());
    assert!(!root.path().join("Fixture Game").exists());
}
#[test]
fn catalogue_can_be_replaced_with_future_platform_definitions() {
    use caskade_installer::formats::rom::PlatformDefinition;
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(RomProbe::new(vec![PlatformDefinition {
        id: "plugin_console".into(),
        name: "Plugin Console".into(),
        extensions: vec!["customrom".into()],
        signatures: vec![],
        require_header: false,
    }])));
    let root = engine
        .probe_reader(
            &mut std::io::Cursor::new(b"payload"),
            Path::new("Game.customrom"),
        )
        .unwrap()
        .root;
    assert!(
        matches!(RomHandler.handoff(&root), Some(InstallOutcome::HandOff(HandOff::Rom { platform, .. })) if platform == "plugin_console")
    );
}
