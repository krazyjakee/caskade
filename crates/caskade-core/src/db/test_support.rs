//! Helpers the library store's tests share.
use super::LibraryStore;
use crate::model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId, new_id};
use std::{collections::BTreeMap, path::PathBuf};

/// The admin every library starts with.
pub(super) async fn admin(store: &LibraryStore) -> String {
    store.accounts().await.unwrap()[0].id.clone()
}

/// An installed game with one entry, launched by a program.
pub(super) async fn game_entry(store: &LibraryStore, title: &str) -> (Game, GameEntry) {
    let game = Game::new(title);
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: PathBuf::from("/fixture/game"),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: PathBuf::from("/fixture/game/play"),
        args: vec!["--test".into(), "a;literal".into()],
        cwd: Some(PathBuf::from("/fixture/game")),
        env: BTreeMap::from([("FIXTURE".into(), "literal value".into())]),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    (game, entry)
}

/// Imports `game` from `source`/`key`, as discovery would; its id after.
pub(super) async fn import_game(
    store: &LibraryStore,
    game: &Game,
    source: &'static str,
    key: &str,
) -> String {
    let entry = GameEntry::new(&game.id, SourceId::new(source), key);
    store
        .import_group(game, &[entry], None)
        .await
        .unwrap()
        .game_id
}

/// [`import_game`] for a new game titled `title`.
pub(super) async fn import(
    store: &LibraryStore,
    title: &str,
    source: &'static str,
    key: &str,
) -> String {
    import_game(store, &Game::new(title), source, key).await
}
