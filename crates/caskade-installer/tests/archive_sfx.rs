use caskade_installer::{
    Format,
    formats::archive::{self, ArchiveOptions, ArchiveProbe, NoopControl, PayloadReader, sfx::*},
    probe::{PeProbe, ProbeEngine},
    safety::{Limits, SafeDir, open_staging},
    testing::TestRoot,
};
use std::{
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
};

use crate::fixtures;

fn fixture(id: &str) -> fixtures::Fixture {
    fixtures::load_windows(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .into_iter()
        .find(|f| f.expected.id == id)
        .unwrap()
}
fn engine() -> ProbeEngine {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(PeProbe::new()));
    engine.register(Box::new(SfxProbe::new()));
    engine.register(Box::new(ArchiveProbe::new()));
    engine
}

#[test]
fn genuine_zip_and_seven_sfx_extract_list_and_reprobe_without_running_stubs() {
    for (id, kind, format) in [
        ("zip-sfx", "zip", Format::Zip),
        ("sevenzip-sfx", "seven_zip", Format::SevenZip),
    ] {
        let fixture = fixture(id);
        let path = fixture.input();
        let mut probe = engine().probe_path(&path).unwrap();
        assert_eq!(probe.root.format, Format::Pe);
        assert_eq!(probe.root.facts["pe.sfx_format"], kind);
        let payload = locate(
            &mut std::fs::File::open(&path).unwrap(),
            &ArchiveOptions::default(),
        )
        .unwrap();
        assert_eq!(payload.format, format);
        assert!(payload.offset >= probe.root.facts["pe.overlay_offset"].as_u64().unwrap());
        assert!(payload.offset > 0);
        let root_id = probe.root.id.clone();
        let stats = engine()
            .list_children(
                &mut std::fs::File::open(&path).unwrap(),
                &mut probe.root,
                &root_id,
            )
            .unwrap();
        assert!(stats.bytes_read < 8 * 1024 * 1024);
        assert!(probe.root.children_complete);
        assert!(
            probe
                .root
                .children
                .iter()
                .any(|n| n.path == Path::new(&fixture.expected.launch_target))
        );
        let root = TestRoot::new().unwrap();
        let mut dir = SafeDir::new(
            open_staging(root.path()).unwrap(),
            Limits::default(),
            payload.length,
            true,
        );
        let nodes = extract_path(
            &path,
            root.path(),
            None,
            &mut dir,
            &ArchiveOptions::default(),
            &mut NoopControl,
        )
        .unwrap();
        root.assert_sentinel_untouched();
        fixture.assert_tree(root.path()).unwrap();
        assert!(nodes.iter().any(
            |n| n.path == Path::new(&fixture.expected.launch_target) && n.format == Format::Pe
        ));
        // Existing archive callers also recognise SFX without selecting a new API.
        assert!(
            !archive::list_path(&path, &ArchiveOptions::default())
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn bounded_payload_view_clips_reads_and_rejects_outside_seeks() {
    let root = TestRoot::new().unwrap();
    let mut reader = PayloadReader::new(Cursor::new(b"HEADpayloadTAIL"), 4, 7).unwrap();
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    assert_eq!(out, b"payload");
    assert_eq!(reader.seek(SeekFrom::End(-3)).unwrap(), 4);
    let mut tail = [0; 8];
    assert_eq!(reader.read(&mut tail).unwrap(), 3);
    assert_eq!(&tail[..3], b"oad");
    for from in [
        SeekFrom::Start(8),
        SeekFrom::End(1),
        SeekFrom::Current(i64::MIN),
    ] {
        assert!(reader.seek(from).is_err());
    }
    assert!(PayloadReader::new(Cursor::new(b"small"), u64::MAX, 2).is_err());
    root.assert_clean();
}

#[test]
fn truncated_genuine_sfx_never_executes_or_changes_the_sentinel() {
    for id in ["zip-sfx", "sevenzip-sfx"] {
        let fixture = fixture(id);
        let bytes = std::fs::read(fixture.input()).unwrap();
        let payload = locate(&mut Cursor::new(&bytes), &ArchiveOptions::default()).unwrap();
        let truncated = &bytes[..payload.offset as usize + 10];
        let root = TestRoot::new().unwrap();
        let result = locate(&mut Cursor::new(truncated), &ArchiveOptions::default());
        root.assert_clean();
        assert!(result.is_err());
    }
}

async fn install(id: &str, tool: Option<archive::sevenzip::SevenZip>) {
    use caskade_installer::{
        exec::{CancellationToken, ExecRequest, Executor},
        host::HostCaps,
        plan::{PlanEngine, Policy},
        testing::{FakeDiskSpace, FakeRuntime, RecordingSink},
    };
    use std::{collections::BTreeMap, sync::Arc};
    let fixture = fixture(id);
    let node = engine().probe_path(&fixture.input()).unwrap().root;
    let mut host: HostCaps = toml::from_str(include_str!("hosts/linux.toml")).unwrap();
    if tool.is_none() {
        host.seven_zip = None;
    }
    let mut planner = PlanEngine::new();
    planner.register(Box::new(SfxHandler::new())).unwrap();
    let plans = planner
        .plan(std::slice::from_ref(&node), &host, &Policy::default())
        .unwrap();
    let candidate = &plans.candidates[plans.selected.unwrap() as usize];
    assert!(!candidate.requires.executes_code);
    assert!(candidate.requires.runtime.is_none());
    let sources = BTreeMap::from([(node.id.clone(), fixture.input())]);
    let mut extractor = SfxExtractor::new(sources.clone());
    if let Some(tool) = tool {
        extractor = extractor.with_sidecar(tool);
    }
    let extractor = Arc::new(extractor);
    let root = TestRoot::new().unwrap();
    let report = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor.clone())
    .execute(
        ExecRequest {
            job_id: "sfx".into(),
            root: root.path().into(),
            relative_dir: "Game".into(),
            plans,
            sources,
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    root.assert_sentinel_untouched();
    fixture.assert_tree(&report.dir).unwrap();
    assert!(
        extractor
            .take_artifacts(&node.id)
            .unwrap()
            .iter()
            .any(|n| n.format == Format::Pe)
    );
}

#[tokio::test]
async fn native_sfx_candidates_install_statically_through_executor() {
    install("zip-sfx", None).await;
    install("sevenzip-sfx", None).await;
}

#[tokio::test]
#[ignore = "requires CASKADE_7ZZ pointing to the pinned official sidecar"]
async fn genuine_handmade_rar_sfx_installs_and_reprobes() {
    let tool = archive::sevenzip::SevenZip::pinned(
        std::env::var_os("CASKADE_7ZZ").expect("set CASKADE_7ZZ"),
    )
    .unwrap();
    let fixture = fixture("rar-sfx");
    let payload = locate(
        &mut std::fs::File::open(fixture.input()).unwrap(),
        &ArchiveOptions::default(),
    )
    .unwrap();
    assert_eq!(payload.format, Format::Rar);
    install("rar-sfx", Some(tool)).await;
}
