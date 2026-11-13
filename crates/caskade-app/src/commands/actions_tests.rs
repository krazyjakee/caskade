//! Launch actions: add, list, default, order, run a tool.
use super::*;
use crate::commands::launch::{LaunchOutcome, launch_game_with};
use crate::error::ErrorCode;
use caskade_core::{
    launch::LaunchChoice,
    model::{Game, GameEntry, InstallLocation, InstallState, LaunchSpec, SourceId},
    test_programs::write_program,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};

/// Waits up to five seconds for `path` to appear.
async fn appears(path: &Path) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !path.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    path.exists()
}

#[tokio::test(flavor = "multi_thread")]
async fn actions_are_added_chosen_ordered_and_run() {
    let temp = tempfile::tempdir().unwrap();
    let tool = temp.path().join("configure.sh");
    let ran = temp.path().join("ran");
    write_program(&tool, format!("#!/bin/sh\ntouch '{}'\n", ran.display()));
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Actions")).await.unwrap();
    let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "ipc");
    entry.state = InstallState::Installed;
    entry.location = Some(InstallLocation::External {
        path: temp.path().into(),
    });
    entry.launch = Some(LaunchSpec::Executable {
        path: temp.path().join("game"),
        args: vec![],
        cwd: None,
        env: BTreeMap::new(),
    });
    let entry = store.upsert_entry(&entry).await.unwrap();
    let bus = Bus::default();
    let mut events = bus.stream();
    let launch = LaunchService::new(store.clone()).with_aliases(None);

    let listed = game_actions(&store, &game.id).await.unwrap();
    assert_eq!(listed[0].entry_id, entry.id);
    assert!(!listed[0].editable_built_in);
    assert_eq!(listed[0].actions[0].name, "Play");
    let action = |name: &str, kind: &str| -> GameAction {
        serde_json::from_value(json!({
            "id": "", "entryId": entry.id, "name": name, "kind": kind,
            "target": {"type": "executable", "path": tool, "args": ["a b"], "cwd": null, "env": {}},
            "tracking": {"mode": "automatic", "folder": null, "names": []},
        }))
        .unwrap()
    };
    let configure = game_action_save(&store, &bus, &game.id, action("Configure", "utility"))
        .await
        .unwrap();
    let modded = game_action_save(&store, &bus, &game.id, action("Modded", "play"))
        .await
        .unwrap();
    assert!(matches!(
        events.try_recv(),
        Ok(Event::LibraryGameUpdated { game_id }) if game_id == game.id
    ));
    let refused = game_action_save(&store, &bus, &game.id, action("", "play"))
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Invalid);

    game_actions_order(
        &store,
        &bus,
        &game.id,
        &entry.id,
        &[modded.id.clone(), configure.id.clone()],
    )
    .await
    .unwrap();
    let settings: EntryActionSettings =
        serde_json::from_value(json!({"defaultAction": modded.id, "ask": true, "builtIn": {}}))
            .unwrap();
    let settings = game_action_settings_save(&store, &bus, &game.id, &entry.id, settings)
        .await
        .unwrap();
    assert_eq!(settings.default_action.as_ref(), Some(&modded.id));
    let listed = game_actions(&store, &game.id).await.unwrap();
    let names: Vec<_> = listed[0]
        .actions
        .iter()
        .map(|action| action.name.as_str())
        .collect();
    assert_eq!(names, ["Play", "Modded", "Configure"]);
    // Another game's release can't be changed through this one.
    let other: EntryActionSettings =
        serde_json::from_value(json!({"ask": false, "builtIn": {}})).unwrap();
    let elsewhere = game_action_settings_save(&store, &bus, "other", &entry.id, other)
        .await
        .unwrap_err();
    assert_eq!(elsewhere.code, ErrorCode::Invalid);

    let outcome = game_tool_run(&launch, &entry.id, &configure.id)
        .await
        .unwrap();
    assert!(matches!(outcome, ToolOutcome::Started), "{outcome:?}");
    assert!(appears(&ran).await);
    let sessions = store.sessions_for_game(&game.id, 10).await.unwrap();
    assert!(sessions.is_empty());

    // An action-only launch: Play starts the chosen action's program, with
    // its arguments as given, in a session of the release.
    let played = temp.path().join("played");
    let game_script = temp.path().join("modded.sh");
    write_program(
        &game_script,
        format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", played.display()),
    );
    let mut changed: Value = serde_json::to_value(&modded).unwrap();
    changed["target"]["path"] = json!(game_script);
    changed["tracking"]["mode"] = json!("original_process");
    let changed: GameAction = serde_json::from_value(changed).unwrap();
    game_action_save(&store, &bus, &game.id, changed)
        .await
        .unwrap();
    let started = launch_game_with(
        &launch,
        &entry.id,
        Some(LaunchChoice {
            action: Some(modded.id.clone()),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let LaunchOutcome::Started { ticket } = started else {
        panic!("{started:?}");
    };
    assert_eq!(ticket.entry_id, entry.id);
    assert!(appears(&played).await);
    assert_eq!(std::fs::read_to_string(&played).unwrap(), "a b");

    assert!(
        game_action_delete(&store, &bus, &game.id, &modded.id)
            .await
            .unwrap()
    );
}
