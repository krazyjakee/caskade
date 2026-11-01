use super::{LaunchCommand, LaunchEvent, LaunchTicket, Result};
use async_trait::async_trait;

/// Injectable in-process extension points. The bus adapter applies subscriber
/// ordering, timeouts and isolation; the pipeline keeps OS work out of hooks.
#[async_trait]
pub trait LaunchHooks: Send + Sync {
    /// Return false to cancel before a process or session is created.
    async fn before(&self, _launch: &LaunchTicket, _command: &mut LaunchCommand) -> Result<bool> {
        Ok(true)
    }
    async fn started(&self, _event: &LaunchEvent) {}
    async fn exited(&self, _event: &LaunchEvent) {}
    async fn after(&self, _event: &LaunchEvent) {}
    /// Lifecycle scripts ran (#179): their results, output included. Only
    /// Caskade sees them; plugins never do.
    async fn scripts(&self, _report: &crate::scripts::ScriptReport) {}
}
pub struct NoHooks;
#[async_trait]
impl LaunchHooks for NoHooks {}

/// Resolves `LaunchSpec::Plugin` entries to a command. The plugin host's
/// `PluginRunners` implements it; the core itself loads no WASM.
#[async_trait]
pub trait PluginRunner: Send + Sync {
    async fn resolve(
        &self,
        plugin_id: &str,
        runner_id: &str,
        payload: &serde_json::Value,
    ) -> Result<LaunchCommand>;
}
pub struct NoPluginRunner;
#[async_trait]
impl PluginRunner for NoPluginRunner {
    async fn resolve(
        &self,
        plugin_id: &str,
        runner_id: &str,
        _payload: &serde_json::Value,
    ) -> Result<LaunchCommand> {
        Err(super::LaunchError::Unsupported(format!(
            "plugin {plugin_id}/{runner_id}"
        )))
    }
}

/// Gets an entry ready before it resolves, e.g. downloads a RomM ROM into
/// its cache (#1). Runs before the game's lease, so it may take long.
#[async_trait]
pub trait LaunchPreparer: Send + Sync {
    async fn prepare(&self, entry_id: &str) -> Result<()>;
}

/// Whether whoever is playing may start a game (#209: age limits). Asked
/// before anything else a launch does (downloads, runners, emulators).
#[async_trait]
pub trait PlayGate: Send + Sync {
    /// [`super::LaunchError::AgeRestricted`] when the game needs an admin's
    /// approval first.
    async fn admit(&self, game_id: &str) -> Result<()>;
    /// The game started: an approval it used is spent.
    fn started(&self, game_id: &str);
}
