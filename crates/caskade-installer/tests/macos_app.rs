#![cfg(feature = "macos-formats")]
use crate::fixtures;
use caskade_installer::{
    Format,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::{
        archive::{self, ArchiveOptions, NoopControl},
        macos::app::{self, AppExtractor, AppHandler},
    },
    host::HostCaps,
    plan::{PlanEngine, Policy},
    probe::{ProbeEngine, ProbeLimits},
    safety::{Limits, SafeDir, open_staging},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
fn fixture() -> fixtures::Fixture {
    fixtures::load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .into_iter()
        .find(|f| f.expected.id == "macos-app-zip")
        .unwrap()
}
fn host() -> HostCaps {
    caskade_installer::pipeline::input::host(Some("macos")).unwrap()
}
fn writer(root: &Path) -> SafeDir {
    SafeDir::new(
        open_staging(root).unwrap(),
        Limits::default(),
        1024 * 1024 * 1024,
        true,
    )
}
fn unpack() -> (tempfile::TempDir, PathBuf) {
    let fixture = fixture();
    let source = tempfile::tempdir().unwrap();
    archive::extract_path(
        &fixture.input(),
        &mut writer(source.path()),
        &ArchiveOptions::default(),
        &mut NoopControl,
    )
    .unwrap();
    let bundle = source.path().join("FixtureGame.app");
    (source, bundle)
}
#[test]
fn committed_bundle_metadata_folder_copy_and_discovery_match_golden() {
    let (source, bundle) = unpack();
    let metadata = app::inspect_bundle(&bundle).unwrap();
    assert_eq!(metadata.name, "Fixture Game");
    assert!(
        metadata
            .identifier
            .as_ref()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(metadata.executable, Path::new("Contents/MacOS/FixtureGame"));
    let node = app::probe_bundle(&bundle).unwrap();
    assert_eq!(node.format, Format::AppBundle);
    let root = TestRoot::new().unwrap();
    let mut safe = writer(root.path());
    app::copy_bundle(&bundle, &mut safe, &mut NoopControl).unwrap();
    fixture().assert_tree(root.path()).unwrap();
    root.assert_sentinel_untouched();
    let found = app::discover(root.path(), &host()).unwrap();
    assert_eq!(found.title, "Fixture Game");
    assert_eq!(found.ids.get("bundle"), metadata.identifier.as_ref());
    assert!(
        found
            .launch_targets
            .iter()
            .any(|target| target.path == Path::new("FixtureGame.app/Contents/MacOS/FixtureGame"))
    );
    assert!(source.path().join("FixtureGame.app").exists());
}
#[cfg(unix)]
#[test]
fn folder_copy_preserves_framework_links_modes_xattrs_and_quarantine() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_source, bundle) = unpack();
    let framework = bundle.join("Contents/Frameworks/Example.framework");
    std::fs::create_dir_all(framework.join("Versions/A")).unwrap();
    std::fs::write(framework.join("Versions/A/Example"), b"framework bytes").unwrap();
    symlink("A", framework.join("Versions/Current")).unwrap();
    symlink("Versions/Current/Example", framework.join("Example")).unwrap();
    #[cfg(target_os = "macos")]
    xattr::set(
        framework.join("Example"),
        "org.caskade.link",
        b"link metadata",
    )
    .unwrap();
    std::fs::set_permissions(
        framework.join("Versions/A"),
        std::fs::Permissions::from_mode(0o750),
    )
    .unwrap();
    std::fs::set_permissions(
        framework.join("Versions/A/Example"),
        std::fs::Permissions::from_mode(0o6740),
    )
    .unwrap();
    #[cfg(target_os = "macos")]
    let quarantine = "com.apple.quarantine";
    #[cfg(not(target_os = "macos"))]
    let quarantine = "user.com.apple.quarantine";
    xattr::set(
        &bundle,
        quarantine,
        b"0083;fixture;Caskade;quarantine-preserved",
    )
    .unwrap();
    #[cfg(target_os = "macos")]
    let custom = "org.caskade.fixture";
    #[cfg(not(target_os = "macos"))]
    let custom = "user.caskade.fixture";
    xattr::set(
        framework.join("Versions/A/Example"),
        custom,
        b"original metadata",
    )
    .unwrap();
    let root = TestRoot::new().unwrap();
    let mut safe = writer(root.path());
    app::copy_bundle(&bundle, &mut safe, &mut NoopControl).unwrap();
    let installed = root.path().join("FixtureGame.app");
    let framework = installed.join("Contents/Frameworks/Example.framework");
    assert_eq!(
        std::fs::read_link(framework.join("Versions/Current")).unwrap(),
        Path::new("A")
    );
    assert_eq!(
        std::fs::read_link(framework.join("Example")).unwrap(),
        Path::new("Versions/Current/Example")
    );
    assert_eq!(
        std::fs::read(framework.join("Example")).unwrap(),
        b"framework bytes"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(
        xattr::get(framework.join("Example"), "org.caskade.link")
            .unwrap()
            .unwrap(),
        b"link metadata"
    );
    assert_eq!(
        std::fs::metadata(framework.join("Versions/A"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o750
    );
    assert_eq!(
        std::fs::metadata(framework.join("Example"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o740
    );
    assert_eq!(
        xattr::get(&installed, quarantine).unwrap().unwrap(),
        b"0083;fixture;Caskade;quarantine-preserved"
    );
    assert_eq!(
        xattr::get(framework.join("Versions/A/Example"), custom)
            .unwrap()
            .unwrap(),
        b"original metadata"
    );
    assert!(
        safe.set_entry_xattr(
            "FixtureGame.app/Contents/Frameworks/Example.framework/Example",
            std::ffi::OsStr::new(custom),
            b"unsafe"
        )
        .is_err()
    );
    root.assert_sentinel_untouched();
}
async fn install_zip_fixture(root: &TestRoot) -> caskade_installer::exec::ExecReport {
    let fixture = fixture();
    let input = fixture.input();
    let probe = ProbeEngine::with_builtin_formats(ProbeLimits::default());
    let mut node = probe.probe_path(&input).unwrap().root;
    let id = node.id.clone();
    probe
        .list_children(&mut std::fs::File::open(&input).unwrap(), &mut node, &id)
        .unwrap();
    let mut planner = PlanEngine::new();
    planner.register(Box::new(AppHandler)).unwrap();
    let plans = planner
        .plan(&[node.clone()], &host(), &Policy::default())
        .unwrap();
    let sources = BTreeMap::from([(id.clone(), input)]);
    let extractor = Arc::new(AppExtractor::new(sources.clone()));
    let runtime = Arc::new(FakeRuntime::default());
    let report = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor.clone())
    .execute(
        ExecRequest {
            job_id: "macos-app-zip".into(),
            root: root.path().into(),
            relative_dir: "Bundle Game".into(),
            plans,
            sources,
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    fixture.assert_tree(&report.dir).unwrap();
    assert!(runtime.processes().is_empty());
    assert_eq!(
        extractor.take_bundles(&id).unwrap()[Path::new("FixtureGame.app")].executable,
        Path::new("Contents/MacOS/FixtureGame")
    );
    let discovery = app::discover(&report.dir, &host()).unwrap();
    assert_eq!(discovery.title, "Fixture Game");
    root.assert_sentinel_untouched();
    report
}
#[tokio::test]
async fn committed_zipped_bundle_installs_without_running_payload() {
    let root = TestRoot::new().unwrap();
    let report = install_zip_fixture(&root).await;
    // The committed bundle deliberately uses a portable shell entrypoint,
    // never synthetic Mach-O bytes. Installation above ran zero processes.
    #[cfg(unix)]
    {
        let token = root.path().join("portable-app-token");
        let output = std::process::Command::new(
            report
                .dir
                .join("FixtureGame.app/Contents/MacOS/FixtureGame"),
        )
        .args(["--token", token.to_str().unwrap()])
        .output()
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            std::fs::read_to_string(token)
                .unwrap()
                .trim()
                .ends_with("/FixtureGame")
        );
    }
    #[cfg(not(unix))]
    let _ = report;
    root.assert_sentinel_untouched();
}
#[test]
fn hostile_plists_and_links_fail_cleanly_and_leave_sentinel_untouched() {
    for executable in [
        "../escape",
        "/tmp/escape",
        "C:\\escape",
        "NUL",
        "MacOS/escape",
    ] {
        let bytes = format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{executable}</string></dict></plist>"
        );
        assert!(app::parse_info(bytes.as_bytes()).is_err());
    }
    let mut binary = vec![0; 40];
    binary[..8].copy_from_slice(b"bplist00");
    binary[8 + 6] = 8;
    binary[8 + 7] = 8;
    binary[8 + 8..8 + 16].fill(255);
    assert!(app::parse_info(&binary).is_err());
    let deep = format!(
        "<plist>{}{}{}",
        "<array>".repeat(65),
        "</array>".repeat(65),
        "</plist>"
    );
    assert!(app::parse_info(deep.as_bytes()).is_err());
    #[cfg(unix)]
    let (_source, bundle) = unpack();
    let root = TestRoot::new().unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.sentinel_path(), bundle.join("escape")).unwrap();
        assert!(app::copy_bundle(&bundle, &mut writer(root.path()), &mut NoopControl).is_err());
    }
    root.assert_sentinel_untouched();
}
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "native macOS runner (#22): launch committed app entrypoint and verify macOS metadata"]
async fn native_macos_installed_zip_launches_and_framework_links_survive() {
    let root = TestRoot::new().unwrap();
    let report = install_zip_fixture(&root).await;
    let framework = report
        .dir
        .join("FixtureGame.app/Contents/Frameworks/X.framework");
    assert!(
        std::fs::symlink_metadata(framework.join("Versions/Current"))
            .unwrap()
            .is_symlink()
    );
    assert!(std::fs::read(framework.join("X")).is_ok());
    let token = root.path().join("native-token");
    let status = std::process::Command::new(
        report
            .dir
            .join("FixtureGame.app/Contents/MacOS/FixtureGame"),
    )
    .args(["--token", token.to_str().unwrap()])
    .status()
    .unwrap();
    assert!(status.success());
    assert!(token.is_file());
    folder_copy_preserves_framework_links_modes_xattrs_and_quarantine();
    root.assert_sentinel_untouched();
}

fn appledouble_attribute(name: &str, value: &[u8]) -> Vec<u8> {
    let name_length = name.len() + 1;
    let entry_length = (11 + name_length + 3) & !3;
    let start = 120 + entry_length;
    let end = start + value.len();
    let mut bytes = vec![0; end];
    fn put(bytes: &mut [u8], at: usize, value: usize) {
        bytes[at..at + 4].copy_from_slice(&(value as u32).to_be_bytes());
    }
    put(&mut bytes, 0, 0x00051607);
    put(&mut bytes, 4, 0x00020000);
    bytes[24..26].copy_from_slice(&2_u16.to_be_bytes());
    put(&mut bytes, 26, 9);
    put(&mut bytes, 30, 50);
    put(&mut bytes, 34, end - 50);
    put(&mut bytes, 38, 2);
    put(&mut bytes, 42, end);
    put(&mut bytes, 46, 0);
    put(&mut bytes, 84, 0x41545452);
    put(&mut bytes, 92, end);
    put(&mut bytes, 96, start);
    put(&mut bytes, 100, value.len());
    bytes[118..120].copy_from_slice(&1_u16.to_be_bytes());
    put(&mut bytes, 120, start);
    put(&mut bytes, 124, value.len());
    bytes[130] = name_length as u8;
    bytes[131..131 + name.len()].copy_from_slice(name.as_bytes());
    bytes[start..].copy_from_slice(value);
    bytes
}
#[test]
fn appledouble_attributes_are_bounded_validated_and_restored_without_privileges() {
    #[cfg(target_os = "macos")]
    let name = "com.apple.quarantine";
    #[cfg(not(target_os = "macos"))]
    let name = "user.com.apple.quarantine";
    let bytes = appledouble_attribute(name, b"quarantine-original");
    assert_eq!(
        app::parse_appledouble(&bytes).unwrap(),
        vec![(name.into(), b"quarantine-original".to_vec())]
    );
    for offset in [30, 34, 96, 100, 120, 124] {
        let mut hostile = bytes.clone();
        hostile[offset..offset + 4].fill(255);
        assert!(app::parse_appledouble(&hostile).is_err(), "offset{offset}");
    }
    let root = TestRoot::new().unwrap();
    let mut safe = writer(root.path());
    safe.write_file(
        "FixtureGame.app/Contents/Resources/gameinfo",
        b"data".as_slice(),
        Some(4),
        0,
        0o644,
    )
    .unwrap();
    safe.write_file(
        "__MACOSX/FixtureGame.app/Contents/Resources/._gameinfo",
        bytes.as_slice(),
        Some(bytes.len() as u64),
        0,
        0o644,
    )
    .unwrap();
    #[cfg(unix)]
    {
        app::restore_zip_attributes(&mut safe).unwrap();
        assert_eq!(
            xattr::get(
                root.path()
                    .join("FixtureGame.app/Contents/Resources/gameinfo"),
                name
            )
            .unwrap()
            .unwrap(),
            b"quarantine-original"
        );
        assert!(matches!(
            safe.set_entry_xattr(
                "FixtureGame.app/Contents/Resources/gameinfo",
                std::ffi::OsStr::new("security.capability"),
                b"privilege"
            ),
            Err(caskade_installer::safety::SafetyError::UnsafeAttribute)
        ));
    }
    #[cfg(not(unix))]
    assert!(app::restore_zip_attributes(&mut safe).is_err());
    root.assert_sentinel_untouched();
}
#[test]
fn binary_plist_is_supported_but_recursive_and_deep_graphs_are_rejected() {
    let value = plist::Value::Dictionary(plist::Dictionary::from_iter([
        (
            "CFBundleExecutable",
            plist::Value::String("FixtureGame".into()),
        ),
        (
            "CFBundleIdentifier",
            plist::Value::String("org.caskade.fixture".into()),
        ),
    ]));
    let mut bytes = Vec::new();
    value.to_writer_binary(&mut bytes).unwrap();
    assert_eq!(
        app::parse_info(&bytes).unwrap().executable,
        Path::new("Contents/MacOS/FixtureGame")
    );
    let mut cyclic = b"bplist00".to_vec();
    cyclic.extend_from_slice(&[0xa1, 0, 8]);
    let mut trailer = [0_u8; 32];
    trailer[6] = 1;
    trailer[7] = 1;
    trailer[15] = 1;
    trailer[31] = 10;
    cyclic.extend_from_slice(&trailer);
    assert!(app::parse_info(&cyclic).is_err());
    let mut deep = b"bplist00".to_vec();
    for id in 1_u8..=66 {
        deep.extend_from_slice(&[0xa1, id]);
    }
    deep.push(0x09);
    let table = deep.len();
    for id in 0..67 {
        deep.extend_from_slice(&(8 + id * 2_u16).to_be_bytes());
    }
    let mut trailer = [0_u8; 32];
    trailer[6] = 2;
    trailer[7] = 1;
    trailer[15] = 67;
    trailer[24..].copy_from_slice(&(table as u64).to_be_bytes());
    deep.extend_from_slice(&trailer);
    assert!(app::parse_info(&deep).is_err());
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(512))]
    #[test]
    fn arbitrary_bundle_metadata_is_a_clean_error_or_confined_leaf(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..8192)) {
        if let Ok(info)=app::parse_info(&bytes) {proptest::prop_assert!(info.executable.starts_with("Contents/MacOS"));proptest::prop_assert_eq!(info.executable.components().count(),3);}
        let _=app::parse_appledouble(&bytes);
    }
}

#[test]
fn bundle_icns_is_normalized_to_png_and_builtin_directory_probe_is_opt_in() {
    let (_source, bundle) = unpack();
    let path = bundle.join("Contents/Info.plist");
    let mut value = plist::Value::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
    value.as_dictionary_mut().unwrap().insert(
        "CFBundleIconFile".into(),
        plist::Value::String("FixtureIcon".into()),
    );
    value.to_file_xml(&path).unwrap();
    let mut family = icns::IconFamily::new();
    let image = icns::Image::from_data(
        icns::PixelFormat::RGBA,
        16,
        16,
        [10_u8, 80, 170, 255].repeat(16 * 16),
    )
    .unwrap();
    family.add_icon(&image).unwrap();
    std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
    family
        .write(std::fs::File::create(bundle.join("Contents/Resources/FixtureIcon.icns")).unwrap())
        .unwrap();
    assert_eq!(
        ProbeEngine::default()
            .probe_path(&bundle)
            .unwrap()
            .root
            .format,
        Format::Directory
    );
    assert_eq!(
        ProbeEngine::with_builtin_formats(ProbeLimits::default())
            .probe_path(&bundle)
            .unwrap()
            .root
            .format,
        Format::AppBundle
    );
    let root = TestRoot::new().unwrap();
    app::copy_bundle(&bundle, &mut writer(root.path()), &mut NoopControl).unwrap();
    let metadata = app::inspect_bundle(&bundle).unwrap();
    assert_eq!(
        metadata.icon,
        Some("Contents/Resources/FixtureIcon.icns".into())
    );
    let found = app::discover(root.path(), &host()).unwrap();
    let icon = root.path().join(found.icon.unwrap());
    let image = image::open(icon).unwrap();
    assert_eq!((image.width(), image.height()), (16, 16));
    root.assert_sentinel_untouched();
}
