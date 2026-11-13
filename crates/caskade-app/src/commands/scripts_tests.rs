//! Lifecycle scripts through the commands; who may change each scope is
//! `crate::permissions::script_scope`.
use super::*;
use crate::error::ErrorCode;
use caskade_core::model::Game;
#[cfg(unix)]
use caskade_core::scripts::{ScriptPhase, ScriptStatus};
use serde_json::json;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// An unsaved shell script of `scope` that runs `text`.
fn script(scope: &ScriptScope, name: &str, text: &str) -> LifecycleScript {
    serde_json::from_value(json!({
        "id": "", "scope": scope, "name": name, "enabled": true, "phase": "before_launch",
        "command": {"type": "shell", "shell": "sh", "script": text},
        "cwd": null, "timeoutSecs": 30,
    }))
    .unwrap()
}

#[tokio::test]
async fn scripts_are_saved_ordered_and_deleted_within_their_scope() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Scripted")).await.unwrap();
    let scope = ScriptScope::Game {
        game_id: game.id.clone(),
    };
    async fn save(
        store: &LibraryStore,
        name: &str,
        scope: &ScriptScope,
    ) -> Result<LifecycleScript, CommandError> {
        lifecycle_script_save(
            store,
            scope,
            script(scope, name, "echo \"$CASKADE_GAME_TITLE\""),
        )
        .await
    }
    let first = save(&store, "Mount", &scope).await.unwrap();
    let second = save(&store, "VPN", &scope).await.unwrap();
    assert!(!first.id.is_empty());
    // A script names the scope it is saved in.
    let other = ScriptScope::Global;
    let refused = lifecycle_script_save(&store, &other, first.clone())
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Invalid);
    lifecycle_scripts_order(&store, &scope, &[second.id.clone(), first.id.clone()])
        .await
        .unwrap();
    let listed = lifecycle_scripts(&store, &scope).await.unwrap();
    assert_eq!(listed[0].name, "VPN");
    assert_eq!(listed[1].name, "Mount");
    assert!(
        lifecycle_script_delete(&store, &other, &first.id)
            .await
            .is_err()
    );
    assert!(
        lifecycle_script_delete(&store, &scope, &first.id)
            .await
            .unwrap()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_test_runs_with_the_game_s_context_shows_its_output_and_can_be_cancelled() {
    let store = LibraryStore::in_memory().await.unwrap();
    let game = store.upsert_game(&Game::new("Tested")).await.unwrap();
    let scope = ScriptScope::Game {
        game_id: game.id.clone(),
    };
    // Tested unsaved: the game's values, its output, and how it ended.
    let tested = lifecycle_script_test(
        &store,
        &scope,
        &script(
            &scope,
            "Echo",
            "printf '%s' \"$CASKADE_GAME_TITLE\"; echo oops >&2; exit 3",
        ),
        None,
        "one",
    )
    .await
    .unwrap();
    assert_eq!(tested.run.status, ScriptStatus::Failed);
    assert_eq!(tested.run.exit_code, Some(3));
    assert_eq!(tested.run.stdout, "Tested");
    assert_eq!(tested.run.stderr, "oops\n");
    assert!(
        tested
            .context
            .iter()
            .any(|value| value.variable == "CASKADE_TEST" && value.value == "1")
    );
    // A slow one is cancelled from another call.
    let slow = tokio::spawn({
        let store = store.clone();
        let scope = scope.clone();
        let script = script(&scope, "Slow", "sleep 30");
        async move { lifecycle_script_test(&store, &scope, &script, None, "slow").await }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !lifecycle_script_test_cancel("slow").await.unwrap() {
        assert!(Instant::now() < deadline, "the test never started");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        slow.await.unwrap().unwrap().run.status,
        ScriptStatus::Cancelled
    );
    // The order a launch would take, including a disabled step.
    let mut off = script(&scope, "Off", "true");
    off.enabled = false;
    lifecycle_script_save(&store, &scope, off).await.unwrap();
    let steps = lifecycle_scripts_effective(&store, Some(&game.id), None)
        .await
        .unwrap();
    assert_eq!(steps[0].phase, ScriptPhase::BeforeLaunch);
    assert_eq!(steps[0].script.name, "Off");
    assert!(!steps[0].script.enabled);
}
