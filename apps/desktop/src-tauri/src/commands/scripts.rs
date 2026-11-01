//! Lifecycle scripts (#179): listing and changing the global, emulator
//! profile and game scripts, and stopping a launch while its before-launch
//! scripts run. Who may change a scope is `permissions::script_scope`.
use crate::error::{CommandError, report};
use caskade_core::{
    db::LibraryStore,
    launch::LaunchService,
    scripts::{self, LifecycleScript, ScriptScope, ScriptStep, ScriptTest, ScriptTests},
};
use std::sync::LazyLock;
use tauri::State;

/// Tests running now, by the id the webview gave them.
static TESTS: LazyLock<ScriptTests> = LazyLock::new(ScriptTests::default);

/// One scope's scripts in their order, disabled ones included.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_scripts(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
) -> Result<Vec<LifecycleScript>, CommandError> {
    store.lifecycle_scripts(&scope).await.map_err(report)
}

/// Adds or changes a script of `scope`; an empty `id` adds one, last.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_save(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    script: LifecycleScript,
) -> Result<LifecycleScript, CommandError> {
    let mut script = script;
    if script.scope != scope {
        return Err(mismatch());
    }
    if script.id.is_empty() {
        script.id = caskade_core::model::new_id();
    }
    store.save_lifecycle_script(&script).await.map_err(report)
}

#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_delete(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    id: String,
) -> Result<bool, CommandError> {
    match store.lifecycle_script(&id).await.map_err(report)? {
        Some(script) if script.scope == scope => {
            store.delete_lifecycle_script(&id).await.map_err(report)
        }
        Some(_) => Err(mismatch()),
        None => Ok(false),
    }
}

/// Puts a scope's scripts in the order of `ids`.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_scripts_order(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    ids: Vec<String>,
) -> Result<(), CommandError> {
    store
        .order_lifecycle_scripts(&scope, &ids)
        .await
        .map_err(report)
}

/// Stops the game's launch while its before-launch scripts run; false
/// when it isn't at that point.
#[tauri::command]
#[specta::specta]
pub async fn launch_cancel(
    service: State<'_, LaunchService>,
    game_id: String,
) -> Result<bool, CommandError> {
    Ok(service.cancel_start(&game_id))
}

/// The steps a launch of the game (through the profile) takes, in order,
/// disabled scripts where they would run (#180).
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_scripts_effective(
    store: State<'_, LibraryStore>,
    game_id: Option<String>,
    profile_id: Option<String>,
) -> Result<Vec<ScriptStep>, CommandError> {
    scripts::effective_order(&store, game_id.as_deref(), profile_id.as_deref())
        .await
        .map_err(report)
}

/// Runs `script` of `scope` once as a test, saved or not, through the
/// launch's runner (#180): with `game_id`'s context, or with no game's.
/// `test_id` names it for `lifecycle_script_test_cancel`.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_test(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    script: LifecycleScript,
    game_id: Option<String>,
    test_id: String,
) -> Result<ScriptTest, CommandError> {
    if script.scope != scope {
        return Err(mismatch());
    }
    let game_id = match &scope {
        ScriptScope::Game { game_id } => Some(game_id.clone()),
        _ => game_id,
    };
    let context = scripts::test_context(&store, game_id.as_deref())
        .await
        .map_err(report)?;
    TESTS.run(&test_id, &script, &context).await.map_err(report)
}

/// Stops a running test; false when it already finished.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_test_cancel(test_id: String) -> Result<bool, CommandError> {
    Ok(TESTS.cancel(&test_id))
}

fn mismatch() -> CommandError {
    report(caskade_core::Error::Invalid(caskade_core::tr!(
        "the script belongs to another scope"
    )))
}

#[cfg(test)]
#[path = "scripts_tests.rs"]
mod tests;
