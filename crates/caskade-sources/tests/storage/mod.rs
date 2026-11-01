//! Managed storage restoration and reconcile over private authored roots.
mod guards;
mod reconcile;
mod remount;
mod restore;

use caskade_core::{
    bus::Bus,
    db::LibraryStore,
    model::{InstallLocation, ListGames, PlaySession, StorageRoot, new_id},
};
use caskade_installer::{
    LaunchTarget, RuntimeSpec,
    exec::CancellationToken,
    manifest::{
        InstallManifest, InstalledFile, ManifestPath, RelocationRecord, RelocationVerdict,
        RootManifest,
    },
    pipeline::INSTALL_ID,
    reconcile::{Reconciler, ScanLimits},
    relocate::{
        Relocator, RunningCheck,
        backends::{NoRegistry, NoShortcuts},
    },
    scan::{ScanInput, scan_references},
    testing::{FakeDiskSpace, RecordingSink},
};
use caskade_sources::storage::{StorageGameStatus, StorageSource};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

struct NotRunning;
impl RunningCheck for NotRunning {
    fn running(&self, _: &Path) -> bool {
        false
    }
}
fn reconciler() -> Reconciler {
    Reconciler::new(Arc::new(Relocator {
        disk: Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        sink: Arc::new(RecordingSink::default()),
        registry: Arc::new(NoRegistry),
        shortcuts: Arc::new(NoShortcuts),
        running: Arc::new(NotRunning),
    }))
}
async fn root(store: &LibraryStore, path: &Path, id: &str) -> StorageRoot {
    fs::create_dir_all(path).unwrap();
    let manifest = RootManifest::new(id).unwrap();
    manifest.write(path).unwrap();
    let root = StorageRoot::from_manifest(fs::canonicalize(path).unwrap(), &manifest);
    store.upsert_root(&root).await.unwrap();
    root
}
fn game(
    root: &StorageRoot,
    relative: &str,
    title: &str,
    id: Option<&str>,
    verdict: RelocationVerdict,
) -> PathBuf {
    let path = root.path.join(relative);
    fs::create_dir_all(path.join("bin")).unwrap();
    fs::write(path.join("bin/play"), b"synthetic game bytes").unwrap();
    let config = if verdict == RelocationVerdict::Rewritable {
        format!("[game]\npath={}\n", path.display())
    } else {
        "[game]\npath=relative\n".into()
    };
    fs::write(path.join("config.ini"), config).unwrap();
    let mut record = InstallManifest::from_json(include_bytes!(
        "../../../caskade-installer/tests/manifests/install.v1.posix.json"
    ))
    .unwrap()
    .into_record();
    record.title = title.into();
    record.ids.clear();
    if let Some(id) = id {
        record.ids.insert(INSTALL_ID.into(), id.into());
    }
    record.plan.root_id = root.id.clone();
    record.plan.relative_dir = relative.into();
    record.launch_targets = vec![LaunchTarget {
        path: "bin/play".into(),
        args: vec![],
        working_dir: ".".into(),
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        score: 1.0,
        reason: "Synthetic storage fixture".into(),
    }];
    record.runtime = RuntimeSpec::Native;
    record.alias = None;
    record.uninstall_steps.clear();
    record.external_effects.clear();
    record.files = ["bin/play", "config.ini"]
        .into_iter()
        .map(|name| InstalledFile {
            path: ManifestPath::new(name).unwrap(),
            size: fs::metadata(path.join(name)).unwrap().len(),
            sha256: None,
        })
        .collect();
    record.relocation = RelocationRecord {
        verdict,
        last_path: Some(path.to_string_lossy().into_owned()),
        references: vec![],
    };
    if verdict == RelocationVerdict::Rewritable {
        record.relocation.references = scan_references(&ScanInput {
            game_dir: &path,
            real_path: &path.to_string_lossy(),
            alias_path: None,
            real_variants: &[],
            registry: &[],
            external_files: &[],
            delegated: false,
            limits: Default::default(),
        })
        .unwrap()
        .references;
    }
    InstallManifest::new(record).unwrap().write(&path).unwrap();
    path
}
fn source(store: &LibraryStore) -> StorageSource {
    StorageSource::new(store.clone(), Bus::default(), Arc::new(reconciler()))
}
