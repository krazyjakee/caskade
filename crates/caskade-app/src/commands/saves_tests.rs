use super::*;
use caskade_core::{
    ludusavi::ManifestStatus,
    model::{Game, GameEntry, InstallLocation, InstallState, SourceId},
    saves::{LocationKind, versions::VersionReason},
};

/// The Saves panel's commands (#241): the history, restoring an older
/// version (the one it replaced stays), a location that isn't there, and
/// the manifest's state before any download.
#[tokio::test]
async fn history_restore_and_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Fixture")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), "fixture");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: temp.path().join("game"),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    let folder = temp.path().join("saves");
    std::fs::create_dir_all(&folder).unwrap();
    store
        .set_save_preferences(
            &entry.id,
            &SavePreferences {
                manifest_title: None,
                locations: Some(vec![LocationSpec {
                    kind: LocationKind::Folder,
                    path: folder.to_string_lossy().into_owned(),
                }]),
            },
            0,
        )
        .await
        .unwrap();
    let manifest = Ludusavi::new(temp.path().join("ludusavi"));
    let versions = SaveVersions::new(
        Saves::new(store.clone(), manifest.clone()),
        temp.path().join("blobs"),
    );
    let slot = folder.join("slot.sav");
    std::fs::write(&slot, "older").unwrap();
    let older = versions
        .capture(&entry.id, None, VersionReason::Session)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(&slot, "newer").unwrap();
    let newer = versions
        .capture(&entry.id, None, VersionReason::Session)
        .await
        .unwrap()
        .unwrap();

    let history = game_save_history(&versions, game.id.clone()).await.unwrap();
    assert!(history.conflict.is_empty());
    assert_eq!(history.versions[0].version.id, newer.id);
    assert!(history.versions[0].current);
    assert_eq!(history.versions[1].version.id, older.id);
    assert!(history.versions[1].complete);
    assert_eq!(history.versions[1].version.reason, VersionReason::Session);

    let restored = game_saves_restore(&versions, entry.id.clone(), older.id.clone())
        .await
        .unwrap();
    assert_eq!(restored.reason, VersionReason::Restored);
    assert_eq!(std::fs::read_to_string(&slot).unwrap(), "older");
    let history = game_save_history(&versions, game.id.clone()).await.unwrap();
    let ids: Vec<&str> = history
        .versions
        .iter()
        .map(|each| each.version.id.as_str())
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], restored.id);
    assert!(ids.contains(&newer.id.as_str()));
    // An unknown version is refused and changes nothing.
    assert!(
        game_saves_restore(&versions, entry.id.clone(), "nope".into())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&slot).unwrap(), "older");
    // A location the release doesn't have is refused before any file
    // manager starts.
    assert!(
        game_saves_reveal(&versions, entry.id.clone(), 5)
            .await
            .is_err()
    );
    assert_eq!(
        saves_manifest_status(&manifest).await.unwrap(),
        ManifestStatus {
            available: false,
            checked_at_ms: None,
        }
    );
    store.close().await;
}
