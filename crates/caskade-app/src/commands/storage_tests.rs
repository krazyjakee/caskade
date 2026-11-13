use super::*;
use caskade_core::{
    AppPaths, bus::Bus, db::LibraryStore, launch::LaunchService, settings::SettingsService,
    storage::RootStatus,
};

#[tokio::test]
async fn storage_commands_round_trip_default_identity_labels_and_keep_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let paths = AppPaths::under(temp.path());
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let storage = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths);
    let source = StorageSource::new(store.clone(), Bus::default(), storage.reconciler().unwrap());
    let path = |name: &str| temp.path().join(name).to_string_lossy().into_owned();

    let one = storage_add(&storage, &source, path("one"), Some("SSD".into()))
        .await
        .unwrap();
    let one_id = one.root.id.clone();
    assert!(one.root.is_default);
    assert_eq!(one.status, RootStatus::Online);
    assert!(storage_remove(&storage, &one_id).await.is_err());
    let two = storage_add(&storage, &source, path("two"), None)
        .await
        .unwrap();
    let two_id = two.root.id.clone();
    assert!(
        storage_edit(
            &storage,
            &two_id,
            Some("invalid".into()),
            "../{title}".into()
        )
        .await
        .is_err()
    );
    let edited = storage_edit(
        &storage,
        &two_id,
        Some("HDD".into()),
        "Library {title}".into(),
    )
    .await
    .unwrap();
    assert_eq!(edited.root.label.as_deref(), Some("HDD"));
    assert_eq!(edited.folder_template, "Library {title}");
    storage_default(&storage, &two_id).await.unwrap();
    storage_remove(&storage, &one_id).await.unwrap();
    assert!(temp.path().join("one/.caskade/root.json").is_file());
    let roots = storage_list(&storage).await.unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].root.id, two_id);
    store.close().await;
}
