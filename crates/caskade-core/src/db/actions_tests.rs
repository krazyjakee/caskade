//! A release's launch actions (#177) in the store: order, default, the
//! paths a managed install keeps relative, and rediscovery.
use super::LibraryStore;
use crate::model::{
    ActionKind, ActionTracking, EntryActionSettings, Game, GameAction, GameEntry, InstallLocation,
    LaunchSpec, SourceId, StorageRoot, built_in_action_id, new_id,
};
use caskade_discovery::InstallState;
use std::collections::BTreeMap;

fn exe(path: &str) -> LaunchSpec {
    LaunchSpec::Executable {
        path: path.into(),
        args: vec!["--flag".into(), "two words".into()],
        cwd: None,
        env: BTreeMap::from([("MODS".into(), "on".into())]),
    }
}
fn entry(game: &Game, key: &str) -> GameEntry {
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), key);
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: "/games/fixture".into(),
    });
    entry.launch = Some(exe("/games/fixture/game"));
    entry
}
fn action(entry: &GameEntry, name: &str, kind: ActionKind) -> GameAction {
    GameAction {
        id: new_id(),
        entry_id: entry.id.clone(),
        name: name.into(),
        kind,
        target: exe("/games/fixture/tool"),
        tracking: ActionTracking::default(),
        proton: None,
    }
}
async fn imported(store: &LibraryStore, key: &str) -> (Game, GameEntry) {
    let game = Game::new("Fixture");
    let entry = entry(&game, key);
    let group = store.import_group(&game, &[entry], None).await.unwrap();
    assert!(group.created);
    let entry = store.get_entry(&group.entry_ids[0]).await.unwrap().unwrap();
    (game, entry)
}
fn names(actions: &[GameAction]) -> Vec<&str> {
    actions.iter().map(|action| action.name.as_str()).collect()
}

#[tokio::test]
async fn actions_keep_their_order_name_target_and_tracking() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (_, entry) = imported(&store, "a").await;
    let mut modded = action(&entry, "  Modded  ", ActionKind::Play);
    modded.tracking = ActionTracking {
        mode: crate::tracking::TrackingMode::ProcessName,
        folder: None,
        names: vec![" game.exe ".into(), "".into()],
    };
    let saved = store.save_game_action(&modded).await.unwrap();
    assert_eq!(saved.name, "Modded");
    assert_eq!(saved.tracking.names, ["game.exe"]);
    let config = store
        .save_game_action(&action(&entry, "Configure", ActionKind::Utility))
        .await
        .unwrap();
    assert_eq!(
        names(&store.game_actions(&entry.id).await.unwrap()),
        ["Modded", "Configure"]
    );
    store
        .order_game_actions(&entry.id, &[config.id.clone(), saved.id.clone()])
        .await
        .unwrap();
    let listed = store.game_actions(&entry.id).await.unwrap();
    assert_eq!(names(&listed), ["Configure", "Modded"]);
    assert_eq!(listed[1], saved);
    // An order must name each action once.
    assert!(
        store
            .order_game_actions(&entry.id, std::slice::from_ref(&config.id))
            .await
            .is_err()
    );
    // Renaming keeps its place.
    let mut renamed = saved.clone();
    renamed.name = "Modded (MO2)".into();
    store.save_game_action(&renamed).await.unwrap();
    assert_eq!(
        names(&store.game_actions(&entry.id).await.unwrap()),
        ["Configure", "Modded (MO2)"]
    );
    // Invalid input is refused.
    let mut nameless = action(&entry, " ", ActionKind::Play);
    assert!(store.save_game_action(&nameless).await.is_err());
    nameless.name = "x".into();
    nameless.entry_id = "missing".into();
    assert!(store.save_game_action(&nameless).await.is_err());
    let mut slash = action(&entry, "Bad", ActionKind::Play);
    slash.tracking.names = vec!["bin/game".into()];
    assert!(store.save_game_action(&slash).await.is_err());
}

#[tokio::test]
async fn the_default_is_a_play_action_of_the_release_and_falls_back_when_it_goes() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (_, entry) = imported(&store, "a").await;
    let (_, other) = imported(&store, "b").await;
    let modded = store
        .save_game_action(&action(&entry, "Modded", ActionKind::Play))
        .await
        .unwrap();
    let tool = store
        .save_game_action(&action(&entry, "Configure", ActionKind::Utility))
        .await
        .unwrap();
    let theirs = store
        .save_game_action(&action(&other, "Theirs", ActionKind::Play))
        .await
        .unwrap();
    assert_eq!(
        store.entry_action_settings(&entry.id).await.unwrap(),
        EntryActionSettings::default()
    );
    let with = |id: &str| EntryActionSettings {
        default_action: Some(id.into()),
        ask: true,
        ..Default::default()
    };
    for refused in [&tool.id, &theirs.id, &"missing".to_owned()] {
        assert!(
            store
                .save_entry_action_settings(&entry.id, &with(refused))
                .await
                .is_err()
        );
    }
    // The built-in action's id is stored as no default.
    let built_in = store
        .save_entry_action_settings(&entry.id, &with(&built_in_action_id(&entry.id)))
        .await
        .unwrap();
    assert_eq!(built_in.default_action, None);
    store
        .save_entry_action_settings(&entry.id, &with(&modded.id))
        .await
        .unwrap();
    // Turning the default into a tool unsets it, and so does deleting it.
    let mut as_tool = modded.clone();
    as_tool.kind = ActionKind::Utility;
    store.save_game_action(&as_tool).await.unwrap();
    let settings = store.entry_action_settings(&entry.id).await.unwrap();
    assert_eq!(settings.default_action, None);
    assert!(settings.ask);
    store.save_game_action(&modded).await.unwrap();
    store
        .save_entry_action_settings(&entry.id, &with(&modded.id))
        .await
        .unwrap();
    assert!(store.delete_game_action(&modded.id).await.unwrap());
    assert!(!store.delete_game_action(&modded.id).await.unwrap());
    assert_eq!(
        store
            .entry_action_settings(&entry.id)
            .await
            .unwrap()
            .default_action,
        None
    );
    // An action can't move to another release.
    let mut moved = tool.clone();
    moved.entry_id = other.id.clone();
    assert!(store.save_game_action(&moved).await.is_err());
}

#[tokio::test]
async fn a_managed_release_keeps_its_actions_paths_relative_so_they_move_with_it() {
    let store = LibraryStore::in_memory().await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    store
        .upsert_root(&StorageRoot {
            id: "fixture-root".into(),
            path: temp.path().into(),
            is_default: true,
            label: None,
        })
        .await
        .unwrap();
    let game = Game::new("Managed");
    store.upsert_game(&game).await.unwrap();
    let mut managed = GameEntry::new(&game.id, SourceId::new("caskade-storage"), "m");
    managed.state = InstallState::Installed;
    managed.location = Some(InstallLocation::Managed {
        root_id: "fixture-root".into(),
        rel_dir: "Game".into(),
    });
    managed.launch = Some(exe("game"));
    store.upsert_entry(&managed).await.unwrap();
    let mut inside = action(&managed, "Setup", ActionKind::Utility);
    inside.target = exe("tools\\setup.exe");
    let saved = store.save_game_action(&inside).await.unwrap();
    let LaunchSpec::Executable { path, .. } = &saved.target else {
        unreachable!()
    };
    assert_eq!(path.to_str(), Some("tools/setup.exe"));
    // A tool outside the install keeps its absolute path.
    let outside = store
        .save_game_action(&action(&managed, "Editor", ActionKind::Utility))
        .await
        .unwrap();
    assert_eq!(outside.target, exe("/games/fixture/tool"));
    // A path out of the install is refused.
    let mut escape = action(&managed, "Escape", ActionKind::Utility);
    escape.target = exe("../other/tool");
    assert!(store.save_game_action(&escape).await.is_err());
    let mut tracking = action(&managed, "Tracked", ActionKind::Play);
    tracking.tracking.mode = crate::tracking::TrackingMode::Folder;
    tracking.tracking.folder = Some("bin".into());
    assert_eq!(
        store
            .save_game_action(&tracking)
            .await
            .unwrap()
            .tracking
            .folder
            .as_deref(),
        Some(std::path::Path::new("bin"))
    );
}

#[tokio::test]
async fn actions_survive_rediscovery_and_follow_a_retired_entry() {
    let store = LibraryStore::in_memory().await.unwrap();
    let (game, entry) = imported(&store, "a").await;
    let modded = store
        .save_game_action(&action(&entry, "Modded", ActionKind::Play))
        .await
        .unwrap();
    let settings = EntryActionSettings {
        default_action: Some(modded.id.clone()),
        ask: false,
        ..Default::default()
    };
    store
        .save_entry_action_settings(&entry.id, &settings)
        .await
        .unwrap();
    // The same release found again, with a new launch from its source.
    let mut again = GameEntry::new(&game.id, SourceId::new("fixture"), "a");
    again.state = InstallState::Installed;
    again.launch = Some(exe("/games/fixture/game-v2"));
    let group = store.import_group(&game, &[again], None).await.unwrap();
    assert_eq!(group.entry_ids, std::slice::from_ref(&entry.id));
    assert_eq!(
        store.game_actions(&entry.id).await.unwrap(),
        std::slice::from_ref(&modded)
    );
    assert_eq!(
        store.entry_action_settings(&entry.id).await.unwrap(),
        settings
    );
    // A later import retires the entry into another one of the game's.
    let mut successor = GameEntry::new(&game.id, SourceId::new("fixture"), "b");
    successor.state = InstallState::Installed;
    successor.launch = Some(exe("/games/fixture/game"));
    let group = store
        .import_group_retiring(
            &game,
            &[successor],
            &[(SourceId::new("fixture"), "a".into())],
            None,
        )
        .await
        .unwrap();
    let kept = &group.entry_ids[0];
    assert!(store.get_entry(&entry.id).await.unwrap().is_none());
    let moved = store.game_actions(kept).await.unwrap();
    assert_eq!(names(&moved), ["Modded"]);
    assert_eq!(moved[0].entry_id, *kept);
    assert_eq!(
        store
            .entry_action_settings(kept)
            .await
            .unwrap()
            .default_action,
        Some(modded.id.clone())
    );
    // Deleting the entry deletes its actions.
    store.delete_entry(kept).await.unwrap();
    assert!(store.game_action(&modded.id).await.unwrap().is_none());
}
