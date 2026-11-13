//! Finding a root again after its drive is mounted somewhere else.
use super::*;

#[tokio::test]
async fn automatic_remount_uses_volume_relative_hint_and_physical_identity_after_db_loss() {
    use caskade_core::{
        AppPaths,
        launch::LaunchService,
        settings::SettingsService,
        storage::{RootCapabilities, RootProbe, StorageService},
    };
    use caskade_discovery::{
        HostEnv,
        env::{FixedDrives, Volume, VolumeKind},
    };
    struct Writable;
    impl RootProbe for Writable {
        fn probe(&self, _: &Path) -> std::io::Result<RootCapabilities> {
            Ok(RootCapabilities {
                writable: true,
                executable: true,
                symlinks: true,
                case_sensitive: true,
                ..Default::default()
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("app"));
    let machine = temp.path().join("machine");
    fs::create_dir_all(machine.join("root/mnt/old-drive")).unwrap();
    fs::write(
        machine.join("machine.toml"),
        "schema_version = 1\nos = \"linux\"\n",
    )
    .unwrap();
    let old_volume = fs::canonicalize(machine.join("root/mnt/old-drive")).unwrap();
    let volume = |root: PathBuf| Volume {
        root,
        kind: VolumeKind::Removable,
        case_sensitive: true,
        filesystem: Some("fixturefs".into()),
    };
    let env = HostEnv::fixture(&machine)
        .unwrap()
        .with_drives(Arc::new(FixedDrives::new(vec![volume(old_volume.clone())])));
    let store = LibraryStore::open_default(&paths).await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let registry = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths.clone())
        .with_probe(Arc::new(Writable));
    let root = registry
        .register_root(old_volume.join("Games"), Some("Removable games".into()))
        .await
        .unwrap()
        .root;
    let id = new_id();
    game(
        &root,
        "Game",
        "Remounted game",
        Some(&id),
        RelocationVerdict::Free,
    );
    let managed = source(&store).with_remounts(registry.clone(), env, &paths);
    assert_eq!(
        managed
            .refresh(CancellationToken::new())
            .await
            .unwrap()
            .imported(),
        1
    );
    let hints = fs::read_to_string(paths.config.join("storage-volume-hints.json")).unwrap();
    assert!(hints.contains("Games"));
    assert!(!hints.contains(old_volume.to_str().unwrap()));
    drop(managed);
    drop(registry);
    store.close().await;
    fs::remove_file(paths.database()).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(PathBuf::from(format!(
            "{}{suffix}",
            paths.database().display()
        )));
    }
    let new_volume = machine.join("root/mnt/new-drive");
    fs::rename(&old_volume, &new_volume).unwrap();
    // Another mounted volume has the same folder name, but another root ID.
    let other = machine.join("root/mnt/unrelated");
    fs::create_dir_all(other.join("Games")).unwrap();
    RootManifest::new("unrelated-root")
        .unwrap()
        .write(&other.join("Games"))
        .unwrap();
    let env = HostEnv::fixture(&machine)
        .unwrap()
        .with_drives(Arc::new(FixedDrives::new(vec![
            volume(new_volume.clone()),
            volume(other),
        ])));
    let store = LibraryStore::open_default(&paths).await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let registry = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths.clone())
        .with_probe(Arc::new(Writable));
    registry.restore_roots().await.unwrap();
    assert_eq!(
        store.get_root(&root.id).await.unwrap().unwrap().path,
        root.path
    );
    let managed = source(&store).with_remounts(registry, env, &paths);
    let report = managed.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(report.imported(), 1, "{report:?}");
    assert_eq!(
        store.get_root(&root.id).await.unwrap().unwrap().path,
        new_volume.join("Games")
    );
    let entry = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        entry.location,
        Some(InstallLocation::Managed {
            root_id: root.id,
            rel_dir: "Game".into()
        })
    );
}

#[tokio::test]
async fn ambiguous_or_traversing_remount_hints_never_choose_a_copy_or_rewrite_it() {
    use caskade_core::{
        AppPaths,
        launch::LaunchService,
        settings::SettingsService,
        storage::{RootCapabilities, RootProbe, StorageService},
    };
    use caskade_discovery::{
        HostEnv,
        env::{FixedDrives, Volume, VolumeKind},
    };
    struct Writable;
    impl RootProbe for Writable {
        fn probe(&self, _: &Path) -> std::io::Result<RootCapabilities> {
            Ok(RootCapabilities {
                writable: true,
                executable: true,
                symlinks: true,
                case_sensitive: true,
                ..Default::default()
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(&temp.path().join("app"));
    let machine = temp.path().join("machine");
    fs::create_dir_all(machine.join("root/mnt/old")).unwrap();
    fs::write(
        machine.join("machine.toml"),
        "schema_version = 1\nos = \"linux\"\n",
    )
    .unwrap();
    let volume = |path: PathBuf| Volume {
        root: path,
        kind: VolumeKind::Removable,
        case_sensitive: true,
        filesystem: None,
    };
    let old = fs::canonicalize(machine.join("root/mnt/old")).unwrap();
    let env = HostEnv::fixture(&machine)
        .unwrap()
        .with_drives(Arc::new(FixedDrives::new(vec![volume(old.clone())])));
    let store = LibraryStore::in_memory().await.unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let registry = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths.clone())
        .with_probe(Arc::new(Writable));
    let root = registry
        .register_root(old.join("Games"), None)
        .await
        .unwrap()
        .root;
    let id = new_id();
    let original = game(
        &root,
        "Game",
        "Ambiguous",
        Some(&id),
        RelocationVerdict::Rewritable,
    );
    let managed = source(&store).with_remounts(registry.clone(), env, &paths);
    managed.refresh(CancellationToken::new()).await.unwrap();
    drop(managed);
    let catalogue = fs::read(paths.config.join("storage-roots.json")).unwrap();
    let old_entry = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    let before = fs::read(original.join("config.ini")).unwrap();
    let first = machine.join("root/mnt/first");
    fs::rename(&old, &first).unwrap();
    let second = machine.join("root/mnt/second");
    fs::create_dir_all(second.join("Games")).unwrap();
    RootManifest::new(&root.id)
        .unwrap()
        .write(&second.join("Games"))
        .unwrap();
    let env = HostEnv::fixture(&machine)
        .unwrap()
        .with_drives(Arc::new(FixedDrives::new(vec![
            volume(first.clone()),
            volume(second),
        ])));
    let managed = source(&store).with_remounts(registry, env, &paths);
    let report = managed.refresh(CancellationToken::new()).await.unwrap();
    assert!(
        report.roots[0]
            .error
            .as_deref()
            .unwrap()
            .contains("several mounted volumes")
    );
    assert_eq!(
        store.get_root(&root.id).await.unwrap().unwrap().path,
        root.path
    );
    assert_eq!(
        store.get_entry(&old_entry.id).await.unwrap(),
        Some(old_entry)
    );
    assert_eq!(
        fs::read(first.join("Games/Game/config.ini")).unwrap(),
        before
    );
    assert_eq!(
        fs::read(paths.config.join("storage-roots.json")).unwrap(),
        catalogue
    );
    let hints = paths.config.join("storage-volume-hints.json");
    fs::write(
        &hints,
        serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"roots":{(root.id.clone()):"../escape"}}),
        )
        .unwrap(),
    )
    .unwrap();
    let report = managed.refresh(CancellationToken::new()).await.unwrap();
    assert!(report.roots[0].error.is_some());
    assert_eq!(
        fs::read(first.join("Games/Game/config.ini")).unwrap(),
        before
    );
    assert_eq!(
        store.get_root(&root.id).await.unwrap().unwrap().path,
        root.path
    );
}
