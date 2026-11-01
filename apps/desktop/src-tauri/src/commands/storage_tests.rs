use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    AppPaths, bus::Bus, db::LibraryStore, launch::LaunchService, settings::SettingsService,
};
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};
#[test]
fn storage_commands_round_trip_default_identity_labels_and_keep_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let paths = AppPaths::under(temp.path());
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let storage = StorageService::new(store.clone(), settings, LaunchService::new(store.clone()))
        .unwrap()
        .with_paths(paths);
    let source = StorageSource::new(store.clone(), Bus::default(), storage.reconciler().unwrap());
    let app = mock_builder()
        .manage(storage)
        .manage(source)
        .invoke_handler(tauri::generate_handler![
            storage_list,
            storage_add,
            storage_edit,
            storage_default,
            storage_remove
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let one = invoke(
        &window,
        "storage_add",
        json!({"path":temp.path().join("one"),"label":"SSD"}),
    )
    .unwrap();
    let one_id = one["root"]["id"].as_str().unwrap();
    assert_eq!(one["root"]["isDefault"], true);
    assert_eq!(one["status"], "online");
    assert!(invoke(&window, "storage_remove", json!({"id":one_id})).is_err());
    let two = invoke(
        &window,
        "storage_add",
        json!({"path":temp.path().join("two"),"label":null}),
    )
    .unwrap();
    let two_id = two["root"]["id"].as_str().unwrap();
    assert!(
        invoke(
            &window,
            "storage_edit",
            json!({"id":two_id,"label":"invalid","folderTemplate":"../{title}"})
        )
        .is_err()
    );
    let edited = invoke(
        &window,
        "storage_edit",
        json!({"id":two_id,"label":"HDD","folderTemplate":"Library {title}"}),
    )
    .unwrap();
    assert_eq!(edited["root"]["label"], "HDD");
    assert_eq!(edited["folderTemplate"], "Library {title}");
    invoke(&window, "storage_default", json!({"id":two_id})).unwrap();
    invoke(&window, "storage_remove", json!({"id":one_id})).unwrap();
    assert!(temp.path().join("one/.caskade/root.json").is_file());
    let roots = invoke(&window, "storage_list", json!({})).unwrap();
    assert_eq!(roots.as_array().unwrap().len(), 1);
    assert_eq!(roots[0]["root"]["id"], two_id);
    tauri::async_runtime::block_on(store.close());
}
