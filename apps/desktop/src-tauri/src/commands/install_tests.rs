use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{bus::Bus, db::LibraryStore, install::InstallOptions};
use serde_json::{Value, json};
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
#[cfg(target_os = "linux")]
use {
    caskade_core::model::StorageRoot,
    std::{
        sync::Mutex,
        time::{Duration, Instant},
    },
    tauri::Listener,
    tauri_specta::Event,
};

fn picker() -> InstallPicker {
    InstallPicker {
        files: Arc::new(|| {
            Box::pin(async {
                Ok(vec![
                    "/fixture/Café game.zip".into(),
                    "/fixture/second.7z".into(),
                ])
            })
        }),
        folder: Arc::new(|| Box::pin(async { Ok(None) })),
    }
}

#[test]
fn dialogs_and_job_errors_round_trip_through_typed_ipc() {
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let service = InstallService::new(store, Bus::default(), InstallOptions::default());
    let app = mock_builder()
        .manage(service)
        .manage(picker())
        .invoke_handler(tauri::generate_handler![
            install_start,
            jobs_list,
            job_get,
            job_cancel,
            choose_install_files,
            choose_install_folder
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    assert_eq!(
        invoke(&window, "choose_install_files", json!({})).unwrap(),
        json!(["/fixture/Café game.zip", "/fixture/second.7z"])
    );
    assert_eq!(
        invoke(&window, "choose_install_folder", json!({})).unwrap(),
        Value::Null
    );
    assert_eq!(invoke(&window, "jobs_list", json!({})).unwrap(), json!([]));
    assert!(invoke(&window, "job_get", json!({"id":"unknown"})).is_err());
    assert!(invoke(&window, "job_cancel", json!({"id":"unknown"})).is_err());
    assert!(invoke(&window, "install_start", json!({"input":"","rootId":null})).is_err());
    assert!(
        invoke(
            &window,
            "install_start",
            json!({"input":"/fixture/game.zip","rootId":"missing"})
        )
        .is_err()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn installed_fixture_reports_revisioned_progress_and_completion_through_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Games");
    std::fs::create_dir_all(root.join(".caskade")).unwrap();
    std::fs::write(
        root.join(".caskade/root.json"),
        br#"{"schema_version":1,"root_id":"ipc-root"}"#,
    )
    .unwrap();
    let source = temp.path().join("Fixture Game");
    std::fs::create_dir(&source).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../crates/caskade-installer/tests/fixtures/bin/fixture-game-linux-x86_64"
        ),
        source.join("fixture-game.x86_64"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        source.join("fixture-game.x86_64"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(source.join("data.pak"), b"fixture payload").unwrap();
    let store = tauri::async_runtime::block_on(async {
        let store = LibraryStore::in_memory().await.unwrap();
        store
            .upsert_root(&StorageRoot {
                id: "ipc-root".into(),
                path: root.clone(),
                is_default: true,
                label: None,
            })
            .await
            .unwrap();
        store
    });
    let bus = Bus::default();
    let service = InstallService::new(
        store.clone(),
        bus.clone(),
        InstallOptions {
            downloads: 1,
            aliases: None,
        },
    );
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let storage = StorageService::new(
        store.clone(),
        caskade_core::settings::SettingsService::new(store.clone()),
        launch.clone(),
    )
    .unwrap()
    .with_paths(caskade_core::AppPaths::under(&temp.path().join("App")));
    let second_root = temp.path().join("Other Games");
    std::fs::create_dir_all(second_root.join(".caskade")).unwrap();
    std::fs::write(
        second_root.join(".caskade/root.json"),
        br#"{"schema_version":1,"root_id":"second-root"}"#,
    )
    .unwrap();
    tauri::async_runtime::block_on(store.upsert_root(&StorageRoot {
        id: "second-root".into(),
        path: second_root.clone(),
        is_default: false,
        label: None,
    }))
    .unwrap();
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .dangerously_cast_bigints_to_number()
        .commands(tauri_specta::collect_commands![
            install_start,
            jobs_list,
            job_get,
            job_cancel,
            job_answer,
            job_retry,
            uninstall_game,
            remove_game,
            move_game
        ])
        .events(tauri_specta::collect_events![
            crate::events::JobChanged,
            crate::events::LibraryChanged
        ]);
    let app = mock_builder()
        .manage(service)
        .manage(launch)
        .manage(storage)
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = events.clone();
    app.listen(crate::events::JobChanged::NAME, move |event| {
        seen.lock()
            .unwrap()
            .push(serde_json::from_str(event.payload()).unwrap());
    });
    crate::events::bridge(app.handle().clone(), &bus);
    let queued = invoke(
        &window,
        "install_start",
        json!({"input":source,"rootId":"ipc-root"}),
    )
    .unwrap();
    assert_eq!(queued["status"], "queued");
    let id = queued["id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let finished = loop {
        let job = invoke(&window, "job_get", json!({"id":id})).unwrap();
        if job["status"] == "completed" {
            break job;
        }
        assert_ne!(job["status"], "failed", "{job}");
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(10));
    };
    let game_id = finished["gameId"].as_str().unwrap();
    let entries = tauri::async_runtime::block_on(store.entries_for_game(game_id)).unwrap();
    assert_eq!(entries.len(), 1);
    let listed = invoke(&window, "jobs_list", json!({})).unwrap();
    assert_eq!(listed[0]["id"], id);
    let deadline = Instant::now() + Duration::from_secs(3);
    while events
        .lock()
        .unwrap()
        .last()
        .is_none_or(|job| job["status"] != "completed")
    {
        assert!(
            Instant::now() < deadline,
            "completion event missing: {:?}",
            events.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        invoke(
            &window,
            "job_answer",
            json!({"id":id,"revision":0,"answer":{"type":"select","index":999}})
        )
        .is_err()
    );
    assert!(invoke(&window, "job_retry", json!({"id":id,"candidateIndex":999})).is_err());
    let entry_id = entries[0].id.clone();
    let moved = invoke(
        &window,
        "move_game",
        json!({"entryId":entry_id,"targetRootId":"second-root"}),
    )
    .unwrap();
    let moved = terminal(&window, moved["id"].as_str().unwrap());
    assert_eq!(moved["status"], "completed", "{moved}");
    assert_eq!(moved["phase"], "move");
    let updated = tauri::async_runtime::block_on(store.get_entry(&entry_id))
        .unwrap()
        .unwrap();
    assert!(
        matches!(updated.location,Some(caskade_core::model::InstallLocation::Managed{ref root_id,..}) if root_id=="second-root")
    );
    let removed = invoke(&window, "uninstall_game", json!({"entryId":entry_id})).unwrap();
    let removed = terminal(&window, removed["id"].as_str().unwrap());
    assert_eq!(removed["status"], "completed", "{removed}");
    assert!(
        tauri::async_runtime::block_on(store.get_entry(&entry_id))
            .unwrap()
            .is_none()
    );
    assert!(source.exists());
    // The uninstall left the game without entries; removing it needs no job.
    let game_id = updated.game_id.clone();
    let gone = invoke(&window, "remove_game", json!({"gameId":game_id})).unwrap();
    assert_eq!(gone, Value::Null);
    assert!(
        tauri::async_runtime::block_on(store.game(&game_id))
            .unwrap()
            .is_none()
    );
    let events = events.lock().unwrap();
    assert!(events.iter().any(|job| job["phase"] == "install"));
    let original = events
        .iter()
        .filter(|event| event["id"] == id)
        .collect::<Vec<_>>();
    assert!(
        original
            .windows(2)
            .all(|pair| pair[0]["revision"].as_u64() < pair[1]["revision"].as_u64())
    );
}

#[cfg(target_os = "linux")]
fn terminal(window: &tauri::WebviewWindow<tauri::test::MockRuntime>, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = invoke(window, "job_get", json!({"id":id})).unwrap();
        if matches!(
            job["status"].as_str(),
            Some("completed" | "failed" | "cancelled" | "needsUser")
        ) {
            return job;
        }
        assert!(Instant::now() < deadline, "job did not finish: {job}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A missing or rejected 7-Zip sidecar costs the archive formats that need
/// it, not the app: setup still manages the install service and its queue.
#[test]
fn setup_without_the_7zip_sidecar_still_starts() {
    use tauri::Manager;
    let temp = tempfile::tempdir().unwrap();
    let paths = caskade_core::AppPaths::under(&temp.path().join("App"));
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let settings = caskade_core::settings::SettingsService::new(store.clone());
    let launch = LaunchService::new(store.clone()).with_aliases(None);
    let storage = StorageService::new(store.clone(), settings.clone(), launch)
        .unwrap()
        .with_paths(paths.clone());
    let app = mock_builder()
        .manage(storage)
        .build(mock_context(noop_assets()))
        .unwrap();
    // Not there at all.
    setup_with_sidecar(
        app.handle(),
        store.clone(),
        Bus::default(),
        &paths,
        settings.clone(),
        Ok(temp.path().join("7zz")),
    )
    .unwrap();
    assert!(app.try_state::<InstallService>().is_some());
    assert!(
        app.try_state::<crate::commands::install_requests::InstallRequests>()
            .is_some()
    );
    assert!(app.state::<InstallService>().jobs().is_empty());
    // Not even a path for it.
    let app = mock_builder()
        .manage(
            StorageService::new(
                store.clone(),
                settings.clone(),
                LaunchService::new(store.clone()),
            )
            .unwrap()
            .with_paths(paths.clone()),
        )
        .build(mock_context(noop_assets()))
        .unwrap();
    setup_with_sidecar(
        app.handle(),
        store,
        Bus::default(),
        &paths,
        settings,
        Err(std::io::Error::other("no executable path")),
    )
    .unwrap();
    assert!(app.try_state::<InstallService>().is_some());
}
