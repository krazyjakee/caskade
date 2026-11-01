//! What a game's menu changes or opens: its favourite mark, and the folder
//! an entry's files are in.
use super::LibraryStore;
use crate::model::{Game, GameEntry, InstallLocation, SourceId, StorageRoot};

async fn import(store: &LibraryStore, title: &str, source: &'static str, key: &str) -> String {
    let game = Game::new(title);
    let entry = GameEntry::new(&game.id, SourceId::new(source), key);
    store
        .import_group(&game, &[entry], None)
        .await
        .unwrap()
        .game_id
}

#[tokio::test]
async fn a_favourite_stays_one_through_rescans_and_can_be_unmarked() {
    let store = LibraryStore::in_memory().await.unwrap();
    let doom = import(&store, "Doom", "steam", "2280").await;
    assert!(store.set_game_favorite(&doom, true).await.unwrap().favorite);
    assert_eq!(store.library_stats().await.unwrap().favorites, 1);

    import(&store, "Doom", "steam", "2280").await;
    assert!(store.game(&doom).await.unwrap().unwrap().favorite);

    store.set_game_favorite(&doom, false).await.unwrap();
    assert_eq!(store.library_stats().await.unwrap().favorites, 0);
    assert!(store.set_game_favorite("missing", true).await.is_err());
}

async fn entry_at(store: &LibraryStore, location: InstallLocation) -> String {
    let game = store.upsert_game(&Game::new("Quake")).await.unwrap();
    let key = game.id.clone();
    let mut entry = GameEntry::new(&game.id, SourceId::new("caskade-storage"), key);
    entry.location = Some(location);
    store.upsert_entry(&entry).await.unwrap().id
}

#[tokio::test]
async fn an_entrys_folder_is_its_managed_folder_its_external_folder_or_its_files_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(root.join("Quake")).unwrap();
    std::fs::write(dir.path().join("quake.exe"), b"").unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "games".into(),
            path: root.clone(),
            is_default: true,
            label: None,
        })
        .await
        .unwrap();

    let managed = InstallLocation::Managed {
        root_id: "games".into(),
        rel_dir: "Quake".into(),
    };
    let managed = entry_at(&store, managed).await;
    assert_eq!(
        store.entry_folder(&managed).await.unwrap(),
        root.join("Quake")
    );

    let folder = InstallLocation::External {
        path: root.join("Quake"),
    };
    let folder = entry_at(&store, folder).await;
    assert_eq!(
        store.entry_folder(&folder).await.unwrap(),
        root.join("Quake")
    );

    let file = InstallLocation::External {
        path: dir.path().join("quake.exe"),
    };
    let file = entry_at(&store, file).await;
    assert_eq!(store.entry_folder(&file).await.unwrap(), dir.path());

    let gone = InstallLocation::External {
        path: dir.path().join("gone"),
    };
    let gone = entry_at(&store, gone).await;
    assert!(store.entry_folder(&gone).await.is_err());
    assert!(store.entry_folder("missing").await.is_err());
}

#[tokio::test]
async fn a_default_entry_is_one_of_the_games_and_survives_rescans_merges_and_separations() {
    let store = LibraryStore::in_memory().await.unwrap();
    let doom = import(&store, "Doom", "steam", "2280").await;
    let gog = import(&store, "Doom (GOG)", "gog", "1").await;
    let other = import(&store, "Quake", "steam", "2310").await;
    let entry = |game: String| {
        let store = &store;
        async move { store.entries_for_game(&game).await.unwrap()[0].id.clone() }
    };
    let gog_entry = entry(gog.clone()).await;
    let quake_entry = entry(other).await;

    assert!(
        store
            .set_game_default_entry(&doom, Some(&quake_entry))
            .await
            .is_err()
    );
    store
        .set_game_default_entry(&gog, Some(&gog_entry))
        .await
        .unwrap();
    store.merge_games(&doom, &gog).await.unwrap();
    let game = store.game(&doom).await.unwrap().unwrap();
    assert_eq!(game.default_entry.as_deref(), Some(gog_entry.as_str()));

    import(&store, "Doom", "steam", "2280").await;
    let game = store.game(&doom).await.unwrap().unwrap();
    assert_eq!(game.default_entry.as_deref(), Some(gog_entry.as_str()));

    let apart = store.separate_entry(&gog_entry, &[], &[]).await.unwrap();
    assert_eq!(apart.default_entry, None);
    assert_eq!(
        store.game(&doom).await.unwrap().unwrap().default_entry,
        None
    );

    store.set_game_default_entry(&doom, None).await.unwrap();
    assert!(store.set_game_default_entry("missing", None).await.is_err());
}
