//! What runs around a session besides the game: the user's lifecycle
//! scripts (#179) and the emulator profile's own commands.
use super::{LaunchService, event, recovery};
use crate::{
    launch::SessionStatus,
    model::PlaySession,
    scripts::{ScriptContext, ScriptPhase, ScriptReport, ScriptSet},
};
use std::{
    collections::BTreeMap,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

impl LaunchService {
    /// A finished session: `game.exited`, the profile's `after_exit`
    /// command, the after-exit scripts, then `launch.after`. In the
    /// background, all but `game.exited` run on their own task.
    pub(super) async fn ended(
        &self,
        session: &PlaySession,
        status: SessionStatus,
        background: bool,
    ) {
        let event = event(session, status);
        self.hooks.exited(&event).await;
        let scripts = recovery(session).and_then(|state| state.scripts);
        let mut context = scripts.map(|state| state.context);
        if let Some(context) = &mut context {
            context.status = Some(status_name(status));
            context.duration_ms = Some(session.duration_ms);
        }
        let service = self.clone();
        let rest = async move {
            if let Some(command) = &event.command
                && let Some(after) = command
                    .emulation
                    .as_ref()
                    .and_then(|emulation| emulation.after_exit.as_deref())
            {
                // The game has ended; a failed after-exit command is logged
                // and can't undo it.
                let _ = profile_hook("after_exit", after, command.cwd.as_deref()).await;
            }
            if let Some(context) = context {
                service.after_exit(&context).await;
            }
            service.hooks.after(&event).await;
        };
        if background {
            tokio::spawn(rest);
        } else {
            rest.await;
        }
    }

    /// Runs the after-exit scripts the game has now, and reports them.
    async fn after_exit(&self, context: &ScriptContext) {
        match self
            .repository
            .scripts(&context.game_id, context.profile_id.as_deref())
            .await
        {
            Ok(scripts) => self.cleanup(&scripts, context, "").await,
            Err(error) => {
                tracing::warn!(game = %context.game_id, %error, "after-exit scripts not loaded");
            }
        }
    }

    /// The after-start scripts, beside the game: they never hold its
    /// tracking up, and app shutdown (`cancel`) stops them.
    pub(super) fn after_start(&self, context: ScriptContext, cancel: CancellationToken) {
        let service = self.clone();
        tokio::spawn(async move {
            match service
                .repository
                .scripts(&context.game_id, context.profile_id.as_deref())
                .await
            {
                Ok(scripts) => {
                    let report = service
                        .script_runner
                        .run_phase(&scripts, ScriptPhase::AfterStart, &context, &cancel)
                        .await;
                    service.report(&report).await;
                }
                Err(error) => {
                    tracing::warn!(game = %context.game_id, %error, "after-start scripts not loaded");
                }
            }
        });
    }

    /// The after-exit scripts: once a session ends, or once a launch that
    /// passed its before-launch scripts doesn't start (`status` says why).
    pub(super) async fn cleanup(&self, scripts: &ScriptSet, context: &ScriptContext, status: &str) {
        let mut context = context.clone();
        if !status.is_empty() {
            context.status = Some(status.into());
        }
        let report = self
            .script_runner
            .run_phase(
                scripts,
                ScriptPhase::AfterExit,
                &context,
                &CancellationToken::new(),
            )
            .await;
        self.report(&report).await;
    }

    pub(super) async fn report(&self, report: &ScriptReport) {
        if !report.runs.is_empty() {
            self.hooks.scripts(report).await;
        }
    }

    /// While `game` runs its before-launch scripts, [`Self::cancel_start`]
    /// stops them and the launch.
    pub(super) fn starting(&self, game: &str) -> Starting {
        let cancel = CancellationToken::new();
        crate::lock(&self.starting).insert(game.into(), cancel.clone());
        Starting {
            game: game.into(),
            starting: Arc::clone(&self.starting),
            cancel,
        }
    }

    /// Stops `game_id`'s launch while its before-launch scripts run; false
    /// when it isn't at that point.
    pub fn cancel_start(&self, game_id: &str) -> bool {
        match crate::lock(&self.starting).get(game_id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }
}

/// A game running its before-launch scripts; dropping it ends that.
pub(super) struct Starting {
    game: String,
    starting: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
    pub(super) cancel: CancellationToken,
}
impl Drop for Starting {
    fn drop(&mut self) {
        if let Ok(mut starting) = self.starting.lock() {
            starting.remove(&self.game);
        }
    }
}

fn status_name(status: SessionStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Profile commands are bounded so a stuck script can't hold a launch.
const PROFILE_HOOK_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs a profile's `before`/`after_exit` argv without a shell. Logs the
/// program's file name and the outcome, never arguments or environment.
/// A failure says what happened, to follow "the command “name”".
pub(super) async fn profile_hook(
    which: &str,
    argv: &[String],
    cwd: Option<&Path>,
) -> std::result::Result<(), String> {
    let Some((program, args)) = argv.split_first() else {
        return Ok(());
    };
    let name = Path::new(program)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let status = match command.spawn() {
        Ok(mut child) => tokio::time::timeout(PROFILE_HOOK_TIMEOUT, child.wait()).await,
        Err(error) => {
            tracing::warn!(hook = which, program = %name, %error, "profile command failed to start");
            return Err(crate::tr!(
                "“{program}” couldn't start ({error})",
                program = name,
                error = error
            ));
        }
    };
    match status {
        Ok(Ok(status)) if status.success() => {
            tracing::info!(hook = which, program = %name, "profile command finished");
            Ok(())
        }
        Ok(Ok(status)) => {
            tracing::warn!(hook = which, program = %name, code = ?status.code(), "profile command failed");
            Err(match status.code() {
                Some(code) => crate::tr!(
                    "“{program}” failed (exit code {code})",
                    program = name,
                    code = code
                ),
                None => crate::tr!("“{program}” was stopped by a signal", program = name),
            })
        }
        Ok(Err(error)) => {
            tracing::warn!(hook = which, program = %name, %error, "profile command failed");
            Err(crate::tr!(
                "“{program}” failed ({error})",
                program = name,
                error = error
            ))
        }
        Err(_) => {
            tracing::warn!(hook = which, program = %name, "profile command timed out");
            Err(crate::tr!(
                "“{program}” didn't finish within {seconds} seconds",
                program = name,
                seconds = PROFILE_HOOK_TIMEOUT.as_secs()
            ))
        }
    }
}
