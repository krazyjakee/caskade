//! A launch action from another device (#245) that names a Flatpak app this
//! device doesn't have is refused with the app's name, before anything
//! starts, instead of failing inside `flatpak run`.

use std::time::Duration;

use caskade_core::db::LibraryStore;
use caskade_core::launch::{LaunchChoice, LaunchConfig, LaunchError, LaunchService};
use caskade_core::model::{
    ActionKind, ActionTracking, Game, GameAction, GameEntry, InstallState, LaunchSpec, SourceId,
};

const MISSING: &str = "org.caskade.Test.NotInstalled";

#[tokio::test]
async fn a_synced_flatpak_action_whose_app_is_missing_says_so() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Celeste")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("manual"), "celeste");
    entry.state = InstallState::Installed;
    entry.launch = Some(LaunchSpec::Executable {
        path: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: Default::default(),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    // As it arrives from another device: a Flatpak bound by its app id.
    let action = store
        .save_game_action(&GameAction {
            id: uuid::Uuid::now_v7().to_string(),
            entry_id: entry.id.clone(),
            name: "Play in the Flatpak".into(),
            kind: ActionKind::Play,
            target: LaunchSpec::Flatpak {
                app_id: MISSING.into(),
                args: Vec::new(),
            },
            tracking: ActionTracking::default(),
            proton: None,
        })
        .await
        .unwrap();
    let launches = LaunchService::new(store.clone()).with_config(LaunchConfig {
        poll_interval: Duration::from_millis(20),
        launch_timeout: Duration::from_secs(10),
        exit_grace: Duration::from_millis(100),
        heartbeat_interval: Duration::from_millis(200),
    });
    let choice = LaunchChoice {
        action: Some(action.id.clone()),
        ..LaunchChoice::default()
    };
    match launches.launch_with(&entry.id, &choice).await {
        Err(LaunchError::Refused(message)) => {
            assert!(message.contains(MISSING), "{message}");
        }
        Err(other) => panic!("expected a refusal naming the app, got {other:?}"),
        Ok(_) => panic!("a missing Flatpak app started"),
    }
    // Nothing started: no session.
    assert!(
        store
            .sessions_for_game(&game.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
}
