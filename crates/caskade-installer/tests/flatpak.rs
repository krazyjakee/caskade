#![cfg(feature = "linux-formats")]
use caskade_installer::formats::linux::flatpak::{
    FlatpakHandler, FlatpakInstaller, FlatpakProbe, inspect_bundle, inspect_ref,
    metadata_from_artifact,
};
use caskade_installer::{ArtifactNode, Format, HandOff, Input, InstallOutcome, PlanSet, Strategy};
use caskade_installer::{
    exec::{CancellationToken, ExecRequest, Executor},
    host::HostCaps,
    manifest::{ManifestSource, RelocationVerdict, UninstallRequest, Uninstaller},
    plan::{PlanEngine, Policy},
    probe::{ProbeEngine, ProbeLimits},
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
fn source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/flatpak/flatpak-bundle.flatpak")
}
fn probe() -> ArtifactNode {
    let mut engine = ProbeEngine::default();
    engine.register(Box::new(FlatpakProbe));
    engine.probe_path(&source()).unwrap().root
}
fn host(name: &str) -> HostCaps {
    let mut host: HostCaps = toml::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/hosts")
                .join(format!("{name}.toml")),
        )
        .unwrap(),
    )
    .unwrap();
    host.packages
        .managers
        .insert("flatpak".into(), "/usr/bin/flatpak".into());
    host
}
fn plans(node: &ArtifactNode, host: &HostCaps) -> PlanSet {
    let mut engine = PlanEngine::new();
    engine.register(Box::new(FlatpakHandler)).unwrap();
    engine
        .plan(std::slice::from_ref(node), host, &Policy::default())
        .unwrap()
}
fn executor(runtime: Arc<dyn caskade_installer::runtime::Runtime>) -> Executor {
    Executor::new(
        runtime,
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
}
fn request(root: &TestRoot, node: &ArtifactNode) -> ExecRequest {
    ExecRequest {
        job_id: "flatpak-test".into(),
        root: root.path().into(),
        relative_dir: "Flatpak Game".into(),
        plans: plans(node, &host("linux")),
        sources: BTreeMap::from([(node.id.clone(), source())]),
    }
}
#[test]
fn genuine_bundle_and_flatpakref_have_static_identity_and_host_gated_delegation() {
    let node = probe();
    assert_eq!(node.format, Format::Flatpak);
    let metadata = metadata_from_artifact(&node).unwrap();
    assert_eq!(metadata.id, "io.caskade.FixtureGame");
    assert_eq!(metadata.branch, "1");
    assert_eq!(metadata.arch, "x86_64");
    for profile in ["linux", "linux-no-umu", "linux-exfat-root"] {
        let plans = plans(&node, &host(profile));
        assert!(plans.question.is_none());
        assert_eq!(plans.candidates.len(), 1);
        assert_eq!(plans.candidates[0].strategy, Strategy::Delegate);
        assert!(!plans.candidates[0].requires.may_prompt_elevation);
    }
    for profile in ["windows", "macos", "macos-no-wine"] {
        assert!(plans(&node, &host(profile)).candidates.is_empty());
    }
    let mut missing = host("linux");
    missing.packages.managers.remove("flatpak");
    assert!(plans(&node, &missing).candidates.is_empty());
    let reference=inspect_ref(b"[Flatpak Ref]\nName=io.caskade.FixtureGame\nBranch=stable\nUrl=https://example.test/repo\nTitle=Fixture\nIsRuntime=false\n").unwrap();
    assert_eq!(reference.id, metadata.id);
    assert!(!reference.bundle);
    assert_eq!(
        reference.launch_spec().args,
        ["run", "--user", "--branch=stable", "io.caskade.FixtureGame"]
    );
}
#[test]
fn malformed_refs_and_bundle_framing_never_become_unchecked_commands() {
    for input in [
        "Name=--command=sh\nUrl=https://example.test",
        "Name=io.caskade.Game\nName=io.caskade.Other\nUrl=https://example.test",
        "Name=io.caskade.Game\nUrl=file:///etc/passwd",
        "Name=io.caskade.Game\nUrl=https://user:secret@example.test",
        "Name=io.caskade.Game\nUrl=https://example.test\nBranch=../other",
    ] {
        assert!(inspect_ref(format!("[Flatpak Ref]\n{input}").as_bytes()).is_err());
    }
    let original = fs::read(source()).unwrap();
    for end in [0, 1, 12, 100, original.len() / 2] {
        assert!(inspect_bundle(&mut io::Cursor::new(&original[..end])).is_err());
    }
    let mut corrupt = original;
    let len = corrupt.len();
    corrupt[len - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(inspect_bundle(&mut io::Cursor::new(corrupt)).is_err());
}
struct Count<R> {
    inner: R,
    bytes: u64,
}
impl<R: Read> Read for Count<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.bytes += read as u64;
        Ok(read)
    }
}
impl<R: Seek> Seek for Count<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.inner.seek(from)
    }
}
#[test]
fn sparse_twenty_gigabyte_bundle_probe_reads_framing_and_identity_without_payload() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sparse.flatpak");
    let bytes = fs::read(source()).unwrap();
    let metadata_end = u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().unwrap()) as u64;
    let mut file = fs::File::create(&path).unwrap();
    // NTFS zero-fills up to a write past the end of a file that isn't sparse:
    // 20 GiB of writes for the trailer below.
    #[cfg(windows)]
    assert!(
        std::process::Command::new("fsutil")
            .args(["sparse", "setflag"])
            .arg(&path)
            .status()
            .unwrap()
            .success(),
        "mark the bundle sparse"
    );
    file.write_all(&bytes).unwrap();
    let size = 20 * 1024 * 1024 * 1024u64;
    file.set_len(size).unwrap();
    file.seek(SeekFrom::End(-8)).unwrap();
    file.write_all(&metadata_end.to_le_bytes()).unwrap();
    drop(file);
    let mut reader = Count {
        inner: fs::File::open(path).unwrap(),
        bytes: 0,
    };
    let metadata = inspect_bundle(&mut reader).unwrap();
    assert_eq!(metadata.id, "io.caskade.FixtureGame");
    assert!(reader.bytes < 2 * 1024 * 1024, "{}", reader.bytes);
    let mut engine = ProbeEngine::new(ProbeLimits {
        max_read_bytes: 2 * 1024 * 1024,
        ..Default::default()
    });
    engine.register(Box::new(FlatpakProbe));
    engine
        .probe_reader(&mut reader, Path::new("sparse.flatpak"))
        .unwrap();
}
#[tokio::test]
async fn fake_delegate_records_pinned_manifest_handoff_and_uninstall_compensation() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([
        FakeScript::default(),
        FakeScript::default(),
        FakeScript::default(),
    ]));
    let worker = executor(runtime.clone());
    let node = probe();
    let report = FlatpakInstaller::new(worker.clone())
        .install(
            request(&root, &node),
            node,
            ManifestSource::from_input(&Input::Path(source()), None, None).unwrap(),
            "fixture-root".into(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        report.manifest.record().relocation.verdict,
        RelocationVerdict::Pinned
    );
    assert_eq!(
        report.manifest.record().ids["flatpak"],
        "io.caskade.FixtureGame"
    );
    assert!(
        report
            .manifest
            .verify(&report.execution.dir)
            .unwrap()
            .is_ok()
    );
    let InstallOutcome::HandOff(HandOff::Delegated {
        launch_uri,
        uninstall_steps,
    }) = report.outcome
    else {
        panic!("delegate handoff")
    };
    assert_eq!(launch_uri, "flatpak:io.caskade.FixtureGame");
    assert_eq!(uninstall_steps, report.manifest.record().uninstall_steps);
    Uninstaller::new(worker)
        .uninstall(
            UninstallRequest {
                job_id: "flatpak-uninstall".into(),
                root: root.path().into(),
                relative_dir: report.execution.relative_dir,
                manifest: report.manifest,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    root.assert_clean();
    let processes = runtime.processes();
    assert_eq!(processes.len(), 3);
    assert_eq!(
        &processes[0].command.args[..4],
        ["install", "--user", "--noninteractive", "--"]
    );
    // Asked first whether the app is still installed.
    assert_eq!(
        processes[1].command.args,
        [
            "info",
            "--user",
            "--",
            "app/io.caskade.FixtureGame/x86_64/1"
        ]
    );
    assert_eq!(
        &processes[2].command.args[..4],
        ["uninstall", "--user", "--noninteractive", "--"]
    );
    assert_eq!(
        processes[2].command.args.last().unwrap(),
        "app/io.caskade.FixtureGame/x86_64/1"
    );
}
#[tokio::test]
async fn failed_delegate_never_leaves_a_managed_record() {
    let root = TestRoot::new().unwrap();
    let runtime = Arc::new(FakeRuntime::new([
        FakeScript {
            actions: vec![FakeAction::Exit { code: 1 }],
        },
        FakeScript::default(),
    ]));
    let node = probe();
    let result = FlatpakInstaller::new(executor(runtime))
        .install(
            request(&root, &node),
            node,
            ManifestSource::from_input(&Input::Path(source()), None, None).unwrap(),
            "fixture-root".into(),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    root.assert_clean();
}

#[cfg(target_os = "linux")]
#[path = "flatpak/native.rs"]
mod native;
