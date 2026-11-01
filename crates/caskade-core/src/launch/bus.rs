use super::{LaunchCommand, LaunchEvent, LaunchHooks, LaunchTicket, Result};
use crate::bus::{Bus, Event, GameSession, InterceptPoint, Outcome};
use async_trait::async_trait;

pub struct LaunchBefore;
impl InterceptPoint for LaunchBefore {
    type Payload = LaunchCommand;
    const NAME: &'static str = "launch.before";
}
pub struct LaunchAfter;
impl InterceptPoint for LaunchAfter {
    type Payload = LaunchEvent;
    const NAME: &'static str = "launch.after";
}

#[async_trait]
impl LaunchHooks for Bus {
    async fn before(&self, launch: &LaunchTicket, command: &mut LaunchCommand) -> Result<bool> {
        match self.run::<LaunchBefore>(command.clone()).await.outcome {
            Outcome::Continue(next) => {
                *command = next;
                self.emit(Event::GameLaunching {
                    game_id: launch.game_id.clone(),
                    entry_id: launch.entry_id.clone(),
                });
                Ok(true)
            }
            // Say who stopped it and why, not just "cancelled" (#160).
            Outcome::Cancelled { by, reason } => {
                Err(super::LaunchError::Refused(if reason.trim().is_empty() {
                    crate::tr!("The plugin “{plugin}” stopped the launch.", plugin = by)
                } else {
                    crate::tr!(
                        "The plugin “{plugin}” stopped the launch: {reason}",
                        plugin = by,
                        reason = reason
                    )
                }))
            }
        }
    }
    async fn started(&self, event: &LaunchEvent) {
        self.emit(Event::GameStarted(session(event)));
    }
    async fn exited(&self, event: &LaunchEvent) {
        self.emit(Event::GameExited(session(event)));
    }
    async fn after(&self, event: &LaunchEvent) {
        // Completion is durable already. After hooks may synchronize saves,
        // but cancelling them cannot undo a game that has finished.
        let _ = self.run::<LaunchAfter>(event.clone()).await;
    }
    async fn scripts(&self, report: &crate::scripts::ScriptReport) {
        self.emit(Event::LifecycleScriptsRan(Box::new(report.clone())));
    }
}
fn session(event: &LaunchEvent) -> GameSession {
    GameSession {
        game_id: event.game_id.clone(),
        entry_id: event.entry_id.clone(),
        session_id: event.session_id.clone(),
        started_at_ms: event.started_at_ms,
        ended_at_ms: event.ended_at_ms,
        duration_ms: event.duration_ms,
    }
}
