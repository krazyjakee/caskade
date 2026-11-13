use crate::fixtures;
use caskade_installer::{
    exec::CancellationToken,
    formats::archive::{EventProgress, NoopControl, sevenzip::*},
    safety::{Limits, SafeDir, open_staging},
    testing::RecordingSink,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

fn fake(root: &Path, body: &str) -> PathBuf {
    let python = root.join("fake.py");
    std::fs::write(
        &python,
        format!("import sys,time,pathlib,subprocess\n{body}\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("7zz");
        std::fs::write(
            &path,
            "#!/bin/sh\nexec python3 \"$(dirname \"$0\")/fake.py\" \"$@\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(windows)]
    {
        let path = root.join("7zz.cmd");
        std::fs::write(&path, "@echo off\r\npython \"%~dp0fake.py\" %*\r\n").unwrap();
        path
    }
}
fn source(root: &Path) -> PathBuf {
    let p = root.join("input.bin");
    std::fs::write(&p, b"input").unwrap();
    p
}
fn writer(root: &Path) -> SafeDir {
    SafeDir::new(open_staging(root).unwrap(), Limits::default(), 1024, true)
}
const LIST: &str = "Path = file\nFolder = -\nSize = 7\nAttributes =  -rw-r--r--\n";

#[test]
fn technical_listing_is_validated_before_extraction() {
    let tool = SevenZip::new("unused").with_case_sensitivity(false);
    assert_eq!(tool.parse_listing(LIST.as_bytes(), 1024).unwrap().len(), 1);
    for name in [
        "../sentinel",
        "/sentinel",
        "C:\\sentinel",
        "\\\\server\\share",
        "NUL.txt",
        "foo.",
        "a\\..\\sentinel",
    ] {
        assert!(
            tool.parse_listing(
                LIST.replace("Path = file", &format!("Path = {name}"))
                    .as_bytes(),
                1024
            )
            .is_err(),
            "{name}"
        );
    }
    let collision = format!("{LIST}\n{}", LIST.replace("file", "FILE"));
    assert!(tool.parse_listing(collision.as_bytes(), 1024).is_err());
    assert!(matches!(
        tool.parse_listing(
            LIST.replace("Size = 7", "Size = 999999999999999")
                .as_bytes(),
            1024
        ),
        Err(SevenZipError::Safety(_))
    ));
    assert!(matches!(
        tool.parse_listing(format!("{LIST}Encrypted = +\n").as_bytes(), 1024),
        Err(SevenZipError::Encrypted)
    ));
    assert!(
        tool.parse_listing(b"Path = one\nPath = two\nSize = 1\n", 1024)
            .is_err()
    );
    let chain=b"Path = dir/sub\nFolder = -\nSize = 0\nSymbolic Link = ..\n\nPath = dir/link\nFolder = -\nSize = 0\nSymbolic Link = sub/../../sentinel\n";
    assert!(tool.parse_listing(chain, 1024).is_err());
}
#[test]
fn exit_codes_and_password_errors_are_explicit() {
    assert!(matches!(
        SevenZipError::exit(Some(1), "warning"),
        SevenZipError::Warning(_)
    ));
    assert!(matches!(
        SevenZipError::exit(Some(2), "broken data"),
        SevenZipError::Fatal(_)
    ));
    assert!(matches!(
        SevenZipError::exit(Some(7), "bad args"),
        SevenZipError::CommandLine(_)
    ));
    assert!(matches!(
        SevenZipError::exit(Some(8), ""),
        SevenZipError::OutOfMemory
    ));
    assert!(matches!(
        SevenZipError::exit(Some(255), ""),
        SevenZipError::Stopped
    ));
    assert!(matches!(
        SevenZipError::exit(Some(2), "ERROR: Wrong password?"),
        SevenZipError::Encrypted
    ));
    assert!(matches!(
        SevenZipError::exit(Some(3), "other"),
        SevenZipError::Exit { code: Some(3), .. }
    ));
}
#[test]
fn fake_script_reports_errors_and_times_out() {
    let root = tempfile::tempdir().unwrap();
    let input = source(root.path());
    let program = fake(
        root.path(),
        "sys.stderr.write('ERROR: Data Error\\n')\nsys.exit(2)",
    );
    assert!(
        matches!(SevenZip::new(program).list(&input,&mut NoopControl),Err(SevenZipError::Fatal(message))if message.contains("Data Error"))
    );
    let program = fake(root.path(), "time.sleep(30)");
    assert!(matches!(
        SevenZip::new(program)
            .with_timeout(Duration::from_millis(500))
            .list(&input, &mut NoopControl),
        Err(SevenZipError::TimedOut)
    ));
}
#[test]
fn fake_extraction_parses_progress_imports_safely_and_discards_scratch() {
    let root = tempfile::tempdir().unwrap();
    let input = source(root.path());
    let scratch = root.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    let output = root.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let body = format!(
        "if sys.argv[1]=='l':\n print({:?})\nelse:\n out=next(arg[2:] for arg in sys.argv if arg.startswith('-o'))\n pathlib.Path(out,'file').write_bytes(b'payload')\n print(' 12%\\r 100%\\r',end='',flush=True)",
        LIST
    );
    let program = fake(root.path(), &body);
    let seen = Arc::new(Mutex::new(vec![]));
    let capture = seen.clone();
    let tool = SevenZip::new(program).with_progress(Arc::new(move |percent| {
        capture.lock().unwrap().push(percent)
    }));
    let sink = RecordingSink::default();
    let token = CancellationToken::new();
    let mut control = EventProgress::new("sidecar", &token, &sink);
    tool.extract(&input, &scratch, &mut writer(&output), &mut control)
        .unwrap();
    assert_eq!(std::fs::read(output.join("file")).unwrap(), b"payload");
    assert_eq!(*seen.lock().unwrap(), vec![12, 100]);
    assert_eq!(std::fs::read_dir(scratch).unwrap().count(), 0);
    assert!(
        sink.events()
            .iter()
            .any(|event| event.bytes == 7 && event.files == 1)
    );
}
#[test]
fn cancellation_kills_fake_process_tree_and_leaves_sentinel_untouched() {
    let root = tempfile::tempdir().unwrap();
    let input = source(root.path());
    let scratch = root.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    let output = root.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let sentinel = root.path().join("sentinel");
    std::fs::write(&sentinel, b"unchanged").unwrap();
    let child = format!(
        "import time,pathlib;time.sleep(0.7);pathlib.Path({:?}).write_bytes(b'changed')",
        sentinel.to_str().unwrap()
    );
    let body = format!(
        "if sys.argv[1]=='l':\n print({:?})\nelse:\n subprocess.Popen([sys.executable,'-c',{:?}])\n print(' 12%\\r',end='',flush=True)\n time.sleep(30)",
        LIST, child
    );
    let program = fake(root.path(), &body);
    let token = CancellationToken::new();
    let cancel = token.clone();
    let tool = SevenZip::new(program).with_progress(Arc::new(move |_| cancel.cancel()));
    let sink = RecordingSink::default();
    let mut control = EventProgress::new("cancel-sidecar", &token, &sink);
    assert!(
        tool.extract(&input, &scratch, &mut writer(&output), &mut control)
            .is_err()
    );
    assert!(token.is_cancelled());
    std::thread::sleep(Duration::from_millis(1000));
    assert_eq!(std::fs::read(sentinel).unwrap(), b"unchanged");
    assert_eq!(std::fs::read_dir(scratch).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(output).unwrap().count(), 0);
}

fn pinned() -> SevenZip {
    let program = std::env::var_os("CASKADE_7ZZ")
        .map(PathBuf::from)
        .expect("set CASKADE_7ZZ to scripts/7zz-download.py output program");
    let tool = SevenZip::pinned(program).unwrap();
    let banner = tool.version_output().unwrap();
    assert!(
        banner.contains("26.05"),
        "must use pinned official26.05, not system p7zip: {banner}"
    );
    tool
}
#[test]
#[ignore = "native pinned official sidecar; prepare scripts/7zz-download.py then set CASKADE_7ZZ"]
fn pinned_official_rar_fixtures_match_all_expected_trees() {
    let tool = pinned();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let corpus = fixtures::load_all(&root).unwrap();
    let mut count = 0;
    for fixture in corpus
        .into_iter()
        .filter(|f| f.expected.format.starts_with("rar"))
    {
        let parent = tempfile::tempdir().unwrap();
        let staging = parent.path().join("staging");
        let scratch = parent.path().join("scratch");
        std::fs::create_dir(&staging).unwrap();
        std::fs::create_dir(&scratch).unwrap();
        let packed = fixture
            .parts()
            .iter()
            .map(|p| p.metadata().unwrap().len())
            .sum();
        let mut dir = SafeDir::new(
            open_staging(&staging).unwrap(),
            Limits::default(),
            packed,
            true,
        );
        let listed = tool.list(&fixture.input(), &mut NoopControl).unwrap();
        assert!(!listed.is_empty());
        tool.extract(&fixture.input(), &scratch, &mut dir, &mut NoopControl)
            .unwrap_or_else(|e| panic!("{}: {e}", fixture.expected.id));
        fixture.assert_tree(&staging).unwrap();
        assert_eq!(std::fs::read_dir(scratch).unwrap().count(), 0);
        count += 1;
    }
    assert_eq!(count, 3);
}
#[tokio::test]
#[ignore = "native pinned official sidecar; prepare scripts/7zz-download.py then set CASKADE_7ZZ"]
async fn pinned_official_sidecar_runs_through_executor() {
    use caskade_installer::{
        exec::{ExecRequest, Executor},
        host::HostCaps,
        plan::{PlanEngine, Policy},
        testing::{FakeDiskSpace, FakeRuntime},
    };
    let tool = pinned();
    let fixture = fixtures::Fixture::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        "unity-rar5",
    )
    .unwrap();
    let source = fixture.input();
    let node = caskade_installer::probe::ProbeEngine::default()
        .probe_path(&source)
        .unwrap()
        .root;
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("linux")).unwrap();
    host.seven_zip = Some(tool.program().into());
    let mut plans = PlanEngine::new();
    plans.register(Box::new(SevenZipHandler)).unwrap();
    let plans = plans
        .plan(std::slice::from_ref(&node), &host, &Policy::default())
        .unwrap();
    assert_eq!(plans.selected, Some(0));
    let sources = BTreeMap::from([(node.id.clone(), source)]);
    let extractor = Arc::new(SevenZipExtractor::new(tool, sources.clone()));
    let root = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let executor = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink,
    )
    .with_extractor(extractor.clone());
    let result = executor
        .execute(
            ExecRequest {
                job_id: "rar-test".into(),
                root: root.path().into(),
                relative_dir: "Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    fixture.assert_tree(&result.dir).unwrap();
}

#[test]
fn production_construction_rejects_an_unpinned_program_before_execution() {
    let root = tempfile::tempdir().unwrap();
    let file = source(root.path());
    assert!(matches!(
        SevenZip::pinned(&file),
        Err(SevenZipError::UnpinnedBinary)
    ));
}

#[tokio::test]
#[ignore = "native pinned sidecar fallback; prepare downloader and CASKADE_7ZZ"]
async fn unsupported_native_codec_falls_back_after_executor_rollback() {
    use caskade_installer::{
        exec::{ExecRequest, Executor},
        formats::archive::ArchiveHandler,
        host::HostCaps,
        plan::{PlanEngine, Policy},
        testing::{FakeDiskSpace, FakeRuntime},
    };
    let tool = pinned();
    let source_root = tempfile::tempdir().unwrap();
    let payload = b"#!/bin/sh\necho game\n".repeat(4096);
    std::fs::write(source_root.path().join("game.sh"), &payload).unwrap();
    let archive = source_root.path().join("game.zip");
    let status = std::process::Command::new(tool.program())
        .current_dir(source_root.path())
        .args(["a", "-tzip", "-mm=BZip2"])
        .arg(&archive)
        .arg("game.sh")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let node = caskade_installer::probe::ProbeEngine::default()
        .probe_path(&archive)
        .unwrap()
        .root;
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("linux")).unwrap();
    host.seven_zip = Some(tool.program().into());
    let mut planner = PlanEngine::new();
    planner.register(Box::new(ArchiveHandler)).unwrap();
    planner.register(Box::new(SevenZipHandler)).unwrap();
    let plans = planner
        .plan(std::slice::from_ref(&node), &host, &Policy::default())
        .unwrap();
    assert_eq!(plans.candidates.len(), 2);
    assert_eq!(plans.candidates[0].handler.0, "archive");
    let sources = BTreeMap::from([(node.id.clone(), archive)]);
    let extractor = Arc::new(HybridArchiveExtractor::new(tool, sources.clone()));
    let root = tempfile::tempdir().unwrap();
    let executor = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor.clone());
    let report = executor
        .execute(
            ExecRequest {
                job_id: "fallback".into(),
                root: root.path().into(),
                relative_dir: "Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.candidate.handler.0, "sevenzip");
    assert_eq!(report.failures.len(), 1);
    assert_eq!(std::fs::read(report.dir.join("game.sh")).unwrap(), payload);
}
