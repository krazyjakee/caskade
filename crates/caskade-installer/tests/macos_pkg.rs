#![cfg(feature = "macos-formats")]
use crate::fixtures;
use caskade_installer::{
    ArtifactNode, Fidelity, Format, PlanSet, Step, Strategy,
    exec::{CancellationToken, ExecRequest, Executor},
    formats::{
        archive::{ArchiveError, ArchiveExtractor, ExtractionControl, NoopControl},
        macos::{
            app,
            pkg::{self, PkgHandler, PkgProbe, Xar, XarLimits},
        },
    },
    host::HostCaps,
    plan::{ExecutionPolicy, PlanEngine, Policy},
    probe::{ProbeEngine, ProbeLimits},
    safety::{Limits, SafeDir, open_staging},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot, probe_tree},
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
};
fn fixture(id: &str) -> fixtures::Fixture {
    fixtures::load_macos_pkg(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .into_iter()
        .find(|f| f.expected.id == id)
        .unwrap()
}
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(PkgProbe));
    engine
}
fn host() -> HostCaps {
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("macos")).unwrap();
    host.packages.managers = BTreeMap::from([
        ("installer".into(), "/usr/sbin/installer".into()),
        ("pkgutil".into(), "/usr/sbin/pkgutil".into()),
        ("osascript".into(), "/usr/bin/osascript".into()),
    ]);
    host
}
fn plans(node: &ArtifactNode, host: &HostCaps, policy: &Policy) -> PlanSet {
    let mut plan = PlanEngine::new();
    plan.register(Box::new(PkgHandler)).unwrap();
    plan.plan(std::slice::from_ref(node), host, policy).unwrap()
}
fn writer(root: &Path, packed: u64) -> SafeDir {
    SafeDir::new(open_staging(root).unwrap(), Limits::default(), packed, true)
}
#[test]
fn genuine_gzip_odc_and_nested_pbzx_newc_packages_expand_match_golden_and_launch() {
    for id in ["mac-pkg", "mac-pkg-pbzx", "mac-pkg-unused-scripts"] {
        let fixture = fixture(id);
        let path = fixture.input();
        let mut node = engine().probe_path(&path).unwrap().root;
        assert_eq!(node.format, Format::Pkg);
        assert!(node.facts.contains_key("macos.pkg"), "{:?}", node.facts);
        let artifact = node.id.clone();
        engine()
            .list_children(
                &mut std::fs::File::open(&path).unwrap(),
                &mut node,
                &artifact,
            )
            .unwrap();
        assert!(node.children.iter().any(|n| n.path.ends_with("Payload")));
        let mut source = std::fs::File::open(&path).unwrap();
        let (xar, metadata) = pkg::inspect(&mut source, XarLimits::default()).unwrap();
        assert!(!metadata.requires_delegate);
        assert_eq!(metadata.components.len(), 1);
        let bom = xar
            .toc
            .entries
            .iter()
            .find(|e| e.path.ends_with("Bom"))
            .unwrap();
        assert!(
            xar.read_metadata(&mut source, bom, 65536)
                .unwrap()
                .starts_with(b"BOMStore")
        );
        let root = TestRoot::new().unwrap();
        let mut dir = writer(root.path(), path.metadata().unwrap().len());
        pkg::extract(source, &mut dir, &mut NoopControl, 16 * 1024 * 1024).unwrap();
        fixture.assert_tree(root.path()).unwrap();
        let found = app::discover(root.path(), &host()).unwrap();
        assert_eq!(found.title, "Fixture Game");
        #[cfg(unix)]
        {
            let token = root.path().join("pkg-token");
            let output =
                std::process::Command::new(root.path().join(&fixture.expected.launch_target))
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
        root.assert_sentinel_untouched();
    }
}
#[tokio::test]
async fn real_pkg_plan_executor_routes_archive_extractor_reprobes_and_executes_zero_scripts() {
    for id in ["mac-pkg", "mac-pkg-pbzx", "mac-pkg-unused-scripts"] {
        let fixture = fixture(id);
        let input = fixture.input();
        let node = ProbeEngine::with_builtin_formats(ProbeLimits::default())
            .probe_path(&input)
            .unwrap()
            .root;
        assert_eq!(node.format, Format::Pkg);
        let plans = plans(&node, &host(), &Policy::default());
        assert!(plans.question.is_none());
        let candidate = &plans.candidates[plans.selected.unwrap() as usize];
        assert_eq!(candidate.strategy, Strategy::Extract);
        assert_eq!(
            candidate.fidelity,
            if id.ends_with("unused-scripts") {
                Fidelity::Partial
            } else {
                Fidelity::Exact
            }
        );
        let root = TestRoot::new().unwrap();
        let sources = BTreeMap::from([(node.id.clone(), input)]);
        let extractor = Arc::new(ArchiveExtractor::new(sources.clone()));
        let runtime = Arc::new(FakeRuntime::default());
        let events = Arc::new(RecordingSink::default());
        let report = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            events.clone(),
        )
        .with_extractor(extractor.clone())
        .execute(
            ExecRequest {
                job_id: id.into(),
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
        assert!(runtime.processes().is_empty());
        assert!(
            probe_tree(&report.dir)
                .iter()
                .any(|n| n.path.ends_with("Contents/MacOS/FixtureGame"))
        );
        assert!(events.events().iter().any(|e| e.bytes > 0 && e.files > 0));
        root.assert_sentinel_untouched();
    }
}
#[test]
fn system_locations_and_active_scripts_delegate_without_caskade_question_and_policy_is_enforced() {
    for id in ["mac-pkg-system", "mac-pkg-scripts"] {
        let fixture = fixture(id);
        let node = engine().probe_path(&fixture.input()).unwrap().root;
        let set = plans(&node, &host(), &Policy::default());
        assert!(set.question.is_none());
        assert_eq!(set.candidates.len(), 1);
        let candidate = &set.candidates[set.selected.unwrap() as usize];
        assert_eq!(candidate.strategy, Strategy::Delegate);
        assert!(candidate.requires.may_prompt_elevation);
        let Step::Delegate {
            tool,
            args,
            uninstall,
            ..
        } = &candidate.steps[0]
        else {
            panic!("delegate")
        };
        assert_eq!(tool, "/usr/bin/osascript");
        assert!(args[1].contains("quoted form of"));
        assert!(args[1].contains("administrator privileges"));
        assert_eq!(
            &args[3..],
            [
                "/usr/sbin/installer",
                "-pkg",
                &caskade_installer::exec::source_argument(&node.id),
                "-target",
                "/"
            ]
        );
        assert_eq!(
            uninstall[0].args,
            ["--forget", &format!("org.caskade.fixture.{id}")]
        );
        let never = Policy {
            execution: ExecutionPolicy::Never,
            ..Policy::default()
        };
        assert!(plans(&node, &host(), &never).candidates.is_empty());
        let linux: HostCaps = caskade_installer::pipeline::input::host(Some("linux")).unwrap();
        assert!(
            plans(&node, &linux, &Policy::default())
                .candidates
                .is_empty()
        );
        let root = TestRoot::new().unwrap();
        let mut dir = writer(root.path(), 1 << 20);
        assert!(
            pkg::extract(
                std::fs::File::open(fixture.input()).unwrap(),
                &mut dir,
                &mut NoopControl,
                1 << 20
            )
            .is_err()
        );
        root.assert_clean();
        let mut root_host = host();
        root_host.packages.root = true;
        let set = plans(&node, &root_host, &Policy::default());
        let Step::Delegate { tool, args, .. } = &set.candidates[0].steps[0] else {
            panic!()
        };
        assert_eq!(tool, "/usr/sbin/installer");
        assert_eq!(args[0], "-pkg");
        assert!(!set.candidates[0].requires.may_prompt_elevation);
    }
}
#[tokio::test]
async fn delegate_resolves_source_as_one_argument_and_records_component_receipt() {
    let fixture = fixture("mac-pkg-system");
    let node = engine().probe_path(&fixture.input()).unwrap().root;
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([
        caskade_installer::testing::FakeScript::default(),
    ]));
    let report = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .execute(
        ExecRequest {
            job_id: "pkg-delegate".into(),
            root: root.path().into(),
            relative_dir: "System Game".into(),
            plans: plans(&node, &host(), &Policy::default()),
            sources: BTreeMap::from([(node.id, fixture.input())]),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let processes = runtime.processes();
    assert_eq!(processes.len(), 1);
    // The resolved source path, compared by component.
    assert_eq!(
        std::path::Path::new(&processes[0].command.args[5]),
        fixture.input()
    );
    assert_eq!(report.candidate.strategy, Strategy::Delegate);
    root.assert_sentinel_untouched();
}
fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn xar(xml: &str, heap: &[u8]) -> Vec<u8> {
    let toc = zlib(xml.as_bytes());
    let mut bytes = b"xar!".to_vec();
    bytes.extend(28_u16.to_be_bytes());
    bytes.extend(1_u16.to_be_bytes());
    bytes.extend((toc.len() as u64).to_be_bytes());
    bytes.extend((xml.len() as u64).to_be_bytes());
    bytes.extend(0_u32.to_be_bytes());
    bytes.extend(toc);
    bytes.extend(heap);
    bytes
}
fn test_package(payload: &[u8]) -> Vec<u8> {
    let info=b"<pkg-info identifier=\"org.caskade.hostile\" version=\"1\" install-location=\"/Applications\"/>";
    let members = [("PackageInfo", info.as_slice()), ("Payload", payload)];
    let mut heap = Vec::new();
    let mut xml = String::from("<xar><toc>");
    for (name, data) in members {
        xml.push_str(&format!("<file><name>{name}</name><type>file</type><data><offset>{}</offset><length>{}</length><size>{}</size></data></file>",heap.len(),data.len(),data.len()));
        heap.extend(data);
    }
    xml.push_str("</toc></xar>");
    xar(&xml, &heap)
}
fn cpio_entry(output: &mut Vec<u8>, name: &str, data: &[u8], mode: u32, inode: u32, nlink: u32) {
    output.extend(b"070701");
    for n in [
        inode,
        mode,
        0,
        0,
        nlink,
        0,
        data.len() as u32,
        0,
        0,
        0,
        0,
        name.len() as u32 + 1,
        0,
    ] {
        output.extend(format!("{n:08x}").bytes());
    }
    output.extend(name.bytes());
    output.push(0);
    while !output.len().is_multiple_of(4) {
        output.push(0);
    }
    output.extend(data);
    while !output.len().is_multiple_of(4) {
        output.push(0);
    }
}
fn hostile_payload(name: &str, data: &[u8], mode: u32) -> Vec<u8> {
    let mut output = vec![];
    cpio_entry(&mut output, name, data, mode, 1, 1);
    cpio_entry(&mut output, "TRAILER!!!", &[], 0, 0, 1);
    output
}
fn rejection(bytes: &[u8]) {
    let root = TestRoot::new().unwrap();
    let mut dir = writer(root.path(), bytes.len() as u64);
    assert!(
        pkg::extract(
            Cursor::new(bytes),
            &mut dir,
            &mut NoopControl,
            16 * 1024 * 1024
        )
        .is_err()
    );
    root.assert_sentinel_untouched();
}
#[test]
fn hostile_toc_spans_xml_entities_names_and_size_bombs_are_bounded() {
    let file = |name: &str, offset: &str, length: &str| {
        format!(
            "<file><name>{name}</name><type>file</type><data><offset>{offset}</offset><length>{length}</length><size>1</size></data></file>"
        )
    };
    for xml in [
        format!("<xar><toc>{}</toc></xar>", file("../sentinel", "0", "1")),
        format!(
            "<xar><toc>{}</toc></xar>",
            file("Payload", "18446744073709551615", "2")
        ),
        format!(
            "<xar><toc>{}{}</toc></xar>",
            file("first", "0", "2"),
            file("second", "1", "2")
        ),
        format!(
            "<xar><toc>{}{}</toc></xar>",
            file("same", "0", "1"),
            file("same", "1", "1")
        ),
        "<!DOCTYPE xar [<!ENTITY x SYSTEM 'file:///sentinel'>]><xar><toc/></xar>".into(),
        format!("<xar><toc>{}</toc></xar>", "<file>".repeat(65)),
    ] {
        let root = TestRoot::new().unwrap();
        assert!(pkg::parse_toc(xml.as_bytes(), 4096).is_err());
        root.assert_clean();
    }
    let source = std::fs::read(fixture("mac-pkg").input()).unwrap();
    for field in [8..16, 16..24] {
        let mut bytes = source.clone();
        bytes[field].fill(255);
        rejection(&bytes);
    }
    for percent in [25, 50, 75, 99] {
        rejection(&source[..source.len() * percent / 100]);
    }
    let mut checksum = source.clone();
    *checksum.last_mut().unwrap() ^= 1;
    rejection(&checksum);
    let mut toc_checksum = source;
    let offset = 28 + u64::from_be_bytes(toc_checksum[8..16].try_into().unwrap()) as usize;
    toc_checksum[offset] ^= 1;
    rejection(&toc_checksum);
}
#[test]
fn hostile_cpio_paths_links_devices_checksums_and_pbzx_fail_without_touching_sentinel() {
    for name in [
        "../sentinel",
        "/sentinel",
        "C:\\sentinel",
        "FixtureGame.app/../../sentinel",
        "NUL",
    ] {
        rejection(&test_package(&hostile_payload(name, b"bad", 0o100644)));
    }
    for mode in [0o010644, 0o020644, 0o060644, 0o140644] {
        rejection(&test_package(&hostile_payload(
            "FixtureGame.app/special",
            b"",
            mode,
        )));
    }
    for target in ["../../sentinel", "/sentinel", "C:\\sentinel"] {
        rejection(&test_package(&hostile_payload(
            "FixtureGame.app/link",
            target.as_bytes(),
            0o120777,
        )));
    }
    let mut huge = hostile_payload("FixtureGame.app/huge", b"", 0o100644);
    huge[54..62].copy_from_slice(b"ffffffff");
    rejection(&test_package(&huge));
    let mut names = hostile_payload("FixtureGame.app/file", b"", 0o100644);
    names[94..102].copy_from_slice(b"ffffffff");
    rejection(&test_package(&names));
    let mut crc = hostile_payload("FixtureGame.app/file", b"data", 0o100644);
    crc[..6].copy_from_slice(b"070702");
    rejection(&test_package(&crc));
    for block in [0, u64::MAX] {
        let mut payload = b"pbzx".to_vec();
        payload.extend(block.to_be_bytes());
        rejection(&test_package(&payload));
    }
    let mut chunk = b"pbzx".to_vec();
    chunk.extend((1_u64 << 24).to_be_bytes());
    chunk.extend(u64::MAX.to_be_bytes());
    chunk.extend(1_u64.to_be_bytes());
    rejection(&test_package(&chunk));
}
#[test]
fn cpio_hardlinks_forward_symlinks_and_exec_modes_use_safe_writer() {
    let mut payload = vec![];
    let info=b"<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>game</string></dict></plist>";
    cpio_entry(
        &mut payload,
        "FixtureGame.app/Contents/Info.plist",
        info,
        0o100644,
        1,
        1,
    );
    cpio_entry(
        &mut payload,
        "FixtureGame.app/Contents/MacOS/alias",
        b"",
        0o100755,
        2,
        2,
    );
    cpio_entry(
        &mut payload,
        "FixtureGame.app/Contents/MacOS/link",
        b"game",
        0o120777,
        3,
        1,
    );
    cpio_entry(
        &mut payload,
        "FixtureGame.app/Contents/MacOS/game",
        b"#!/bin/sh\nexit 0\n",
        0o106755,
        2,
        2,
    );
    cpio_entry(&mut payload, "TRAILER!!!", b"", 0, 0, 1);
    let root = TestRoot::new().unwrap();
    let mut dir = writer(root.path(), 1 << 20);
    pkg::extract(
        Cursor::new(test_package(&payload)),
        &mut dir,
        &mut NoopControl,
        1 << 20,
    )
    .unwrap();
    let base = root.path().join("FixtureGame.app/Contents/MacOS");
    assert_eq!(
        std::fs::read(base.join("alias")).unwrap(),
        std::fs::read(base.join("game")).unwrap()
    );
    assert_eq!(
        std::fs::read_link(base.join("link")).unwrap(),
        Path::new("game")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(base.join("game"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );
    }
    root.assert_sentinel_untouched();
}
struct Cancel;
impl ExtractionControl for Cancel {
    fn checkpoint(&mut self, _: u64) -> Result<(), ArchiveError> {
        Err(ArchiveError::Cancelled)
    }
    fn progress(&mut self, _: u64, _: u64) -> Result<(), ArchiveError> {
        Ok(())
    }
}
#[test]
fn cancellation_and_declared_payload_budget_fail_cleanly() {
    let fixture = fixture("mac-pkg-pbzx");
    let root = TestRoot::new().unwrap();
    let mut dir = writer(root.path(), 1 << 20);
    assert!(
        pkg::extract(
            std::fs::File::open(fixture.input()).unwrap(),
            &mut dir,
            &mut Cancel,
            1 << 20
        )
        .is_err()
    );
    root.assert_clean();
    assert!(
        pkg::extract(
            std::fs::File::open(fixture.input()).unwrap(),
            &mut dir,
            &mut NoopControl,
            128
        )
        .is_err()
    );
    root.assert_clean();
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(512))]
    #[test]
    fn arbitrary_xar_header_and_toc_are_clean_errors_or_checked_heap_spans(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..8192)){
        if let Ok(toc)=pkg::parse_toc(&bytes,8192){for entry in toc.entries{proptest::prop_assert!(entry.offset.checked_add(entry.length).is_some_and(|n|n<=8192));proptest::prop_assert!(caskade_installer::safety::sanitize_path(entry.path.to_str().unwrap()).is_ok());}}
        let _=Xar::inspect(&mut Cursor::new(bytes),XarLimits::default());
    }
}
