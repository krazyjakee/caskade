#![cfg(feature = "macos-formats")]
use caskade_installer::{
    ArtifactId, ArtifactNode, Format, OperatingSystem, PayloadPlatform, Role, Strategy,
    formats::{
        archive::{self, ArchiveError, ArchiveOptions, ExtractionControl, NoopControl},
        macos::dmg::{self, DmgError, DmgExtractor, DmgHandler, DmgMetadata, MountedImage},
    },
    host::HostCaps,
    plan::{ExecutionPolicy, PlanEngine, Policy},
    safety::{Limits, SafeDir, open_staging},
    testing::{FakeAction, FakeRuntime, FakeScript, TestRoot},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn script(code: i32) -> FakeScript {
    FakeScript {
        actions: vec![FakeAction::Exit { code }],
    }
}
/// A stand-in for `hdiutil` that is absolute on every host, as `attach`
/// requires; the fake runtime never runs it.
fn hdiutil() -> PathBuf {
    std::env::temp_dir().join("hdiutil")
}
fn attach(runtime: Arc<FakeRuntime>) -> dmg::Result<MountedImage> {
    MountedImage::attach(
        runtime,
        &hdiutil(),
        &std::env::temp_dir().join("unopened-guard-test.dmg"),
        &mut NoopControl,
        Duration::from_secs(1),
    )
}
fn host(os: OperatingSystem) -> HostCaps {
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("macos")).unwrap();
    host.os = os;
    host.packages
        .managers
        .insert("hdiutil".into(), "/usr/bin/hdiutil".into());
    host
}
fn node() -> ArtifactNode {
    ArtifactNode {
        id: ArtifactId("source:0".into()),
        path: "image.dmg".into(),
        format: Format::Dmg,
        platform: PayloadPlatform::Macos,
        role: Role::Installer,
        size: Some(4096),
        facts: BTreeMap::from([(
            "macos.dmg".into(),
            serde_json::to_value(DmgMetadata {
                data_offset: 0,
                data_length: 100,
                resource_offset: 0,
                resource_length: 0,
                xml_offset: 100,
                xml_length: 50,
                expanded_bytes: 1 << 20,
                sla: true,
            })
            .unwrap(),
        )]),
        children: vec![],
        children_complete: false,
    }
}
#[test]
fn readonly_attach_has_explicit_argv_sla_stdin_and_detaches_before_releasing_scope() {
    let runtime = Arc::new(FakeRuntime::new([script(0), script(0)]));
    let mounted = attach(runtime.clone()).unwrap();
    let mount = mounted.mountpoint().to_path_buf();
    assert!(mount.exists());
    let first = &runtime.processes()[0];
    assert_eq!(
        &first.command.args[..5],
        [
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint"
        ]
    );
    assert_eq!(first.command.args[5], mount.to_str().unwrap());
    assert_eq!(first.input, b"qy\n");
    assert!(!first.killed);
    mounted.detach().unwrap();
    assert!(!mount.exists());
    let calls = runtime.processes();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].command.args, ["detach", mount.to_str().unwrap()]);
    assert!(calls.iter().all(|p| p.killed));
}
#[test]
fn drop_guard_detaches_after_error_unwind_and_attach_failure() {
    for panic in [false, true] {
        let runtime = Arc::new(FakeRuntime::new([script(0), script(0)]));
        let mounted = attach(runtime.clone()).unwrap();
        let mount = mounted.mountpoint().to_path_buf();
        if panic {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _guard = mounted;
                panic!("injected copy panic");
            }));
        } else {
            drop(mounted);
        }
        assert!(!mount.exists());
        assert_eq!(runtime.processes()[1].command.args[0], "detach");
    }
    let runtime = Arc::new(FakeRuntime::new([script(35), script(0)]));
    assert!(matches!(
        attach(runtime.clone()),
        Err(DmgError::Process {
            operation: "attach",
            code: 35
        })
    ));
    assert_eq!(runtime.processes()[1].command.args[0], "detach");
}
struct CancelAt {
    calls: usize,
    at: usize,
}
impl ExtractionControl for CancelAt {
    fn checkpoint(&mut self, _: u64) -> archive::Result<()> {
        self.calls += 1;
        if self.calls >= self.at {
            Err(ArchiveError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn progress(&mut self, _: u64, _: u64) -> archive::Result<()> {
        self.checkpoint(0)
    }
}
#[test]
fn cancellation_and_timeout_still_detach_without_consulting_cancelled_control() {
    for at in [2, 3] {
        let runtime = Arc::new(FakeRuntime::new([script(0), script(0)]));
        let result = MountedImage::attach(
            runtime.clone(),
            &hdiutil(),
            &std::env::temp_dir().join("unopened-guard-test.dmg"),
            &mut CancelAt { calls: 0, at },
            Duration::from_secs(1),
        );
        assert!(result.is_err());
        assert_eq!(
            runtime.processes().last().unwrap().command.args[0],
            "detach"
        );
        assert!(runtime.processes()[0].killed);
    }
    let runtime = Arc::new(FakeRuntime::new([
        FakeScript {
            actions: vec![FakeAction::Hang],
        },
        script(0),
    ]));
    assert!(attach(runtime.clone()).is_err());
    assert_eq!(runtime.processes()[1].command.args[0], "detach");
    assert!(runtime.processes()[0].killed);
}
#[test]
fn busy_detach_retries_force_and_total_failure_retains_mount_directory() {
    let runtime = Arc::new(FakeRuntime::new([script(0), script(16), script(0)]));
    let mounted = attach(runtime.clone()).unwrap();
    let mount = mounted.mountpoint().to_path_buf();
    mounted.detach().unwrap();
    assert!(!mount.exists());
    assert_eq!(
        runtime.processes()[2].command.args,
        ["detach", mount.to_str().unwrap(), "-force"]
    );
    let runtime = Arc::new(FakeRuntime::new([
        script(0),
        script(16),
        script(16),
        script(16),
        script(16),
    ]));
    let mounted = attach(runtime.clone()).unwrap();
    let mount = mounted.mountpoint().to_path_buf();
    std::fs::write(mount.join("sentinel-inside-fake-volume"), b"untouched").unwrap();
    assert!(matches!(mounted.detach(), Err(DmgError::Detach { .. })));
    assert_eq!(
        std::fs::read(mount.join("sentinel-inside-fake-volume")).unwrap(),
        b"untouched"
    );
    assert_eq!(runtime.processes().len(), 5);
    // This unit Runtime never mounts an actual filesystem. Remove only the
    // retained private test directory after proving cleanup did not traverse it.
    std::fs::remove_dir_all(mount.parent().unwrap()).unwrap();
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/macos-app.zip")
}
fn writer(path: &Path) -> SafeDir {
    SafeDir::new(
        open_staging(path).unwrap(),
        Limits::default(),
        1 << 20,
        true,
    )
}
#[test]
fn mounted_apps_use_capability_copy_preserve_framework_links_and_cleanup_on_copy_failure() {
    for cancel in [false, true] {
        let runtime = Arc::new(FakeRuntime::new([script(0), script(0)]));
        let mounted = attach(runtime.clone()).unwrap();
        let mount = mounted.mountpoint().to_path_buf();
        archive::extract_path(
            &fixture(),
            &mut writer(&mount),
            &ArchiveOptions::default(),
            &mut NoopControl,
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/Applications", mount.join("Applications")).unwrap();
        let root = TestRoot::new().unwrap();
        let mut dir = writer(root.path());
        let mut control = CancelAt { calls: 0, at: 2 };
        let result = if cancel {
            dmg::copy_apps(&mount, &mut dir, &mut control)
        } else {
            dmg::copy_apps(&mount, &mut dir, &mut NoopControl)
        };
        mounted.detach().unwrap();
        assert!(!mount.exists());
        if cancel {
            assert!(result.is_err());
        } else {
            let nodes = result.unwrap();
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].format, Format::AppBundle);
            assert_eq!(
                std::fs::read_link(
                    root.path()
                        .join("FixtureGame.app/Contents/Frameworks/X.framework/Versions/Current")
                )
                .unwrap(),
                Path::new("A")
            );
            assert!(!root.path().join("Applications").exists());
        }
        root.assert_sentinel_untouched();
        assert_eq!(
            runtime.processes().last().unwrap().command.args[0],
            "detach"
        );
    }
}
#[test]
fn dmgs_are_mac_payloads_and_unsupported_off_macos_without_running_tools() {
    let mut plan = PlanEngine::new();
    plan.register(Box::new(DmgHandler)).unwrap();
    let node = node();
    let policy = Policy {
        execution: ExecutionPolicy::Never,
        ..Policy::default()
    };
    for os in [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::Macos,
    ] {
        let plans = plan
            .plan(std::slice::from_ref(&node), &host(os), &policy)
            .unwrap();
        assert!(plans.question.is_none());
        if os == OperatingSystem::Macos {
            assert_eq!(plans.candidates[0].strategy, Strategy::Extract);
            assert!(!plans.candidates[0].requires.executes_code);
            assert_eq!(plans.candidates[0].warnings[0].code, "dmg_license");
        } else {
            assert!(plans.candidates.is_empty());
        }
    }
    let runtime = Arc::new(FakeRuntime::default());
    let _extractor = DmgExtractor::new(
        BTreeMap::new(),
        host(OperatingSystem::Linux),
        runtime.clone(),
    );
    assert!(runtime.processes().is_empty());
}
fn trailer() -> [u8; 512] {
    let mut raw = [0; 512];
    raw[..4].copy_from_slice(b"koly");
    raw[4..8].copy_from_slice(&4_u32.to_be_bytes());
    raw[8..12].copy_from_slice(&512_u32.to_be_bytes());
    raw[32..40].copy_from_slice(&100_u64.to_be_bytes());
    raw[216..224].copy_from_slice(&100_u64.to_be_bytes());
    raw[224..232].copy_from_slice(&100_u64.to_be_bytes());
    raw
}
#[test]
fn generated_trailer_and_plist_metadata_reject_overflows_overlap_entities_and_depth_bombs() {
    // Generated metadata only, never a fake filesystem image or native fixture.
    assert!(dmg::parse_trailer(&trailer(), 712).is_ok());
    for at in [24, 32, 40, 48, 216, 224, 492] {
        let mut raw = trailer();
        raw[at..at + 8].fill(255);
        let root = TestRoot::new().unwrap();
        assert!(dmg::parse_trailer(&raw, 712).is_err());
        root.assert_clean();
    }
    let mut raw = trailer();
    raw[216..224].fill(0);
    assert!(dmg::parse_trailer(&raw, 712).is_err());
    raw = trailer();
    raw[60..64].copy_from_slice(&2_u32.to_be_bytes());
    assert!(dmg::parse_trailer(&raw, 712).is_err());
    assert!(dmg::parse_resource_plist(b"<?xml version=\"1.0\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist><dict><key>resource-fork</key><dict><key>LPic</key><array/></dict></dict></plist>").unwrap());
    for xml in [
        "<!DOCTYPE plist [<!ENTITY x SYSTEM 'file:///sentinel'>]><plist/>".into(),
        format!(
            "<plist>{}{}</plist>",
            "<array>".repeat(65),
            "</array>".repeat(65)
        ),
        "<plist><dict>".into(),
        "<plist/><plist/>".into(),
    ] {
        let root = TestRoot::new().unwrap();
        assert!(dmg::parse_resource_plist(xml.as_bytes()).is_err());
        root.assert_clean();
    }
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(512))]
    #[test]
    fn arbitrary_udif_metadata_errors_or_stays_within_image(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..8192)){
        if let Ok(metadata)=dmg::parse_trailer(&bytes,8192){proptest::prop_assert!(metadata.xml_offset.checked_add(metadata.xml_length).is_some_and(|n|n<=8192-512));}
        let _=dmg::parse_resource_plist(&bytes);
    }
}

#[cfg(unix)]
#[test]
fn image_path_encoding_is_never_lossily_rewritten_in_native_argv() {
    use std::os::unix::ffi::OsStringExt;
    let runtime = Arc::new(FakeRuntime::default());
    let name = std::ffi::OsString::from_vec(b"image-\xff.dmg".to_vec());
    let source = std::env::temp_dir().join(name);
    assert!(matches!(
        MountedImage::attach(
            runtime.clone(),
            &hdiutil(),
            &source,
            &mut NoopControl,
            Duration::from_secs(1)
        ),
        Err(DmgError::Unsupported(_))
    ));
    assert!(runtime.processes().is_empty());
}
