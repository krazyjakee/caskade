//! Lifecycle scripts (#179): commands the user sets to run before a game
//! launches, once its process has started and after it exits, globally,
//! per emulator profile and per game.
//!
//! Scripts run in the launch pipeline (`crate::launch`), beside the
//! emulator profile's own commands, not as bus interceptors: they need the
//! launch's context (release, action, ROM, install folder, the observed
//! process), which plugin payloads and frontend events never carry, and
//! they must run once per lifecycle across restarts, which the session's
//! recovery state records. Their order relative to plugins is fixed
//! (`docs/TECH-SPEC.md`, the launch pipeline).
//!
//! Values reach a script as data: as environment variables, and in a
//! direct command as `{Placeholder}`s expanded inside one argument each.
//! A shell script is never edited; it reads the variables.
mod exec;
mod testing;
pub use exec::{OUTPUT_LIMIT, ProcessExecutor, ScriptExecutor, ScriptRequest, find_program};
pub use testing::{ScriptStep, ScriptTest, ScriptTests, effective_order, test, test_context};

use crate::model::validation as v;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// When a script runs.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPhase {
    /// Before the game starts. A failure stops the launch.
    BeforeLaunch,
    /// Once the game's process has been found.
    AfterStart,
    /// After the game exits, or after a launch that got past the
    /// before-launch scripts and then failed: cleanup.
    AfterExit,
}
impl ScriptPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeforeLaunch => "before_launch",
            Self::AfterStart => "after_start",
            Self::AfterExit => "after_exit",
        }
    }
}

/// Which launches a script belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ScriptScope {
    /// Every game's play launches.
    Global,
    /// Launches through this emulator profile.
    Profile { profile_id: String },
    /// This game's play launches, whichever release or action.
    Game { game_id: String },
}
impl ScriptScope {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Profile { .. } => "profile",
            Self::Game { .. } => "game",
        }
    }
}

/// The shells a script can be written for. `powershell` and `cmd` exist
/// only on Windows; `pwsh` (PowerShell 7) wherever it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    Sh,
    Bash,
    Pwsh,
    Powershell,
    Cmd,
}
impl Shell {
    /// The shells this OS has; others are refused when saved.
    pub fn available() -> &'static [Shell] {
        if cfg!(windows) {
            &[
                Self::Powershell,
                Self::Pwsh,
                Self::Cmd,
                Self::Bash,
                Self::Sh,
            ]
        } else {
            &[Self::Sh, Self::Bash, Self::Pwsh]
        }
    }
    fn program(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::Pwsh => "pwsh",
            Self::Powershell => "powershell",
            Self::Cmd => "cmd",
        }
    }
    /// The arguments that run `script`, and whether they go onto the
    /// command line verbatim ([`ScriptRequest::verbatim_args`]).
    fn args(self, script: &str) -> (Vec<String>, bool) {
        let flags: &[&str] = match self {
            Self::Sh | Self::Bash => &["-c"],
            Self::Pwsh | Self::Powershell => &["-NoProfile", "-NonInteractive", "-Command"],
            // cmd reads its own command line, not argv: the usual escaping
            // (`\"`) reaches it as text (#271). With /s it drops the first
            // and the last quote after /c and runs what is between as typed.
            Self::Cmd => {
                let args = vec![
                    "/d".into(),
                    "/s".into(),
                    "/c".into(),
                    format!("\"{script}\""),
                ];
                return (args, true);
            }
        };
        let args = flags
            .iter()
            .map(|flag| (*flag).to_owned())
            .chain([script.to_owned()])
            .collect();
        (args, false)
    }
}

/// What a script runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ScriptCommand {
    /// A program (or a script file) and its arguments, started directly
    /// with no shell. `{Placeholder}`s expand inside one argument each.
    Direct { program: PathBuf, args: Vec<String> },
    /// Script text for a shell. The context reaches it only through
    /// `CASKADE_*` environment variables, never pasted into the text.
    Shell { shell: Shell, script: String },
}

pub const DEFAULT_TIMEOUT_SECS: u32 = 60;
pub const MAX_TIMEOUT_SECS: u32 = 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleScript {
    pub id: String,
    pub scope: ScriptScope,
    pub name: String,
    pub enabled: bool,
    pub phase: ScriptPhase,
    pub command: ScriptCommand,
    /// Where it runs; placeholders expand. Unset: the install folder, when
    /// the launch has one.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    pub timeout_secs: u32,
}
impl LifecycleScript {
    pub fn new(scope: ScriptScope, phase: ScriptPhase, command: ScriptCommand) -> Self {
        Self {
            id: crate::model::new_id(),
            scope,
            name: String::new(),
            enabled: true,
            phase,
            command,
            cwd: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Checks a script before it is stored, with what to fix.
    pub fn normalized(&self) -> crate::Result<Self> {
        let invalid = |why: String| Err(crate::Error::Invalid(why));
        v::id(&self.id)?;
        match &self.scope {
            ScriptScope::Global => {}
            ScriptScope::Profile { profile_id } => v::id(profile_id)?,
            ScriptScope::Game { game_id } => v::id(game_id)?,
        }
        let mut out = self.clone();
        out.name = out.name.trim().to_owned();
        v::text(&out.name, "script name", false, 256)?;
        if !(1..=MAX_TIMEOUT_SECS).contains(&out.timeout_secs) {
            return invalid(crate::tr!(
                "the time limit must be 1 to {max} seconds",
                max = MAX_TIMEOUT_SECS
            ));
        }
        match &mut out.command {
            ScriptCommand::Direct { program, args } => {
                v::text(v::path_text(program)?, "program", true, 32768)?;
                if program.as_os_str().is_empty() {
                    return invalid(crate::tr!("choose the program the script runs"));
                }
                if args.len() > 1024 {
                    return invalid(crate::tr!("a script can have at most 1024 arguments"));
                }
                for arg in args {
                    v::text(arg, "argument", true, 32768)?;
                }
            }
            ScriptCommand::Shell { shell, script } => {
                if !Shell::available().contains(shell) {
                    return invalid(crate::tr!(
                        "{shell} scripts can't run on this system",
                        shell = shell.program()
                    ));
                }
                if script.trim().is_empty() {
                    return invalid(crate::tr!("the script is empty"));
                }
                v::text(script, "script", false, 64 * 1024)?;
            }
        }
        if let Some(cwd) = &out.cwd {
            if cwd.as_os_str().is_empty() {
                out.cwd = None;
            } else {
                v::text(v::path_text(cwd)?, "working folder", false, 32768)?;
            }
        }
        Ok(out)
    }
}

/// What a script knows about the launch. Unknown values are empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScriptContext {
    pub game_id: String,
    pub game_title: String,
    pub entry_id: String,
    #[serde(default)]
    pub action_id: Option<String>,
    #[serde(default)]
    pub action_name: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub install_dir: Option<PathBuf>,
    #[serde(default)]
    pub rom_path: Option<PathBuf>,
    #[serde(default)]
    pub platform_id: Option<String>,
    #[serde(default)]
    pub emulator_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    /// The game's process, once it is found (after start).
    #[serde(default)]
    pub pid: Option<u32>,
    /// How the session ended (after exit): `exited`, `crashed`,
    /// `interrupted`, `not_started` or `cancelled`.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// A test from the settings, not a launch.
    #[serde(default)]
    pub test: bool,
}

/// One value a script can read: its `{Placeholder}`, its environment
/// variable and its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextValue {
    pub placeholder: String,
    pub variable: String,
    pub value: String,
}

impl ScriptContext {
    /// Every value, in a fixed order, empty when unknown.
    pub fn values(&self, phase: ScriptPhase) -> Vec<ContextValue> {
        let path = |p: &Option<PathBuf>| {
            p.as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let text = |t: &Option<String>| t.clone().unwrap_or_default();
        [
            ("Phase", "CASKADE_PHASE", phase.as_str().to_owned()),
            ("GameId", "CASKADE_GAME_ID", self.game_id.clone()),
            ("GameTitle", "CASKADE_GAME_TITLE", self.game_title.clone()),
            ("EntryId", "CASKADE_ENTRY_ID", self.entry_id.clone()),
            ("ActionId", "CASKADE_ACTION_ID", text(&self.action_id)),
            ("ActionName", "CASKADE_ACTION_NAME", text(&self.action_name)),
            ("SessionId", "CASKADE_SESSION_ID", text(&self.session_id)),
            ("InstallDir", "CASKADE_INSTALL_DIR", path(&self.install_dir)),
            ("RomPath", "CASKADE_ROM_PATH", path(&self.rom_path)),
            ("PlatformId", "CASKADE_PLATFORM_ID", text(&self.platform_id)),
            ("EmulatorId", "CASKADE_EMULATOR_ID", text(&self.emulator_id)),
            ("ProfileId", "CASKADE_PROFILE_ID", text(&self.profile_id)),
            (
                "Pid",
                "CASKADE_PID",
                self.pid.map(|pid| pid.to_string()).unwrap_or_default(),
            ),
            ("Status", "CASKADE_STATUS", text(&self.status)),
            (
                "DurationMs",
                "CASKADE_DURATION_MS",
                self.duration_ms
                    .map(|ms| ms.to_string())
                    .unwrap_or_default(),
            ),
            (
                "Test",
                "CASKADE_TEST",
                if self.test { "1" } else { "" }.to_owned(),
            ),
        ]
        .into_iter()
        .map(|(placeholder, variable, value)| ContextValue {
            placeholder: format!("{{{placeholder}}}"),
            variable: variable.into(),
            value,
        })
        .collect()
    }
}

/// Replaces each `{Placeholder}` in `text` with its value. The value is
/// data: nothing in it is expanded again, and it stays in this one
/// argument. Unknown braces stay as they are.
pub fn expand(text: &str, values: &[ContextValue]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        for value in values {
            if tail.starts_with(&value.placeholder) {
                out.push_str(&value.value);
                rest = &tail[value.placeholder.len()..];
                continue 'scan;
            }
        }
        out.push('{');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// What running one script came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStatus {
    Succeeded,
    /// It ran and exited with a failure (or a signal).
    Failed,
    TimedOut,
    Cancelled,
    /// It couldn't start: a missing program or shell, a bad folder.
    NotStarted,
}

/// One script's run, for people: its output is bounded and stripped of
/// control characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRun {
    pub script_id: String,
    pub name: String,
    pub scope: ScriptScope,
    pub phase: ScriptPhase,
    pub status: ScriptStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Output was longer than kept; the end is kept.
    pub truncated: bool,
    pub duration_ms: u64,
    /// Why it failed, for people.
    pub error: Option<String>,
}
impl ScriptRun {
    pub fn succeeded(&self) -> bool {
        self.status == ScriptStatus::Succeeded
    }

    /// "“name” failed (exit code 2)", to follow a sentence.
    pub fn describe(&self) -> String {
        let name = &self.name;
        match self.status {
            ScriptStatus::Succeeded => crate::tr!("“{name}” finished", name = name),
            ScriptStatus::Failed => match self.exit_code {
                Some(code) => crate::tr!(
                    "“{name}” failed (exit code {code})",
                    name = name,
                    code = code
                ),
                None => crate::tr!("“{name}” was stopped by a signal", name = name),
            },
            ScriptStatus::TimedOut => crate::tr!("“{name}” ran out of time", name = name),
            ScriptStatus::Cancelled => crate::tr!("“{name}” was cancelled", name = name),
            ScriptStatus::NotStarted => crate::tr!(
                "“{name}” couldn't start: {why}",
                name = name,
                why = self.error.clone().unwrap_or_default()
            ),
        }
    }
}

/// The scripts of one launch, by scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptSet {
    pub global: Vec<LifecycleScript>,
    pub profile: Vec<LifecycleScript>,
    pub game: Vec<LifecycleScript>,
}
impl ScriptSet {
    pub fn is_empty(&self) -> bool {
        self.global.is_empty() && self.profile.is_empty() && self.game.is_empty()
    }

    /// The enabled scripts of `phase` in the order they run: global,
    /// emulator profile, game before launch and after start; the reverse
    /// after exit, so cleanup undoes the innermost preparation first. Each
    /// scope keeps its own order.
    pub fn ordered(&self, phase: ScriptPhase) -> Vec<&LifecycleScript> {
        self.ordered_all(phase)
            .into_iter()
            .filter(|script| script.enabled)
            .collect()
    }

    /// [`Self::ordered`] with the disabled scripts where they would run.
    pub fn ordered_all(&self, phase: ScriptPhase) -> Vec<&LifecycleScript> {
        let scopes = [&self.global, &self.profile, &self.game];
        let scopes: Vec<_> = if phase == ScriptPhase::AfterExit {
            scopes.into_iter().rev().collect()
        } else {
            scopes.into_iter().collect()
        };
        scopes
            .into_iter()
            .flatten()
            .filter(|script| script.phase == phase)
            .collect()
    }
}

/// The runs of one phase.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScriptReport {
    pub game_id: String,
    pub game_title: String,
    pub session_id: Option<String>,
    pub phase: Option<ScriptPhase>,
    pub runs: Vec<ScriptRun>,
}
impl ScriptReport {
    pub fn failed(&self) -> Option<&ScriptRun> {
        self.runs.iter().find(|run| !run.succeeded())
    }
}

/// Runs scripts through an executor: the real one, or a fake in tests.
#[derive(Clone)]
pub struct ScriptRunner {
    executor: Arc<dyn ScriptExecutor>,
}
impl Default for ScriptRunner {
    fn default() -> Self {
        Self::new(Arc::new(ProcessExecutor))
    }
}
impl ScriptRunner {
    pub fn new(executor: Arc<dyn ScriptExecutor>) -> Self {
        Self { executor }
    }

    /// Runs `phase`'s scripts in order. Before launch it stops at the first
    /// that doesn't succeed; after start and after exit each runs anyway.
    pub async fn run_phase(
        &self,
        scripts: &ScriptSet,
        phase: ScriptPhase,
        context: &ScriptContext,
        cancel: &CancellationToken,
    ) -> ScriptReport {
        let mut report = ScriptReport {
            game_id: context.game_id.clone(),
            game_title: context.game_title.clone(),
            session_id: context.session_id.clone(),
            phase: Some(phase),
            runs: vec![],
        };
        for script in scripts.ordered(phase) {
            let run = self.run(script, context, cancel).await;
            let stop = !run.succeeded() && phase == ScriptPhase::BeforeLaunch;
            report.runs.push(run);
            if stop {
                break;
            }
        }
        report
    }

    /// Runs one script with `context`: launches and tests alike.
    pub async fn run(
        &self,
        script: &LifecycleScript,
        context: &ScriptContext,
        cancel: &CancellationToken,
    ) -> ScriptRun {
        let values = context.values(script.phase);
        let (program, (args, verbatim_args)) = match &script.command {
            ScriptCommand::Direct { program, args } => (
                PathBuf::from(expand(&program.to_string_lossy(), &values)),
                (args.iter().map(|arg| expand(arg, &values)).collect(), false),
            ),
            ScriptCommand::Shell { shell, script } => {
                (PathBuf::from(shell.program()), shell.args(script))
            }
        };
        let cwd = script
            .cwd
            .as_ref()
            .map(|cwd| PathBuf::from(expand(&cwd.to_string_lossy(), &values)))
            .or_else(|| context.install_dir.clone().filter(|dir| dir.is_dir()));
        let env: BTreeMap<String, String> = values
            .into_iter()
            .map(|value| (value.variable, value.value))
            .collect();
        let started = Instant::now();
        let outcome = self
            .executor
            .run(
                ScriptRequest {
                    program,
                    args,
                    verbatim_args,
                    cwd,
                    env,
                    timeout: Duration::from_secs(u64::from(script.timeout_secs)),
                },
                cancel.clone(),
            )
            .await;
        let run = ScriptRun {
            script_id: script.id.clone(),
            name: script.name.clone(),
            scope: script.scope.clone(),
            phase: script.phase,
            status: outcome.status,
            exit_code: outcome.exit_code,
            stdout: outcome.stdout,
            stderr: outcome.stderr,
            truncated: outcome.truncated,
            duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            error: outcome.error,
        };
        // The name and outcome only: arguments, output and environment can
        // hold paths and personal data.
        tracing::info!(
            script = %run.script_id,
            phase = run.phase.as_str(),
            status = ?run.status,
            code = ?run.exit_code,
            "lifecycle script ran"
        );
        run
    }
}

/// What an executor reports about one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutcome {
    pub status: ScriptStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests;
