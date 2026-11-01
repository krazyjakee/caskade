use caskade_installer::{
    Format,
    formats::archive::*,
    safety::{Limits, SafeDir, open_staging},
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
};

use crate::fixtures;

fn corpus() -> Vec<fixtures::Fixture> {
    fixtures::load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .unwrap()
        .into_iter()
        .filter(|f| !f.expected.format.starts_with("rar"))
        .collect()
}
fn writer(root: &Path, packed: u64) -> SafeDir {
    SafeDir::new(open_staging(root).unwrap(), Limits::default(), packed, true)
}

#[test]
fn every_archive_fixture_lists_extracts_matches_tree_and_reprobes() {
    for fixture in corpus() {
        let entries = list_path(&fixture.input(), &ArchiveOptions::default())
            .unwrap_or_else(|e| panic!("list {}: {e}", fixture.expected.id));
        assert!(!entries.is_empty());
        let temp = tempfile::tempdir().unwrap();
        let packed = fixture
            .parts()
            .iter()
            .map(|p| p.metadata().unwrap().len())
            .sum();
        let mut dir = writer(temp.path(), packed);
        let artifacts = extract_path(
            &fixture.input(),
            &mut dir,
            &ArchiveOptions::default(),
            &mut NoopControl,
        )
        .unwrap_or_else(|e| panic!("extract {}: {e}", fixture.expected.id));
        fixture
            .assert_tree(temp.path())
            .unwrap_or_else(|e| panic!("{}: {e}", fixture.expected.id));
        assert!(
            artifacts
                .iter()
                .any(|a| a.path == Path::new(&fixture.expected.launch_target)),
            "{}",
            fixture.expected.id
        );
        assert!(
            artifacts.iter().any(|a| matches!(
                a.format,
                Format::Pe | Format::Elf | Format::Script | Format::MachO
            )),
            "{}",
            fixture.expected.id
        );
    }
}

fn zip_bytes(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, mode) in files {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().unix_permissions(*mode),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
#[test]
fn prepended_zip_offsets_exec_restoration_and_selection() {
    let mut data = zip_bytes(&[
        ("bundle/Game", b"\x7fELFpayload", 0o644),
        ("bundle/run.sh", b"#!/bin/sh\n", 0o644),
        ("bundle/readme.txt", b"hello", 0o644),
    ]);
    let central: Vec<_> = data
        .windows(4)
        .enumerate()
        .filter_map(|(i, w)| (w == b"PK\x01\x02").then_some(i))
        .collect();
    for offset in central {
        data[offset + 5] = 0;
        data[offset + 38..offset + 42].fill(0);
    }
    let mut prefixed = b"arbitrary prepended launcher bytes".to_vec();
    prefixed.extend(&data);
    let temp = tempfile::tempdir().unwrap();
    let mut safe = writer(temp.path(), prefixed.len() as u64);
    let options = ArchiveOptions {
        select: vec!["bundle/Game".into(), "**/*.sh".into()],
        strip: Some("bundle".into()),
        ..ArchiveOptions::default()
    };
    let nodes = extract(
        Cursor::new(prefixed),
        Format::Zip,
        &mut safe,
        &options,
        &mut NoopControl,
    )
    .unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(temp.path().join("Game").exists());
    assert!(!temp.path().join("readme.txt").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(temp.path().join("Game"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }
}

#[test]
fn progress_is_streamed_and_cancellation_removes_partial_file() {
    use caskade_installer::{
        events::{EventSink, JobEvent},
        exec::CancellationToken,
    };
    struct CancelAfter {
        token: CancellationToken,
        events: std::sync::Mutex<Vec<JobEvent>>,
    }
    impl EventSink for CancelAfter {
        fn emit(&self, event: JobEvent) {
            let cancel = event.bytes >= 65536;
            self.events.lock().unwrap().push(event);
            if cancel {
                self.token.cancel();
            }
        }
    }
    let data = zip_bytes(&[("large", &vec![42; 1024 * 1024], 0o644)]);
    let token = CancellationToken::new();
    let sink = CancelAfter {
        token: token.clone(),
        events: std::sync::Mutex::new(vec![]),
    };
    let mut progress = EventProgress::new("archive-test", &token, &sink);
    let temp = tempfile::tempdir().unwrap();
    let mut safe = SafeDir::new(
        open_staging(temp.path()).unwrap(),
        Limits {
            max_expansion_ratio: 100_000,
            ..Limits::default()
        },
        data.len() as u64,
        true,
    );
    let result = extract(
        Cursor::new(data),
        Format::Zip,
        &mut safe,
        &ArchiveOptions::default(),
        &mut progress,
    );
    assert!(result.is_err());
    assert!(token.is_cancelled());
    assert!(!temp.path().join("large").exists());
    let events = sink.events.lock().unwrap();
    assert!(events.iter().any(|e| e.bytes > 0 && e.files == 0));
    assert!(events.last().unwrap().bytes < 1024 * 1024);
}

#[test]
fn metadata_limits_are_checked_before_archive_allocation() {
    let fixture = corpus()
        .into_iter()
        .find(|f| f.expected.id == "unity-7z")
        .unwrap();
    let options = ArchiveOptions {
        max_metadata_bytes: 8,
        ..ArchiveOptions::default()
    };
    assert!(matches!(
        list_path(&fixture.input(), &options),
        Err(ArchiveError::MemoryLimit)
    ));
    let data = zip_bytes(&[("file", b"data", 0o644)]);
    assert!(matches!(
        list(Cursor::new(data), Format::Zip, &options),
        Err(ArchiveError::MemoryLimit)
    ));
}

#[tokio::test]
async fn archive_extractor_runs_through_executor_and_retains_reprobed_artifacts() {
    use caskade_installer::{
        exec::{ExecRequest, Executor},
        host::HostCaps,
        plan::{PlanEngine, Policy},
        testing::{FakeDiskSpace, RecordingSink},
    };
    let fixture = corpus()
        .into_iter()
        .find(|f| f.expected.id == "unity-zip")
        .unwrap();
    let source = fixture.input();
    let mut probe = caskade_installer::probe::ProbeEngine::default();
    probe.register(Box::new(ArchiveProbe::new()));
    let node = probe.probe_path(&source).unwrap().root;
    let mut plans = PlanEngine::new();
    plans.register(Box::new(ArchiveHandler::new())).unwrap();
    let plans = plans
        .plan(
            std::slice::from_ref(&node),
            &toml::from_str::<HostCaps>(include_str!("hosts/linux.toml")).unwrap(),
            &Policy::default(),
        )
        .unwrap();
    assert_eq!(plans.selected, Some(0));
    let sources = BTreeMap::from([(node.id.clone(), source)]);
    let extractor = Arc::new(ArchiveExtractor::new(sources.clone()));
    let root = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let executor = Executor::new(
        Arc::new(caskade_installer::testing::FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink.clone(),
    )
    .with_extractor(extractor.clone());
    let request = ExecRequest {
        job_id: "archive-test".into(),
        root: root.path().into(),
        relative_dir: "Game".into(),
        plans,
        sources,
    };
    let report = executor
        .execute(request, caskade_installer::exec::CancellationToken::new())
        .await
        .unwrap();
    fixture.assert_tree(&report.dir).unwrap();
    let artifacts = extractor.take_artifacts(&node.id).unwrap();
    assert!(artifacts.iter().any(|a| a.format == Format::Pe));
    assert!(
        sink.events()
            .iter()
            .any(|event| event.bytes > 0 && event.message == "progress")
    );
}
