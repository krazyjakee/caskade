//! Launch actions (#177) through the real pipeline and store: a custom
//! default, a mod manager that starts the game, tools without sessions,
//! relocation, cancellation and a stale process after a restart.
use super::{
    test_support::{Events, fast_config},
    *,
};
use crate::{
    db::LibraryStore,
    model::{
        ActionKind, ActionTracking, EntryActionSettings, Game, GameAction, GameEntry,
        InstallLocation, LaunchSpec, StorageRoot, new_id,
    },
    sessions::{RecoveryState, SessionPhase},
    tracking::TrackingMode,
};
use caskade_discovery::{InstallState, SourceId};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::test_programs::write_program(path, format!("#!/bin/sh\n{body}\n"));
}
fn exe(path: impl Into<PathBuf>, args: &[&str]) -> LaunchSpec {
    LaunchSpec::Executable {
        path: path.into(),
        args: args.iter().map(|arg| (*arg).into()).collect(),
        cwd: None,
        env: BTreeMap::new(),
    }
}
fn action(entry: &GameEntry, kind: ActionKind, target: LaunchSpec) -> GameAction {
    GameAction {
        id: new_id(),
        entry_id: entry.id.clone(),
        name: "Action".into(),
        kind,
        target,
        tracking: ActionTracking::default(),
        proton: None,
    }
}
/// A game in `dir` whose own launch writes `own` when it runs.
async fn release(store: &LibraryStore, dir: &Path) -> GameEntry {
    let game = Game::new("Fixture");
    store.upsert_game(&game).await.unwrap();
    script(
        &dir.join("game.sh"),
        &format!("touch '{}'", dir.join("own").display()),
    );
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), new_id());
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External { path: dir.into() });
    entry.launch = Some(exe(dir.join("game.sh"), &[]));
    store.upsert_entry(&entry).await.unwrap();
    entry
}
fn service(store: &LibraryStore) -> LaunchService {
    LaunchService::new(store.clone())
        .with_hooks(Arc::new(Events::default()))
        .with_config(fast_config())
        .with_aliases(None)
}
async fn wait_for(path: &Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{} never appeared", path.display());
}
fn recovery(session: &crate::model::PlaySession) -> RecoveryState {
    serde_json::from_value(session.tracking.clone().unwrap()).unwrap()
}

#[tokio::test]
async fn the_default_action_launches_with_its_arguments_and_the_same_entry() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path()).await;
    let args = temp.path().join("args");
    script(
        &temp.path().join("modded.sh"),
        &format!("printf '%s\\n' \"$@\" \"$MODS\" > '{}'", args.display()),
    );
    let mut modded = action(
        &entry,
        ActionKind::Play,
        LaunchSpec::Executable {
            path: temp.path().join("modded.sh"),
            args: vec!["two words".into(), "$(touch pwned)".into(), "a;b".into()],
            cwd: None,
            env: BTreeMap::from([("MODS".into(), "on".into())]),
        },
    );
    modded.tracking.mode = TrackingMode::OriginalProcess;
    let modded = store.save_game_action(&modded).await.unwrap();
    store
        .save_entry_action_settings(
            &entry.id,
            &EntryActionSettings {
                default_action: Some(modded.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let session = service(&store)
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(session.entry_id, entry.id);
    assert_eq!(recovery(&session).action, Some(modded.id.clone()));
    assert_eq!(
        fs::read_to_string(&args).unwrap(),
        "two words\n$(touch pwned)\na;b\non\n"
    );
    assert!(!temp.path().join("pwned").exists());
    assert!(!temp.path().join("own").exists());

    // Its own launch is still there to choose, under the entry's id.
    let choice = LaunchChoice {
        action: Some(crate::model::built_in_action_id(&entry.id)),
        ..Default::default()
    };
    let own = service(&store)
        .launch_with(&entry.id, &choice)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(own.entry_id, entry.id);
    assert!(temp.path().join("own").exists());
    assert_eq!(
        store
            .sessions_for_game(&entry.game_id, 10)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn a_mod_manager_play_action_tracks_the_game_it_starts() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path()).await;
    let game = temp.path().join("bin/modgame");
    fs::create_dir_all(game.parent().unwrap()).unwrap();
    crate::test_programs::copy_program("/bin/sleep", &game);
    // The manager starts the game and exits at once.
    script(
        &temp.path().join("manager.sh"),
        &format!("'{}' 0.7 &\nexit 0", game.display()),
    );
    let mut manager = action(
        &entry,
        ActionKind::Play,
        exe(temp.path().join("manager.sh"), &[]),
    );
    manager.tracking = ActionTracking {
        mode: TrackingMode::ProcessName,
        folder: None,
        names: vec!["modgame".into()],
    };
    let manager = store.save_game_action(&manager).await.unwrap();
    let choice = LaunchChoice {
        action: Some(manager.id.clone()),
        ..Default::default()
    };
    let session = service(&store)
        .launch_with(&entry.id, &choice)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(
        session.duration_ms >= 400,
        "the game's time counts, not the manager's: {} ms",
        session.duration_ms
    );
    assert!(session.ended_at_ms.is_some());
}

#[tokio::test]
async fn a_tool_runs_without_a_session_or_last_played() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path()).await;
    let ran = temp.path().join("configured");
    script(
        &temp.path().join("configure.sh"),
        &format!("touch '{}'", ran.display()),
    );
    let tool = store
        .save_game_action(&action(
            &entry,
            ActionKind::Utility,
            exe(temp.path().join("configure.sh"), &[]),
        ))
        .await
        .unwrap();
    let service = service(&store);
    service.run_tool(&entry.id, &tool.id).await.unwrap();
    wait_for(&ran).await;
    assert!(
        store
            .sessions_for_game(&entry.game_id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let details = store.get_game(&entry.game_id).await.unwrap().unwrap();
    assert_eq!(details.last_played_at_ms, None);
    // A tool isn't a way to play, and Play's action isn't a tool.
    let as_play = LaunchChoice {
        action: Some(tool.id.clone()),
        ..Default::default()
    };
    assert!(matches!(
        service.launch_with(&entry.id, &as_play).await,
        Err(LaunchError::Invalid(_))
    ));
    assert!(matches!(
        service
            .run_tool(&entry.id, &crate::model::built_in_action_id(&entry.id))
            .await,
        Err(LaunchError::Invalid(_))
    ));
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_missing_action_reserves_no_session() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path()).await;
    let gone = LaunchChoice {
        action: Some("deleted".into()),
        ..Default::default()
    };
    let service = service(&store);
    assert!(matches!(
        service.launch_with(&entry.id, &gone).await,
        Err(LaunchError::Invalid(_))
    ));
    assert!(matches!(
        service.run_tool(&entry.id, "deleted").await,
        Err(LaunchError::Invalid(_))
    ));
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
    // A failing start ends the session it reserved.
    let broken = store
        .save_game_action(&action(
            &entry,
            ActionKind::Play,
            exe(temp.path().join("missing-program"), &[]),
        ))
        .await
        .unwrap();
    let choice = LaunchChoice {
        action: Some(broken.id),
        ..Default::default()
    };
    let Err(error) = service.launch_with(&entry.id, &choice).await else {
        panic!("a missing program can't start");
    };
    assert!(error.to_string().contains("missing-program"), "{error}");
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
    // The game can still play.
    service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
}

#[tokio::test]
async fn a_managed_release_s_tool_moves_with_it() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let first = temp.path().join("first");
    let game_dir = first.join("Game");
    script(&game_dir.join("game.sh"), "exit 0");
    script(
        &game_dir.join("tools/setup tool.sh"),
        "touch \"$(dirname \"$0\")/../ran-$1\"",
    );
    for root in [&first, &temp.path().join("second")] {
        fs::create_dir_all(root).unwrap();
        caskade_installer::manifest::RootManifest::new("fixture-root")
            .unwrap()
            .write(root)
            .unwrap();
    }
    let root = |path: &Path| StorageRoot {
        id: "fixture-root".into(),
        path: path.into(),
        is_default: true,
        label: None,
    };
    store.upsert_root(&root(&first)).await.unwrap();
    let game = Game::new("Managed");
    store.upsert_game(&game).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("caskade-storage"), "m");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::Managed {
        root_id: "fixture-root".into(),
        rel_dir: "Game".into(),
    });
    entry.launch = Some(exe("game.sh", &[]));
    store.upsert_entry(&entry).await.unwrap();
    let mut tool = action(
        &entry,
        ActionKind::Utility,
        exe("tools/setup tool.sh", &["one"]),
    );
    tool = store.save_game_action(&tool).await.unwrap();
    let service = service(&store);
    service.run_tool(&entry.id, &tool.id).await.unwrap();
    wait_for(&game_dir.join("ran-one")).await;

    // The storage root moves; the stored action is unchanged.
    let second = temp.path().join("second");
    fs::rename(&game_dir, second.join("Game")).unwrap();
    store.upsert_root(&root(&second)).await.unwrap();
    let mut again = tool.clone();
    again.target = exe("tools/setup tool.sh", &["two"]);
    store.save_game_action(&again).await.unwrap();
    service.run_tool(&entry.id, &tool.id).await.unwrap();
    wait_for(&second.join("Game/ran-two")).await;
}

#[tokio::test]
async fn a_cancelled_session_keeps_its_action_and_a_stale_process_is_not_reattached() {
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let entry = release(&store, temp.path()).await;
    let game = temp.path().join("longgame");
    crate::test_programs::copy_program("/bin/sleep", &game);
    let mut play = action(&entry, ActionKind::Play, exe(&game, &["30"]));
    play.tracking.mode = TrackingMode::ProcessTree;
    let play = store.save_game_action(&play).await.unwrap();
    let choice = LaunchChoice {
        action: Some(play.id.clone()),
        ..Default::default()
    };
    let first = service(&store);
    let handle = first.launch_with(&entry.id, &choice).await.unwrap();
    let mut open = vec![];
    for _ in 0..200 {
        open = store.in_flight_sessions().await.unwrap();
        if open
            .first()
            .is_some_and(|session| recovery(session).phase == SessionPhase::Playing)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let state = recovery(&open[0]);
    assert_eq!(state.action, Some(play.id.clone()));
    assert_eq!(state.target.mode, TrackingMode::ProcessTree);
    // App shutdown leaves the game and its session for recovery.
    first.shutdown();
    assert!(matches!(handle.wait().await, Err(LaunchError::Cancelled)));
    assert_eq!(store.in_flight_sessions().await.unwrap().len(), 1);
    // The game ends while Caskade is closed.
    for identity in &state.processes {
        std::process::Command::new("kill")
            .args(["-9", &identity.pid.to_string()])
            .status()
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let handles = service(&store).recover().await.unwrap();
    assert!(handles.is_empty());
    assert!(store.in_flight_sessions().await.unwrap().is_empty());
}
