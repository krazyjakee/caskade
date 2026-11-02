use super::*;
use crate::commands::test_ipc::invoke;
use caskade_core::{
    model::{Game, GameEntry, InstallLocation, InstallState, SourceId},
    saves::{LocationKind, versions::VersionReason},
};
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};

/// The Saves panel's commands (#241) over IPC: the history, restoring an
/// older version (the one it replaced stays), a location that isn't there,
/// and the manifest's state before any download.
#[test]
fn history_restore_and_manifest_through_ipc() {
    let temp = tempfile::tempdir().unwrap();
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let game = tauri::async_runtime::block_on(store.upsert_game(&Game::new("Fixture"))).unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: temp.path().join("game"),
    });
    let entry = tauri::async_runtime::block_on(store.upsert_entry(&entry)).unwrap();
    let folder = temp.path().join("saves");
    std::fs::create_dir_all(&folder).unwrap();
    tauri::async_runtime::block_on(store.set_save_preferences(
        &entry.id,
        &SavePreferences {
            manifest_title: None,
            locations: Some(vec![LocationSpec {
                kind: LocationKind::Folder,
                path: folder.to_string_lossy().into_owned(),
            }]),
        },
        0,
    ))
    .unwrap();
    let manifest = Ludusavi::new(temp.path().join("ludusavi"));
    let versions = SaveVersions::new(
        Saves::new(store.clone(), manifest.clone()),
        temp.path().join("blobs"),
    );
    let slot = folder.join("slot.sav");
    std::fs::write(&slot, "older").unwrap();
    let older =
        tauri::async_runtime::block_on(versions.capture(&entry.id, None, VersionReason::Session))
            .unwrap()
            .unwrap();
    std::fs::write(&slot, "newer").unwrap();
    let newer =
        tauri::async_runtime::block_on(versions.capture(&entry.id, None, VersionReason::Session))
            .unwrap()
            .unwrap();

    let app = mock_builder()
        .manage(store.clone())
        .manage(manifest)
        .manage(versions)
        .invoke_handler(tauri::generate_handler![
            game_save_history,
            game_saves_restore,
            game_saves_reveal,
            saves_manifest_status
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let history = invoke(&window, "game_save_history", json!({"gameId": game.id})).unwrap();
    assert_eq!(history["conflict"], json!([]));
    assert_eq!(history["versions"][0]["version"]["id"], json!(newer.id));
    assert_eq!(history["versions"][0]["current"], json!(true));
    assert_eq!(history["versions"][1]["version"]["id"], json!(older.id));
    assert_eq!(history["versions"][1]["complete"], json!(true));
    assert_eq!(
        history["versions"][1]["version"]["reason"],
        json!("session")
    );

    let restored = invoke(
        &window,
        "game_saves_restore",
        json!({"entryId": entry.id, "versionId": older.id}),
    )
    .unwrap();
    assert_eq!(restored["reason"], json!("restored"));
    assert_eq!(std::fs::read_to_string(&slot).unwrap(), "older");
    let history = invoke(&window, "game_save_history", json!({"gameId": game.id})).unwrap();
    let ids: Vec<&str> = history["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|each| each["version"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], restored["id"].as_str().unwrap());
    assert!(ids.contains(&newer.id.as_str()));
    // An unknown version is refused and changes nothing.
    assert!(
        invoke(
            &window,
            "game_saves_restore",
            json!({"entryId": entry.id, "versionId": "nope"}),
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(&slot).unwrap(), "older");
    // A location the release doesn't have is refused before any file
    // manager starts.
    assert!(
        invoke(
            &window,
            "game_saves_reveal",
            json!({"entryId": entry.id, "location": 5}),
        )
        .is_err()
    );
    let status = invoke(&window, "saves_manifest_status", json!({})).unwrap();
    assert_eq!(status, json!({"available": false, "checkedAtMs": null}));
    tauri::async_runtime::block_on(store.close());
}
