//! Lifecycle scripts (#179): listing and changing the global, emulator
//! profile and game scripts, and testing one. Who may change a scope is
//! [`crate::permissions::script_scope`].
use caskade_core::{
    db::LibraryStore,
    scripts::{self, LifecycleScript, ScriptScope, ScriptStep, ScriptTest, ScriptTests},
};
use std::sync::LazyLock;

use crate::error::{CommandError, report};

/// Tests running now, by the id the UI gave them.
static TESTS: LazyLock<ScriptTests> = LazyLock::new(ScriptTests::default);

/// One scope's scripts in their order, disabled ones included.
pub async fn lifecycle_scripts(
    store: &LibraryStore,
    scope: &ScriptScope,
) -> Result<Vec<LifecycleScript>, CommandError> {
    store.lifecycle_scripts(scope).await.map_err(report)
}

/// Adds or changes a script of `scope`; an empty `id` adds one, last.
pub async fn lifecycle_script_save(
    store: &LibraryStore,
    scope: &ScriptScope,
    script: LifecycleScript,
) -> Result<LifecycleScript, CommandError> {
    crate::permissions::enforce(
        "lifecycle_script_save",
        || serde_json::json!({ "scope": scope }),
    )?;
    let mut script = script;
    if script.scope != *scope {
        return Err(mismatch());
    }
    if script.id.is_empty() {
        script.id = caskade_core::model::new_id();
    }
    store.save_lifecycle_script(&script).await.map_err(report)
}

/// Deletes a script of `scope`; false when it's already gone.
pub async fn lifecycle_script_delete(
    store: &LibraryStore,
    scope: &ScriptScope,
    id: &str,
) -> Result<bool, CommandError> {
    crate::permissions::enforce(
        "lifecycle_script_delete",
        || serde_json::json!({ "scope": scope }),
    )?;
    match store.lifecycle_script(id).await.map_err(report)? {
        Some(script) if script.scope == *scope => {
            store.delete_lifecycle_script(id).await.map_err(report)
        }
        Some(_) => Err(mismatch()),
        None => Ok(false),
    }
}

/// Puts a scope's scripts in the order of `ids`.
pub async fn lifecycle_scripts_order(
    store: &LibraryStore,
    scope: &ScriptScope,
    ids: &[String],
) -> Result<(), CommandError> {
    crate::permissions::enforce(
        "lifecycle_scripts_order",
        || serde_json::json!({ "scope": scope }),
    )?;
    store
        .order_lifecycle_scripts(scope, ids)
        .await
        .map_err(report)
}

/// The steps a launch of the game (through the profile) takes, in order,
/// disabled scripts where they would run (#180).
pub async fn lifecycle_scripts_effective(
    store: &LibraryStore,
    game_id: Option<&str>,
    profile_id: Option<&str>,
) -> Result<Vec<ScriptStep>, CommandError> {
    scripts::effective_order(store, game_id, profile_id)
        .await
        .map_err(report)
}

/// Runs `script` of `scope` once as a test, saved or not, through the
/// launch's runner (#180): with `game_id`'s context, or with no game's.
/// `test_id` names it for [`lifecycle_script_test_cancel`].
pub async fn lifecycle_script_test(
    store: &LibraryStore,
    scope: &ScriptScope,
    script: &LifecycleScript,
    game_id: Option<String>,
    test_id: &str,
) -> Result<ScriptTest, CommandError> {
    crate::permissions::enforce(
        "lifecycle_script_test",
        || serde_json::json!({ "scope": scope }),
    )?;
    if script.scope != *scope {
        return Err(mismatch());
    }
    let game_id = match scope {
        ScriptScope::Game { game_id } => Some(game_id.clone()),
        _ => game_id,
    };
    let context = scripts::test_context(store, game_id.as_deref())
        .await
        .map_err(report)?;
    TESTS.run(test_id, script, &context).await.map_err(report)
}

/// Stops a running test; false when it already finished.
pub async fn lifecycle_script_test_cancel(test_id: &str) -> Result<bool, CommandError> {
    crate::permissions::enforce("lifecycle_script_test_cancel", serde_json::Value::default)?;
    Ok(TESTS.cancel(test_id))
}

fn mismatch() -> CommandError {
    report(caskade_core::Error::Invalid(caskade_core::tr!(
        "the script belongs to another scope"
    )))
}
