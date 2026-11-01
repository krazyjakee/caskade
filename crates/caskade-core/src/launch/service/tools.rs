use super::{LaunchService, spawn};
use crate::launch::{LaunchChoice, LaunchError, Result, actions};
use crate::model::ActionKind;
use tokio_util::sync::CancellationToken;

impl LaunchService {
    /// Runs one of the entry's tools (a utility action, #177): no play
    /// session, playtime or last played, and no plugin launch hooks. The
    /// play gate still decides whether it may start. Returns once the tool
    /// has started; it isn't tracked after that.
    pub async fn run_tool(&self, entry_id: &str, action_id: &str) -> Result<()> {
        let entry = self.entry(entry_id).await?;
        let settings = self.repository.action_settings(entry_id).await?;
        let added = self.repository.actions(entry_id).await?;
        let action = actions::entry_actions(&entry, added, &settings)
            .into_iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| {
                LaunchError::Invalid(crate::tr!("that action no longer exists; choose another"))
            })?;
        if action.kind != ActionKind::Utility {
            return Err(LaunchError::Invalid(crate::tr!(
                "“{name}” plays the game: start it with Play",
                name = action.name
            )));
        }
        if let Some(gate) = &self.gate {
            gate.admit(&entry.game_id).await?;
        }
        let mut prepared = self
            .resolve_entry(
                &actions::apply(&entry, &action),
                &LaunchChoice::default(),
                &CancellationToken::new(),
            )
            .await?;
        prepared.command.validate()?;
        prepared.prepare().await?;
        let mut child = spawn(&prepared.command)?;
        if let Some(gate) = &self.gate {
            gate.started(&entry.game_id);
        }
        let game = entry.game_id.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            tracing::info!(game = %game, ?status, "tool exited");
        });
        Ok(())
    }
}
