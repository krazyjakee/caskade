//! CLI routing proof with a simulated mount containing the committed app ZIP.
//! Generated UDIF metadata is not a native disk-image fixture.
use super::*;
use crate::{
    Format, OperatingSystem,
    exec::{CancellationToken, ExecRequest},
    formats::archive::{self, ArchiveOptions, NoopControl},
    probe::{ProbeEngine, ProbeLimits},
    runtime::{ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError},
    safety::{Limits, SafeDir, open_staging},
    testing::{FakeAction, FakeDiskSpace, FakeRuntime, FakeScript, RecordingSink, TestRoot},
};
use std::time::Duration;

struct MountRuntime {
    fake: FakeRuntime,
    with_app: bool,
}
impl Runtime for MountRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        self.fake.spawn(command)
    }
    fn spawn_with_input(
        &self,
        command: &ProcessCommand,
        input: &[u8],
    ) -> Result<ProcessId, RuntimeError> {
        if command.args.first().is_some_and(|arg| arg == "attach") && self.with_app {
            let at = command
                .args
                .iter()
                .position(|arg| arg == "-mountpoint")
                .unwrap();
            let mount = Path::new(&command.args[at + 1]);
            let mut safe = SafeDir::new(
                open_staging(mount).unwrap(),
                Limits::default(),
                1 << 20,
                true,
            );
            archive::extract_path(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/macos-app.zip"),
                &mut safe,
                &ArchiveOptions::default(),
                &mut NoopControl,
            )
            .map_err(|error| RuntimeError::Io {
                message: error.to_string(),
            })?;
        }
        self.fake.spawn_with_input(command, input)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.fake.wait(id, timeout)
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        self.fake.kill(id)
    }
}
fn metadata_bytes() -> Vec<u8> {
    let xml = b"<plist><dict/></plist>";
    let mut bytes = vec![0; 100 + xml.len() + 512];
    bytes[100..100 + xml.len()].copy_from_slice(xml);
    let trailer = &mut bytes[100 + xml.len()..];
    trailer[..4].copy_from_slice(b"koly");
    trailer[4..8].copy_from_slice(&4_u32.to_be_bytes());
    trailer[8..12].copy_from_slice(&512_u32.to_be_bytes());
    trailer[32..40].copy_from_slice(&100_u64.to_be_bytes());
    trailer[216..224].copy_from_slice(&100_u64.to_be_bytes());
    trailer[224..232].copy_from_slice(&(xml.len() as u64).to_be_bytes());
    bytes
}

#[tokio::test]
async fn cli_dmg_route_copies_discovers_records_and_always_detaches() {
    for with_app in [true, false] {
        let source = tempfile::tempdir().unwrap();
        let image = source.path().join("metadata-only.dmg");
        fs::write(&image, metadata_bytes()).unwrap();
        let artifact = ProbeEngine::with_builtin_formats(ProbeLimits::default())
            .probe_path(&image)
            .unwrap()
            .root;
        assert_eq!(artifact.format, Format::Dmg);
        let mut host = HostCaps::detect();
        host.os = OperatingSystem::Macos;
        host.arch = "x86_64".into();
        host.filesystem.writable = true;
        host.packages
            .managers
            .insert("hdiutil".into(), "/usr/bin/hdiutil".into());
        let plans = input::planner()
            .unwrap()
            .plan(std::slice::from_ref(&artifact), &host, &Policy::default())
            .unwrap();
        assert!(
            !plans.candidates.is_empty(),
            "artifact={artifact:?} host={host:?}"
        );
        assert_eq!(plans.candidates[0].handler.0, "macos-dmg");
        let runtime = Arc::new(MountRuntime {
            fake: FakeRuntime::new((0..2).map(|_| FakeScript {
                actions: vec![FakeAction::Exit { code: 0 }],
            })),
            with_app,
        });
        let sink = Arc::new(RecordingSink::default());
        let context = Context {
            sink: sink.clone(),
            cancel: CancellationToken::new(),
            tool: None,
            aliases: None,
            control: Default::default(),
            folder_template: None,
            config: Default::default(),
            host: None,
            handlers: Vec::new(),
            #[cfg(feature = "fetch")]
            resolvers: Vec::new(),
            #[cfg(feature = "torrent")]
            torrent: Default::default(),
            review: None,
            install_id: None,
            hints: Default::default(),
            umu: None,
        };
        let sources = BTreeMap::from([(artifact.id.clone(), image.clone())]);
        let extractor = Arc::new(PipelineExtractor::new(
            &context,
            sources.clone(),
            host.clone(),
            runtime.clone(),
        ));
        let source_record = ManifestSource::from_input(&Input::Path(image), None, None).unwrap();
        let finalizer = Arc::new(RecordInstall {
            compatibility_runner: CompatibilityRunner::default(),
            extractor: extractor.clone(),
            host,
            hints: DiscoveryHints::default(),
            artifacts: vec![artifact.clone()],
            source: source_record,
            root_id: "cli-dmg-test".into(),
            external_roots: Default::default(),
            discovery: Mutex::new(None),
            control: Default::default(),
            install_id: None,
            package: Mutex::new(None),
        });
        let root = TestRoot::new().unwrap();
        let result = Executor::new(
            runtime.clone(),
            Arc::new(FakeDiskSpace::fixed(u64::MAX)),
            sink,
        )
        .with_extractor(extractor.clone())
        .with_finalizer(finalizer)
        .execute(
            ExecRequest {
                root: root.path().into(),
                job_id: "cli-dmg".into(),
                relative_dir: "Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await;
        let processes = runtime.fake.processes();
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].command.args[0], "attach");
        assert_eq!(processes[0].input, b"qy\n");
        assert!(
            processes[0]
                .command
                .args
                .iter()
                .any(|arg| arg == "-readonly")
        );
        assert_eq!(processes[1].command.args[0], "detach");
        let mount = &processes[1].command.args[1];
        assert!(!Path::new(mount).exists());
        if with_app {
            let report = result.unwrap();
            let manifest = InstallManifest::read(&report.dir).unwrap();
            assert_eq!(manifest.record().plan.candidate.handler.0, "macos-dmg");
            assert!(!manifest.record().launch_targets.is_empty());
            assert!(!manifest.record().files.is_empty());
            assert!(manifest.verify(&report.dir).unwrap().changed.is_empty());
            assert_eq!(
                extractor.dmg.take_artifacts(&artifact.id).unwrap()[0].format,
                Format::AppBundle
            );
        } else {
            assert!(result.is_err());
            root.assert_clean();
        }
        root.assert_sentinel_untouched();
    }
}
