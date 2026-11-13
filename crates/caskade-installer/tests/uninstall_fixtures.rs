use caskade_installer::{
    Input, InstallPlan, RuntimeSpec,
    exec::{CancellationToken, ExecRequest, Executor, Extractor},
    formats::archive::{
        ArchiveExtractor, ArchiveHandler, ArchiveProbe,
        sevenzip::{SevenZip, SevenZipExtractor, SevenZipHandler},
    },
    host::HostCaps,
    manifest::{
        HandlerVersion, InstallManifest, InstalledFile, ManifestPath, ManifestSource, Sha256Digest,
        UninstallRequest, Uninstaller,
    },
    plan::{PlanEngine, Policy},
    probe::ProbeEngine,
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink, TestRoot},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::fixtures;

async fn install_and_uninstall(fixture: fixtures::Fixture, sidecar: Option<&SevenZip>) {
    let root = TestRoot::new().unwrap();
    let source = fixture.input();
    let mut probe = ProbeEngine::default();
    probe.register(Box::new(ArchiveProbe));
    let artifact = probe.probe_path(&source).unwrap().root;
    let sources = BTreeMap::from([(artifact.id.clone(), source.clone())]);
    let mut planner = PlanEngine::new();
    let mut host: HostCaps = caskade_installer::pipeline::input::host(Some("linux")).unwrap();
    let extractor: Arc<dyn Extractor> = if let Some(tool) = sidecar {
        host.seven_zip = Some(tool.program().into());
        planner.register(Box::new(SevenZipHandler)).unwrap();
        Arc::new(SevenZipExtractor::new(tool.clone(), sources.clone()))
    } else {
        planner.register(Box::new(ArchiveHandler)).unwrap();
        Arc::new(ArchiveExtractor::new(sources.clone()))
    };
    let plans = planner
        .plan(std::slice::from_ref(&artifact), &host, &Policy::default())
        .unwrap();
    assert_eq!(plans.selected, Some(0), "{}", fixture.expected.id);
    let worker = Executor::new(
        Arc::new(FakeRuntime::default()),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor);
    let report = worker
        .execute(
            ExecRequest {
                job_id: format!("install-{}", fixture.expected.id),
                root: root.path().into(),
                relative_dir: "Fixture Game".into(),
                plans,
                sources,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|error| panic!("{} install: {error}", fixture.expected.id));
    fixture
        .assert_tree(&report.dir)
        .unwrap_or_else(|error| panic!("{} extracted tree: {error}", fixture.expected.id));
    let mut record = InstallManifest::from_json(include_bytes!("manifests/install.v1.posix.json"))
        .unwrap()
        .into_record();
    record.source = ManifestSource::from_input(&Input::Path(source.clone()), None, None).unwrap();
    record.plan = InstallPlan {
        schema_version: 1,
        input: Input::Path(source),
        artifacts: vec![artifact],
        root_id: "fixture-root".into(),
        relative_dir: report.relative_dir.clone(),
        candidate: report.candidate.clone(),
    };
    record.handlers = vec![HandlerVersion {
        id: report.candidate.handler,
        version: env!("CARGO_PKG_VERSION").into(),
    }];
    record.files = fixture
        .tree
        .iter()
        .filter(|entry| entry.kind == fixtures::EntryKind::File)
        .map(|entry| InstalledFile {
            path: ManifestPath::new(&entry.path).unwrap(),
            size: entry.size,
            sha256: entry
                .sha256
                .as_ref()
                .map(|hash| Sha256Digest::new(hash).unwrap()),
        })
        .collect();
    record.launch_targets[0].path = fixture.expected.launch_target.clone().into();
    record.launch_targets[0].working_dir = ".".into();
    record.runtime = if fixture.expected.platform == "windows" {
        RuntimeSpec::ProtonPrefix {
            id: "fixture".into(),
        }
    } else {
        RuntimeSpec::Native
    };
    record.launch_targets[0].runtime = record.runtime.clone();
    record.uninstall_steps.clear();
    record.external_effects.clear();
    let manifest = InstallManifest::new(record).unwrap();
    manifest.write(&report.dir).unwrap();
    assert!(manifest.verify(&report.dir).unwrap().is_ok());
    let manifest = InstallManifest::read(&report.dir).unwrap();
    let removed = Uninstaller::new(worker)
        .uninstall(
            UninstallRequest {
                job_id: format!("uninstall-{}", fixture.expected.id),
                root: root.path().into(),
                relative_dir: report.relative_dir,
                manifest,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|error| panic!("{} uninstall: {error}", fixture.expected.id));
    assert!(removed.is_clean(), "{}", fixture.expected.id);
    root.assert_clean();
}

#[tokio::test]
async fn every_pure_rust_i0_fixture_installs_then_uninstalls_cleanly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for fixture in fixtures::load_all(&root)
        .unwrap()
        .into_iter()
        .filter(|fixture| !fixture.expected.format.starts_with("rar"))
    {
        install_and_uninstall(fixture, None).await;
        count += 1;
    }
    assert_eq!(count, 12);
}

#[tokio::test]
#[ignore = "native official pinned sidecar; prepare scripts/7zz-download.py and set CASKADE_7ZZ"]
async fn every_rar_i0_fixture_installs_then_uninstalls_cleanly() {
    let program = std::env::var_os("CASKADE_7ZZ")
        .map(PathBuf::from)
        .expect("CASKADE_7ZZ must point to the pinned official sidecar");
    let tool = SevenZip::pinned(program).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for fixture in fixtures::load_all(&root)
        .unwrap()
        .into_iter()
        .filter(|fixture| fixture.expected.format.starts_with("rar"))
    {
        install_and_uninstall(fixture, Some(&tool)).await;
        count += 1;
    }
    assert_eq!(count, 3);
}
