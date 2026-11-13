//! Testing a script from the settings, and the order a game's scripts run
//! in (#180). A test runs through the same [`ScriptRunner`] as a launch,
//! with a context marked as a test (`CASKADE_TEST=1`).
use super::{ContextValue, LifecycleScript, ScriptContext, ScriptPhase, ScriptRun, ScriptRunner};
use crate::{
    db::LibraryStore,
    model::{InstallLocation, built_in_action_id},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

/// A test's result: the values the script saw, and its run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScriptTest {
    pub context: Vec<ContextValue>,
    pub run: ScriptRun,
}

/// One step of the order a launch runs scripts in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScriptStep {
    pub phase: ScriptPhase,
    pub script: LifecycleScript,
}

/// The context a test of a script for `game_id` runs with: the game, its
/// default release and that release's folder, its own action. Without a
/// game (a global or profile script), every game value is empty.
pub async fn test_context(
    store: &LibraryStore,
    game_id: Option<&str>,
) -> crate::Result<ScriptContext> {
    let mut context = ScriptContext {
        test: true,
        ..Default::default()
    };
    let Some(game_id) = game_id else {
        return Ok(context);
    };
    let details = store
        .get_game(game_id)
        .await?
        .ok_or_else(|| crate::db::missing("game", game_id))?;
    context.game_id = details.game.id.clone();
    context.game_title = details.game.title.clone();
    let entry = details
        .entries
        .iter()
        .find(|entry| Some(&entry.id) == details.game.default_entry.as_ref())
        .or_else(|| details.entries.iter().find(|entry| entry.launch.is_some()))
        .or(details.entries.first());
    if let Some(entry) = entry {
        context.entry_id = entry.id.clone();
        context.install_dir = match &entry.location {
            Some(InstallLocation::External { path }) => Some(path.clone()),
            Some(InstallLocation::Managed { root_id, rel_dir }) => store
                .get_root(root_id)
                .await?
                .map(|root| root.path.join(rel_dir)),
            None => None,
        };
        if entry.launch.is_some() {
            context.action_id = Some(built_in_action_id(&entry.id));
        }
        if let Some(crate::model::LaunchSpec::Emulator { platform_id, .. }) = &entry.launch {
            context.platform_id = Some(platform_id.clone());
        }
    }
    Ok(context)
}

/// Runs `script` once as a test: checked as a save would check it, then
/// through the launch's runner. Saving isn't needed.
pub async fn test(
    runner: &ScriptRunner,
    script: &LifecycleScript,
    context: &ScriptContext,
    cancel: &CancellationToken,
) -> crate::Result<ScriptTest> {
    let mut script = script.clone();
    // An unsaved script has no id yet.
    if script.id.is_empty() {
        script.id = "test".into();
    }
    let script = script.normalized()?;
    Ok(ScriptTest {
        context: context.values(script.phase),
        run: runner.run(&script, context, cancel).await,
    })
}

/// Tests running now, by the id their caller gave them, so they can be
/// cancelled from elsewhere.
#[derive(Default)]
pub struct ScriptTests {
    runner: ScriptRunner,
    running: std::sync::Mutex<std::collections::BTreeMap<String, CancellationToken>>,
}
impl ScriptTests {
    pub fn new(runner: ScriptRunner) -> Self {
        Self {
            runner,
            running: Default::default(),
        }
    }

    /// [`test`] under `id`, until it ends.
    pub async fn run(
        &self,
        id: &str,
        script: &LifecycleScript,
        context: &ScriptContext,
    ) -> crate::Result<ScriptTest> {
        let cancel = CancellationToken::new();
        crate::lock(&self.running).insert(id.into(), cancel.clone());
        let result = test(&self.runner, script, context, &cancel).await;
        crate::lock(&self.running).remove(id);
        result
    }

    /// Stops the test `id`; false when it isn't running.
    pub fn cancel(&self, id: &str) -> bool {
        match crate::lock(&self.running).get(id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }
}

/// The steps a launch of `game_id` (through `profile_id`) would take, in
/// order, disabled scripts included where they would be.
pub async fn effective_order(
    store: &LibraryStore,
    game_id: Option<&str>,
    profile_id: Option<&str>,
) -> crate::Result<Vec<ScriptStep>> {
    let set = store.scripts_for_launch(game_id, profile_id).await?;
    Ok([
        ScriptPhase::BeforeLaunch,
        ScriptPhase::AfterStart,
        ScriptPhase::AfterExit,
    ]
    .into_iter()
    .flat_map(|phase| {
        set.ordered_all(phase)
            .into_iter()
            .map(move |script| ScriptStep {
                phase,
                script: script.clone(),
            })
    })
    .collect())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        model::{Game, GameEntry, LaunchSpec, SourceId},
        scripts::{ScriptCommand, ScriptScope, ScriptStatus, Shell},
    };
    use caskade_discovery::InstallState;

    fn shell(scope: ScriptScope, phase: ScriptPhase, name: &str, text: &str) -> LifecycleScript {
        let mut script = LifecycleScript::new(
            scope,
            phase,
            ScriptCommand::Shell {
                shell: Shell::Sh,
                script: text.into(),
            },
        );
        script.name = name.into();
        script
    }

    #[tokio::test]
    async fn a_test_runs_unsaved_with_the_game_s_context_and_says_so() {
        let temp = tempfile::tempdir().unwrap();
        let store = LibraryStore::in_memory().await.unwrap();
        let game = store.upsert_game(&Game::new("Tested")).await.unwrap();
        let mut entry = GameEntry::new(&game.id, SourceId::new("fixture"), "t");
        entry.state = InstallState::Installed;
        entry.location = Some(InstallLocation::External {
            path: temp.path().into(),
        });
        entry.launch = Some(LaunchSpec::Uri {
            uri: "steam://rungameid/1".into(),
        });
        let entry = store.upsert_entry(&entry).await.unwrap();
        let context = test_context(&store, Some(&game.id)).await.unwrap();
        assert!(context.test);
        assert_eq!(context.entry_id, entry.id);
        assert_eq!(context.install_dir.as_deref(), Some(temp.path()));
        let script = shell(
            ScriptScope::Game {
                game_id: game.id.clone(),
            },
            ScriptPhase::BeforeLaunch,
            "Echo",
            "printf '%s|%s' \"$CASKADE_GAME_TITLE\" \"$CASKADE_TEST\"",
        );
        let result = test(
            &ScriptRunner::default(),
            &script,
            &context,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.run.status, ScriptStatus::Succeeded);
        assert_eq!(result.run.stdout, "Tested|1");
        assert!(
            result
                .context
                .iter()
                .any(|value| value.placeholder == "{GameTitle}" && value.value == "Tested")
        );
        // Nothing was saved, and an invalid script isn't run.
        assert!(
            store
                .lifecycle_scripts(&script.scope)
                .await
                .unwrap()
                .is_empty()
        );
        let mut invalid = script.clone();
        invalid.timeout_secs = 0;
        assert!(
            test(
                &ScriptRunner::default(),
                &invalid,
                &context,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
        // Without a game, the game's values are empty.
        let bare = test_context(&store, None).await.unwrap();
        assert!(bare.test && bare.game_id.is_empty());
    }

    #[tokio::test]
    async fn the_effective_order_lists_every_scope_with_disabled_steps_in_place() {
        let store = LibraryStore::in_memory().await.unwrap();
        let game = store.upsert_game(&Game::new("Ordered")).await.unwrap();
        let game_scope = ScriptScope::Game {
            game_id: game.id.clone(),
        };
        for (scope, phase, name, enabled) in [
            (
                ScriptScope::Global,
                ScriptPhase::BeforeLaunch,
                "global",
                true,
            ),
            (
                game_scope.clone(),
                ScriptPhase::BeforeLaunch,
                "game off",
                false,
            ),
            (
                game_scope.clone(),
                ScriptPhase::AfterExit,
                "game exit",
                true,
            ),
            (
                ScriptScope::Global,
                ScriptPhase::AfterExit,
                "global exit",
                true,
            ),
        ] {
            let mut script = shell(scope, phase, name, "true");
            script.enabled = enabled;
            store.save_lifecycle_script(&script).await.unwrap();
        }
        let steps = effective_order(&store, Some(&game.id), None).await.unwrap();
        let named: Vec<_> = steps
            .iter()
            .map(|step| (step.phase, step.script.name.as_str(), step.script.enabled))
            .collect();
        assert_eq!(
            named,
            [
                (ScriptPhase::BeforeLaunch, "global", true),
                (ScriptPhase::BeforeLaunch, "game off", false),
                (ScriptPhase::AfterExit, "game exit", true),
                (ScriptPhase::AfterExit, "global exit", true),
            ]
        );
    }
}
