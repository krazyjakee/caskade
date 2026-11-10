//! Lifecycle scripts (#179): listing and changing the global, emulator
//! profile and game scripts, and stopping a launch while its before-launch
//! scripts run. Who may change a scope is
//! `caskade_app::permissions::script_scope`. Forwarded to
//! `caskade_app::commands::scripts`.
use crate::error::CommandError;
use caskade_app::commands::scripts as app;
use caskade_core::{
    db::LibraryStore,
    launch::LaunchService,
    scripts::{LifecycleScript, ScriptScope, ScriptStep, ScriptTest},
};
use tauri::State;

/// One scope's scripts in their order, disabled ones included.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_scripts(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
) -> Result<Vec<LifecycleScript>, CommandError> {
    app::lifecycle_scripts(&store, &scope).await
}

/// Adds or changes a script of `scope`; an empty `id` adds one, last.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_save(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    script: LifecycleScript,
) -> Result<LifecycleScript, CommandError> {
    app::lifecycle_script_save(&store, &scope, script).await
}

#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_delete(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    id: String,
) -> Result<bool, CommandError> {
    app::lifecycle_script_delete(&store, &scope, &id).await
}

/// Puts a scope's scripts in the order of `ids`.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_scripts_order(
    store: State<'_, LibraryStore>,
    scope: ScriptScope,
    ids: Vec<String>,
) -> Result<(), CommandError> {
    app::lifecycle_scripts_order(&store, &scope, &ids).await
}

/// Stops the game's launch while its before-launch scripts run; false
/// when it isn't at that point.
#[tauri::command]
#[specta::specta]
pub async fn launch_cancel(
    service: State<'_, LaunchService>,
    game_id: String,
) -> Result<bool, CommandError> {
    app::launch_cancel(&service, &game_id).await
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
    app::lifecycle_scripts_effective(&store, game_id.as_deref(), profile_id.as_deref()).await
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
    app::lifecycle_script_test(&store, &scope, &script, game_id, &test_id).await
}

/// Stops a running test; false when it already finished.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_script_test_cancel(test_id: String) -> Result<bool, CommandError> {
    app::lifecycle_script_test_cancel(&test_id).await
}

#[cfg(test)]
#[path = "scripts_tests.rs"]
mod tests;
