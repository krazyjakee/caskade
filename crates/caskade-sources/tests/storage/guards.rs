//! Uncertain scans keep known entries; unsafe trees are refused.
use super::*;

#[tokio::test]
async fn offline_failed_and_cancelled_roots_preserve_known_library_entries() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-offline").await;
    let id = new_id();
    game(&root, "Game", "Offline", Some(&id), RelocationVerdict::Free);
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
    let absent = temp.path().join("detached");
    fs::rename(&root.path, &absent).unwrap();
    let report = source.refresh(CancellationToken::new()).await.unwrap();
    assert!(report.roots[0].error.is_some());
    assert_eq!(
        store.entry_by_source("caskade-storage", &id).await.unwrap(),
        Some(before.clone())
    );
    fs::rename(&absent, &root.path).unwrap();
    fs::write(root.path.join("Game/.caskade/install.json"), b"{invalid").unwrap();
    let report = source.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(report.roots[0].games[0].status, StorageGameStatus::Failed);
    assert_eq!(
        store.entry_by_source("caskade-storage", &id).await.unwrap(),
        Some(before)
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(source.refresh(cancel).await.is_err());
}

#[tokio::test]
async fn depth_limit_never_marks_unseen_known_games_missing_and_identity_mismatch_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-depth").await;
    let id = new_id();
    game(
        &root,
        "Nested/Game",
        "Deep",
        Some(&id),
        RelocationVerdict::Free,
    );
    source(&store)
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    let before = store
        .entry_by_source("caskade-storage", &id)
        .await
        .unwrap()
        .unwrap();
    let bounded = StorageSource::new(
        store.clone(),
        Bus::default(),
        Arc::new(reconciler().with_limits(ScanLimits {
            max_entries: 100,
            max_depth: 1,
        })),
    );
    let report = bounded
        .adopt_root(&root, CancellationToken::new())
        .await
        .unwrap();
    assert!(!report.warnings.is_empty());
    assert_eq!(
        store.entry_by_source("caskade-storage", &id).await.unwrap(),
        Some(before)
    );
    RootManifest::new("different-root")
        .unwrap()
        .write(&root.path)
        .unwrap();
    assert!(
        bounded
            .adopt_root(&root, CancellationToken::new())
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_game_or_manifest_cannot_adopt_or_modify_an_external_tree() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let root = root(&store, &temp.path().join("root"), "root-safe").await;
    let outside = StorageRoot {
        id: "outside".into(),
        path: temp.path().join("outside"),
        is_default: false,
        label: None,
    };
    fs::create_dir(&outside.path).unwrap();
    let game = game(&outside, "Game", "Outside", None, RelocationVerdict::Free);
    let bytes = fs::read(game.join(".caskade/install.json")).unwrap();
    symlink(&game, root.path.join("Link")).unwrap();
    fs::create_dir(root.path.join("Other")).unwrap();
    symlink(game.join(".caskade"), root.path.join("Other/.caskade")).unwrap();
    let report = source(&store)
        .refresh(CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.imported(), 0, "{report:?}");
    assert_eq!(store.library_stats().await.unwrap().total, 0);
    assert_eq!(fs::read(game.join(".caskade/install.json")).unwrap(), bytes);
}
