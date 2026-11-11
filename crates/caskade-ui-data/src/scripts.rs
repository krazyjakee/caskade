//! Lifecycle scripts (#179, #180, #105): one scope's scripts in their
//! order, adding, changing, deleting and ordering them, a test run through
//! the launch's own runner, and the order a launch takes across scopes,
//! through the app layer's commands as the webview's `api/scripts.ts` does.
//! Who may change a scope is the adapter's rule, applied here too:
//! `caskade_app::permissions::script_scope`.

use caskade_app::commands::scripts;
use caskade_core::accounts::AccountService;
use caskade_core::db::LibraryStore;
use caskade_core::error::ErrorReport;
use serde_json::json;

/// The script types flows show.
pub use caskade_core::scripts::{
    ContextValue, LifecycleScript, ScriptCommand, ScriptPhase, ScriptRun, ScriptScope,
    ScriptStatus, ScriptStep, ScriptTest, Shell,
};

type Result<T> = std::result::Result<T, ErrorReport>;

/// What scripts panels read and change, over one app's library.
#[derive(Clone)]
pub struct Scripts {
    store: LibraryStore,
    accounts: AccountService,
}

impl Scripts {
    pub fn new(store: LibraryStore, accounts: AccountService) -> Self {
        Self { store, accounts }
    }

    /// The running app's.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(services.store.clone(), services.accounts.clone())
    }

    /// Whether the signed-in account may call `command` for `scope`.
    fn allowed(&self, command: &str, scope: &ScriptScope) -> Result<()> {
        caskade_app::permissions::check(
            command,
            &json!({ "scope": scope }),
            Some(&self.accounts),
            None,
        )
        .map_err(ErrorReport::from)
    }

    /// `scope`'s scripts in their order, disabled ones included.
    pub async fn list(&self, scope: &ScriptScope) -> Result<Vec<LifecycleScript>> {
        scripts::lifecycle_scripts(&self.store, scope).await
    }

    /// Adds (an empty id) or changes a script of `scope`.
    pub async fn save(
        &self,
        scope: &ScriptScope,
        script: LifecycleScript,
    ) -> Result<LifecycleScript> {
        self.allowed("lifecycle_script_save", scope)?;
        scripts::lifecycle_script_save(&self.store, scope, script).await
    }

    pub async fn delete(&self, scope: &ScriptScope, id: &str) -> Result<bool> {
        self.allowed("lifecycle_script_delete", scope)?;
        scripts::lifecycle_script_delete(&self.store, scope, id).await
    }

    /// Puts `scope`'s scripts in the order of `ids`.
    pub async fn order(&self, scope: &ScriptScope, ids: &[String]) -> Result<()> {
        self.allowed("lifecycle_scripts_order", scope)?;
        scripts::lifecycle_scripts_order(&self.store, scope, ids).await
    }

    /// The steps a launch of `game_id` (through `profile_id`) takes.
    pub async fn effective(
        &self,
        game_id: Option<&str>,
        profile_id: Option<&str>,
    ) -> Result<Vec<ScriptStep>> {
        scripts::lifecycle_scripts_effective(&self.store, game_id, profile_id).await
    }

    /// Runs `script` once now, as a launch would, without a game starting;
    /// `test_id` names it for [`Self::cancel_test`].
    pub async fn test(
        &self,
        scope: &ScriptScope,
        script: &LifecycleScript,
        game_id: Option<String>,
        test_id: &str,
    ) -> Result<ScriptTest> {
        self.allowed("lifecycle_script_test", scope)?;
        scripts::lifecycle_script_test(&self.store, scope, script, game_id, test_id).await
    }

    /// Stops a running test; false when it already finished.
    pub async fn cancel_test(&self, test_id: &str) -> Result<bool> {
        scripts::lifecycle_script_test_cancel(test_id).await
    }
}

impl PartialEq for Scripts {
    /// One library per app: a flow's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
