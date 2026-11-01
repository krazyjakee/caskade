//! Settings → Backup's commands on Tauri's mock runtime.
use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{AppPaths, db::LibraryStore, model::Game, settings::SettingsService};
use serde_json::json;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

struct Fixture {
    _temp: tempfile::TempDir,
    paths: AppPaths,
    window: tauri::WebviewWindow<MockRuntime>,
    _app: tauri::App<MockRuntime>,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    paths.create().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::open_default(&paths)).unwrap();
    tauri::async_runtime::block_on(store.upsert_game(&Game::new("Kept safe"))).unwrap();
    let backups = Backups::new(store.clone(), paths.clone());
    backups.set_settings(SettingsService::new(store.clone()));
    let specta =
        tauri_specta::Builder::<MockRuntime>::new().commands(tauri_specta::collect_commands![
            backup_status,
            backup_plan,
            backup_start,
            backup_cancel,
            backup_inspect,
            backup_restore,
            backup_restore_discard,
            backup_restart::<MockRuntime>,
            backup_reveal
        ]);
    let app = mock_builder()
        .manage(backups)
        .manage(store)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    Fixture {
        _temp: temp,
        paths,
        window,
        _app: app,
    }
}

/// The status once no job runs and a backup has an outcome.
fn settled(window: &tauri::WebviewWindow<MockRuntime>) -> serde_json::Value {
    for _ in 0..500 {
        let status = invoke(window, "backup_status", json!({})).unwrap();
        if status["running"].is_null() && !status["last"].is_null() {
            return status;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the backup didn't finish");
}

#[test]
fn back_up_now_then_check_and_stage_a_restore_over_ipc() {
    let Fixture {
        paths,
        window,
        _temp,
        _app,
    } = fixture();
    let status = invoke(&window, "backup_status", json!({})).unwrap();
    assert!(status["running"].is_null() && status["last"].is_null());
    assert_eq!(
        status["defaultDestination"],
        json!(paths.data.join("backups"))
    );
    let plan = invoke(
        &window,
        "backup_plan",
        json!({"destination": null, "components": ["themes"]}),
    )
    .unwrap();
    assert_eq!(plan["components"][0]["id"], "library");
    assert!(
        plan["excluded"]
            .as_array()
            .unwrap()
            .contains(&json!("credentials"))
    );

    invoke(&window, "backup_start", json!({})).unwrap();
    let last = settled(&window)["last"].clone();
    assert!(last["error"].is_null(), "{last}");
    let archive = last["path"].as_str().unwrap().to_owned();
    assert!(std::path::Path::new(&archive).is_file());

    let inspection = invoke(&window, "backup_inspect", json!({"path": archive})).unwrap();
    assert!(
        !inspection["manifest"]["tables"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(inspection["needsMigration"], false);
    let journal = invoke(
        &window,
        "backup_restore",
        json!({"path": archive, "components": []}),
    )
    .unwrap();
    assert_eq!(journal["state"], "staged");
    let status = invoke(&window, "backup_status", json!({})).unwrap();
    assert_eq!(status["stagedRestore"]["state"], "staged");
    invoke(&window, "backup_restore_discard", json!({})).unwrap();
    let status = invoke(&window, "backup_status", json!({})).unwrap();
    assert!(status["stagedRestore"].is_null());
    // Nothing to apply: no restart.
    let refused = invoke(&window, "backup_restart", json!({})).unwrap_err();
    assert_eq!(refused["code"], "invalid");
}

#[test]
fn only_backups_in_the_backup_folder_are_shown() {
    let Fixture {
        paths,
        window,
        _temp,
        _app,
    } = fixture();
    for path in [
        paths.data.join("library.sqlite"),
        paths.data.join("backups/notes.txt"),
        paths.config.join("caskade-backup-20260101-000000.zip"),
    ] {
        let refused = invoke(&window, "backup_reveal", json!({"path": path})).unwrap_err();
        assert_eq!(refused["code"], "invalid", "{}", path.display());
    }
}

#[test]
fn a_damaged_backup_is_refused_with_its_reason() {
    let Fixture {
        paths,
        window,
        _temp,
        _app,
    } = fixture();
    let damaged = paths.data.join("damaged.zip");
    std::fs::write(&damaged, b"not a backup").unwrap();
    let refused = invoke(&window, "backup_inspect", json!({"path": damaged})).unwrap_err();
    assert_eq!(refused["code"], "corrupt");
}
