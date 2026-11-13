//! Moves, renames and copies reconcile before the library maps them.
use super::*;

#[tokio::test]
async fn explorer_rename_reconciles_before_import_and_preserves_entry_and_session() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-move").await;
    let id = new_id();
    let old = game(
        &root,
        "Before",
        "Moving game",
        Some(&id),
        RelocationVerdict::Rewritable,
    );
    let source = source(&store);
    let initial = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(initial.imported(), 1, "{initial:?}");
    let entry = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    let mut session = PlaySession::new(&entry.game_id, &entry.id, 1000);
    session.ended_at_ms = Some(2000);
    session.duration_ms = 1000;
    store.upsert_session(&session).await.unwrap();
    let new = root.path.join("After");
    fs::rename(&old, &new).unwrap();
    let result = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        result.games[0].status,
        StorageGameStatus::Reconciled,
        "{result:?}"
    );
    let updated = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.id, entry.id);
    assert_eq!(updated.game_id, entry.game_id);
    assert_eq!(
        updated.location,
        Some(InstallLocation::Managed {
            root_id: root.id,
            rel_dir: "After".into()
        })
    );
    assert_eq!(store.get_session(&session.id).await.unwrap(), Some(session));
    let config = fs::read_to_string(new.join("config.ini")).unwrap();
    assert!(config.contains(new.to_str().unwrap()));
    assert!(!config.contains(old.to_str().unwrap()));
    let manifest = InstallManifest::read(&new).unwrap();
    assert_eq!(manifest.record().ids[INSTALL_ID], id);
    assert_eq!(manifest.record().plan.relative_dir, Path::new("After"));
    assert!(manifest.verify(&new).unwrap().is_ok());
}

#[tokio::test]
async fn known_root_at_new_mount_path_updates_only_root_and_relative_manifest_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let mut root = root(&store, &temp.path().join("old-mount"), "root-stable").await;
    let id = new_id();
    game(
        &root,
        "Game",
        "Root move",
        Some(&id),
        RelocationVerdict::Free,
    );
    let source = source(&store);
    source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    let before = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    let new = temp.path().join("new-mount");
    fs::rename(&root.path, &new).unwrap();
    root.path = fs::canonicalize(&new).unwrap();
    assert_eq!(RootManifest::read(&new).unwrap().root_id(), root.id);
    store.upsert_root(&root).await.unwrap(); // registry adopts a known physical ID
    let report = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.imported(), 1, "{report:?}");
    assert_eq!(
        store.entry_by_source("caskade-storage", &id).await.unwrap(),
        Some(before)
    );
    assert_eq!(
        InstallManifest::read(&new.join("Game"))
            .unwrap()
            .record()
            .relocation
            .last_path
            .as_deref(),
        new.join("Game").to_str()
    );
}

#[tokio::test]
async fn duplicate_installation_ids_require_a_choice_instead_of_retargeting_entries() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-copy").await;
    let id = new_id();
    game(
        &root,
        "Original",
        "Original",
        Some(&id),
        RelocationVerdict::Free,
    );
    let source = source(&store);
    source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    let original = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    game(&root, "Copy", "Copy", Some(&id), RelocationVerdict::Free);
    let report = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert!(
        report
            .games
            .iter()
            .all(|g| g.status == StorageGameStatus::NeedsUser),
        "{report:?}"
    );
    assert!(report.games.iter().all(|g| g.question.is_some()));
    assert_eq!(
        store.entry_by_source("caskade-storage", &id).await.unwrap(),
        Some(original)
    );
    assert_eq!(store.library_stats().await.unwrap().total, 1);
}

#[cfg(unix)]
#[tokio::test]
async fn aliased_proton_manifest_retargets_alias_and_keeps_every_persisted_path_relative() {
    use caskade_installer::runtime::alias::AliasStore;
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-alias").await;
    let id = new_id();
    let old = game(
        &root,
        "Before",
        "Aliased prefix",
        Some(&id),
        RelocationVerdict::Aliased,
    );
    let aliases = AliasStore::new(temp.path().join("aliases"));
    let alias_id = "fixture-game-alias";
    aliases.link(alias_id, &old).unwrap();
    let mut record = InstallManifest::read(&old).unwrap().into_record();
    record.alias = Some(alias_id.into());
    record.runtime = RuntimeSpec::ProtonPrefix {
        id: "fixture-prefix".into(),
    };
    record.launch_targets[0].runtime = record.runtime.clone();
    InstallManifest::new(record).unwrap().write(&old).unwrap();
    fs::create_dir_all(old.join(".caskade/prefix/dosdevices")).unwrap();
    std::os::unix::fs::symlink("../../..", old.join(".caskade/prefix/dosdevices/g:")).unwrap();
    let source = StorageSource::new(
        store.clone(),
        Bus::default(),
        Arc::new(reconciler().with_aliases(aliases.clone())),
    );
    let initial = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(initial.imported(), 1, "{initial:?}");
    let original = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.alias_id.as_deref(), Some(alias_id));
    let serialized = serde_json::to_string(&original).unwrap();
    assert!(!serialized.contains(root.path.to_str().unwrap()));
    let new = root.path.join("After");
    fs::rename(&old, &new).unwrap();
    let moved = source
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(moved.imported(), 1, "{moved:?}");
    assert_eq!(moved.games[0].status, StorageGameStatus::Reconciled);
    assert_eq!(aliases.target(alias_id).unwrap(), Some(new.clone()));
    assert_eq!(
        fs::canonicalize(new.join(".caskade/prefix/dosdevices/g:")).unwrap(),
        new
    );
    let entry = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.id, original.id);
    let caskade_discovery::GameRuntime::Windows {
        prefix: Some(prefix),
        runner,
    } = &entry.runtime
    else {
        panic!("prefix runtime expected")
    };
    assert_eq!(prefix.path, Path::new(".caskade/prefix"));
    let runner = runner
        .as_ref()
        .expect("managed prefix retains its runner family");
    assert_eq!(runner.kind, caskade_discovery::RunnerKind::Umu);
    assert!(runner.path.is_none());
    assert!(
        !serde_json::to_string(&entry)
            .unwrap()
            .contains(root.path.to_str().unwrap())
    );
}
