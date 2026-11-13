use super::{
    EmulatorLaunches, LaunchChoice, LaunchCommand, LaunchError, LaunchEvent, LaunchHooks,
    LaunchTicket, NoHooks, NoPluginRunner, PlayGate, PluginRunner, PreparedLaunch, Result,
    RunnerPaths, SessionStatus, actions, resolve, store_error,
};
use crate::{
    model::{GameAction, GameEntry, InstallLocation, LaunchSpec, PlaySession},
    now_ms,
    scripts::{ScriptContext, ScriptPhase, ScriptRunner, ScriptSet},
    sessions::{RecoveryState, ScriptState, SessionRepository},
    tracking::{LiveTracker, ProcessMonitor, ProcessTracker},
};
use caskade_installer::runtime::alias::AliasStore;
use std::{
    collections::BTreeMap,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{process::Child, sync::oneshot};
use tokio_util::sync::CancellationToken;

mod scripts;
mod tools;
mod watch;

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// How often tracking polls while it looks for the game's processes: from
    /// launch until they are found, and again once they are gone (the exit
    /// grace). While the game runs it polls every
    /// [`playing_poll_interval`](Self::playing_poll_interval).
    pub poll_interval: Duration,
    pub launch_timeout: Duration,
    /// Allow a launcher handoff gap without charging the gap as playtime.
    pub exit_grace: Duration,
    pub heartbeat_interval: Duration,
}
impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(250),
            launch_timeout: Duration::from_secs(90),
            exit_grace: Duration::from_secs(2),
            heartbeat_interval: Duration::from_secs(5),
        }
    }
}

/// How long finding a managed Windows game's runner may take before Play
/// gives up. It may download umu-launcher, whose own timeouts end an offline
/// attempt well within this.
const RUNNER_TIMEOUT: Duration = Duration::from_secs(180);

/// While the game runs, polls are this many times further apart …
const PLAYING_POLL_FACTOR: u32 = 6;
/// … up to this, unless `poll_interval` itself is longer.
const PLAYING_POLL_MAX: Duration = Duration::from_millis(1500);

impl LaunchConfig {
    /// The poll interval while the game's processes are alive: 1.5 s by
    /// default. A poll walks every process on the system, and nothing needs
    /// sub-second accuracy once the game is found.
    pub fn playing_poll_interval(&self) -> Duration {
        self.poll_interval
            .saturating_mul(PLAYING_POLL_FACTOR)
            .min(PLAYING_POLL_MAX)
            .max(self.poll_interval)
    }
}

/// Dropping a handle leaves the game and its background tracker running.
pub struct LaunchHandle {
    pub ticket: LaunchTicket,
    pub(super) completed: oneshot::Receiver<Result<(PlaySession, SessionStatus)>>,
}
impl LaunchHandle {
    pub async fn wait(self) -> Result<PlaySession> {
        self.wait_status().await.map(|(session, _)| session)
    }
    /// The ended session, and whether the game exited or crashed.
    pub async fn wait_status(self) -> Result<(PlaySession, SessionStatus)> {
        self.completed
            .await
            .map_err(|_| LaunchError::Store("session worker stopped".into()))?
    }
}

#[derive(Clone)]
pub struct LaunchService {
    pub(super) repository: Arc<dyn SessionRepository>,
    pub(super) monitor: ProcessMonitor,
    pub(super) hooks: Arc<dyn LaunchHooks>,
    plugins: Arc<dyn PluginRunner>,
    aliases: Option<AliasStore>,
    runners: RunnerPaths,
    runner_resolver: Option<Arc<dyn super::RunnerResolver>>,
    emulators: Option<EmulatorLaunches>,
    web: Option<super::WebPlayer>,
    preparer: Option<Arc<dyn super::LaunchPreparer>>,
    gate: Option<Arc<dyn PlayGate>>,
    config: LaunchConfig,
    runner_timeout: Duration,
    running: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
    script_runner: ScriptRunner,
    /// The trackers of the sessions being followed: what [`Self::stop`]
    /// asks for processes newer than the stored ones.
    pub(super) live: Arc<Mutex<BTreeMap<String, LiveTracker>>>,
    /// Sessions [`Self::stop`] ended, until [`Self::take_stopped`] asks.
    pub(super) stopped: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// Streams releases from other devices (#264), once the client exists.
    pub(super) streamer: Arc<std::sync::OnceLock<Arc<dyn super::StreamLauncher>>>,
    /// The streams running now; never stored.
    pub(super) streams: super::stream::Streams,
}
impl LaunchService {
    pub fn new(store: crate::db::LibraryStore) -> Self {
        Self::from_repository(Arc::new(store))
    }
    pub fn from_repository(repository: Arc<dyn SessionRepository>) -> Self {
        Self {
            repository,
            monitor: ProcessMonitor::default(),
            hooks: Arc::new(NoHooks),
            plugins: Arc::new(NoPluginRunner),
            aliases: AliasStore::for_host(),
            runners: RunnerPaths::default(),
            runner_resolver: None,
            emulators: None,
            web: None,
            preparer: None,
            gate: None,
            config: LaunchConfig::default(),
            runner_timeout: RUNNER_TIMEOUT,
            running: Arc::default(),
            script_runner: ScriptRunner::default(),
            live: Arc::default(),
            stopped: Arc::default(),
            streamer: Arc::default(),
            streams: Arc::default(),
        }
    }
    pub fn with_bus(self, bus: crate::bus::Bus) -> Self {
        self.with_hooks(Arc::new(bus))
    }

    pub fn with_hooks(mut self, hooks: Arc<dyn LaunchHooks>) -> Self {
        self.hooks = hooks;
        self
    }
    pub fn with_runner_paths(mut self, runners: RunnerPaths) -> Self {
        self.runners = runners;
        self
    }

    /// Read runner settings at launch time, without persisting machine paths.
    pub fn with_runner_resolver(mut self, resolver: Arc<dyn super::RunnerResolver>) -> Self {
        self.runner_resolver = Some(resolver);
        self
    }

    /// How long the runner resolver may take (3 minutes by default).
    pub fn with_runner_timeout(mut self, timeout: Duration) -> Self {
        self.runner_timeout = timeout;
        self
    }

    /// Launch emulated games (`LaunchSpec::Emulator`).
    pub fn with_emulators(mut self, emulators: EmulatorLaunches) -> Self {
        self.emulators = Some(emulators);
        self
    }

    /// Launch web games (`LaunchSpec::Web`) in the game window.
    pub fn with_web_player(mut self, web: super::WebPlayer) -> Self {
        self.web = Some(web);
        self
    }

    /// Runs before each launch resolves (#1: RomM downloads).
    pub fn with_preparer(mut self, preparer: Arc<dyn super::LaunchPreparer>) -> Self {
        self.preparer = Some(preparer);
        self
    }

    /// Asked before every launch whether it may start (#209: age limits).
    pub fn with_play_gate(mut self, gate: Arc<dyn PlayGate>) -> Self {
        self.gate = Some(gate);
        self
    }

    pub fn with_plugins(mut self, plugins: Arc<dyn PluginRunner>) -> Self {
        self.plugins = plugins;
        self
    }
    pub fn with_aliases(mut self, aliases: Option<AliasStore>) -> Self {
        self.aliases = aliases;
        self
    }
    /// Runs lifecycle scripts through `runner` (a fake one in tests).
    pub fn with_script_runner(mut self, runner: ScriptRunner) -> Self {
        self.script_runner = runner;
        self
    }
    pub fn with_config(mut self, config: LaunchConfig) -> Self {
        self.config = config;
        self
    }

    /// Acquire the same exclusion used by launch/recovery. Storage operations
    /// hold this guard across filesystem work and DB updates, so a game cannot
    /// start halfway through a move. Use the application's shared service.
    pub fn try_game_lease(&self, game: &str) -> Result<GameLease> {
        let cancel = CancellationToken::new();
        // A panicked holder leaves the map as it was; carry on with it
        // rather than refusing every launch until a restart (#160).
        let mut running = crate::lock(&self.running);
        if running.contains_key(game) {
            return Err(LaunchError::AlreadyRunning);
        }
        running.insert(game.into(), cancel.clone());
        Ok(GameLease {
            game: game.into(),
            running: Arc::clone(&self.running),
            cancel,
        })
    }

    /// The games running now, streams (#264) included.
    pub async fn running_sessions(&self) -> Result<Vec<LaunchTicket>> {
        let mut running: Vec<LaunchTicket> = self
            .repository
            .in_flight()
            .await?
            .iter()
            .map(ticket)
            .collect();
        running.extend(self.stream_tickets());
        Ok(running)
    }

    /// Stop observation for app shutdown, preserving in-flight sessions and
    /// processes. recover() reattaches using recorded PID/start-time identities.
    pub fn shutdown(&self) {
        if let Ok(running) = self.running.lock() {
            for cancel in running.values() {
                cancel.cancel();
            }
        }
    }

    pub async fn launch(&self, entry_id: &str) -> Result<LaunchHandle> {
        self.launch_with(entry_id, &LaunchChoice::default()).await
    }

    /// Launch with the answers to an earlier [`LaunchError::NeedsChoice`].
    /// The play gate is asked first, before any download or runner, and
    /// hears when the game started.
    pub async fn launch_with(&self, entry_id: &str, choice: &LaunchChoice) -> Result<LaunchHandle> {
        self.launch_with_env(entry_id, choice, &BTreeMap::new())
            .await
    }

    /// [`Self::launch_with`] for a program that asked Caskade for the launch
    /// (`caskade --launch`, #256): `env` joins the game's environment, after
    /// the target's own and before plugins' `launch.before` hooks. The
    /// webview never supplies it.
    pub async fn launch_with_env(
        &self,
        entry_id: &str,
        choice: &LaunchChoice,
        env: &BTreeMap<String, String>,
    ) -> Result<LaunchHandle> {
        let Some(gate) = &self.gate else {
            return self.start(entry_id, choice, env).await;
        };
        let game_id = self.entry(entry_id).await?.game_id;
        gate.admit(&game_id).await?;
        let handle = self.start(entry_id, choice, env).await?;
        gate.started(&game_id);
        Ok(handle)
    }

    async fn entry(&self, entry_id: &str) -> Result<GameEntry> {
        self.repository
            .entry(entry_id)
            .await?
            .ok_or_else(|| LaunchError::Invalid(crate::tr!("entry not found")))
    }

    async fn start(
        &self,
        entry_id: &str,
        choice: &LaunchChoice,
        env: &BTreeMap<String, String>,
    ) -> Result<LaunchHandle> {
        if let Some(preparer) = &self.preparer {
            preparer.prepare(entry_id).await?;
        }
        let entry = self.entry(entry_id).await?;
        // A release another device offers streams from it (#264): its own
        // launch, never an added action.
        if let Some((host, app)) = super::stream::target(&entry) {
            return self.stream(&entry, host, app, choice).await;
        }
        // The action is chosen before anything is reserved, so a missing or
        // invalid one leaves no session behind (#177).
        let action = self.action(&entry, choice).await?;
        let lease = self.try_game_lease(&entry.game_id)?;
        let mut prepared = self
            .resolve_entry(&actions::apply(&entry, &action), choice, &lease.cancel)
            .await?;
        prepared
            .command
            .env
            .extend(env.iter().map(|(key, value)| (key.clone(), value.clone())));
        let install_dir = prepared.target.real_dir.clone();
        actions::track(&mut prepared, &action.tracking);
        let mut session = PlaySession::new(&entry.game_id, &entry.id, now_ms());
        let ticket = ticket(&session);
        if !self.hooks.before(&ticket, &mut prepared.command).await? {
            return Err(LaunchError::Cancelled);
        }
        prepared.command.validate()?;
        let baseline = self.monitor.snapshot().await?;
        let existing = ProcessTracker::new(prepared.target.clone(), &Default::default());
        let (_, existing) = existing.observe_async(baseline.clone()).await?;
        if (prepared.target.real_dir.is_some() || prepared.target.steam_app_id.is_some())
            && !existing.is_empty()
        {
            return Err(LaunchError::AlreadyRunning);
        }
        if let Some(before) = prepared
            .command
            .emulation
            .as_ref()
            .and_then(|emulation| emulation.before.as_deref())
            && let Err(why) =
                scripts::profile_hook("before", before, prepared.command.cwd.as_deref()).await
        {
            return Err(LaunchError::Refused(crate::tr!(
                "The emulator profile's before-launch command {why}, so the game wasn't started.",
                why = why
            )));
        }
        // The user's lifecycle scripts (#179), after the plugins' and the
        // profile's own before-launch steps. From here on, a launch that
        // doesn't start runs the after-exit scripts once, as cleanup.
        let emulation = prepared.command.emulation.as_ref();
        let context = ScriptContext {
            game_id: entry.game_id.clone(),
            game_title: self
                .repository
                .game_title(&entry.game_id)
                .await?
                .unwrap_or_default(),
            entry_id: entry.id.clone(),
            action_id: Some(action.id.clone()),
            action_name: Some(action.name.clone()),
            session_id: Some(session.id.clone()),
            install_dir,
            rom_path: emulation.map(|emulation| emulation.rom.clone()),
            platform_id: emulation.map(|emulation| emulation.platform_id.clone()),
            emulator_id: emulation.map(|emulation| emulation.emulator_id.clone()),
            profile_id: emulation.map(|emulation| emulation.profile_id.clone()),
            ..Default::default()
        };
        let scripts = self
            .repository
            .scripts(&entry.game_id, context.profile_id.as_deref())
            .await?;
        let before = self
            .script_runner
            .run_phase(
                &scripts,
                ScriptPhase::BeforeLaunch,
                &context,
                &CancellationToken::new(),
            )
            .await;
        self.report(&before).await;
        if let Some(failed) = before.failed() {
            self.cleanup(&scripts, &context, "failed").await;
            return Err(LaunchError::Refused(crate::tr!(
                "The before-launch script {why}, so the game wasn't started.",
                why = failed.describe()
            )));
        }
        let mut state = RecoveryState::pending(prepared.target.clone());
        state.action = Some(action.id.clone());
        state.scripts = Some(ScriptState {
            context: context.clone(),
            started: false,
        });
        if prepared.command.emulation.is_some() {
            state.command = Some(prepared.command.clone());
        }
        session.tracking = Some(serde_json::to_value(&state).map_err(store_error)?);
        if let Err(error) = self.repository.reserve(&session).await {
            self.cleanup(&scripts, &context, "not_started").await;
            return Err(error);
        }
        if let Err(error) = prepared.prepare().await {
            return Err(self
                .not_started(&mut session, &scripts, &context, error)
                .await);
        }
        let baseline = match self.monitor.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Err(self
                    .not_started(&mut session, &scripts, &context, error.into())
                    .await);
            }
        };
        let spawned_at = Instant::now();
        session.started_at_ms = now_ms();
        let child = match spawn(&prepared.command) {
            Ok(child) => child,
            Err(error) => {
                return Err(self
                    .not_started(&mut session, &scripts, &context, error)
                    .await);
            }
        };
        let mut tracker = ProcessTracker::new(prepared.target, &baseline);
        let snapshot = match self.monitor.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // A process is already running. Retain its pending session for
                // recovery rather than claiming that it exited or killing it.
                let mut child = child;
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                return Err(error.into());
            }
        };
        if let Some(identity) = child.id().and_then(|pid| {
            snapshot
                .processes
                .iter()
                .find(|p| p.identity.pid == pid)
                .map(|p| p.identity)
        }) {
            state.launcher = Some(identity);
            tracker.launcher(identity);
            if prepared.direct {
                state.processes = vec![identity];
                tracker.recover(&state.processes);
            }
        }
        session.tracking = Some(serde_json::to_value(&state).map_err(store_error)?);
        self.repository.heartbeat(&session).await?;
        let tracker = LiveTracker::new(tracker);
        Ok(self.follow(watch::Watch {
            session,
            state,
            tracker,
            child: Some(child),
            direct: prepared.direct,
            lease,
            resumed: false,
            requested: spawned_at,
        }))
    }

    /// A reserved session whose game didn't start: it ends now and the
    /// after-exit scripts clean up. Returns `error`, unless ending the
    /// session failed.
    async fn not_started(
        &self,
        session: &mut PlaySession,
        scripts: &ScriptSet,
        context: &ScriptContext,
        error: LaunchError,
    ) -> LaunchError {
        session.ended_at_ms = Some(now_ms());
        if let Err(finish) = self.repository.finish(session).await {
            return finish;
        }
        self.cleanup(scripts, context, "not_started").await;
        error
    }

    /// The play action `choice` names, or the entry's default (#177).
    async fn action(&self, entry: &GameEntry, choice: &LaunchChoice) -> Result<GameAction> {
        let settings = self.repository.action_settings(&entry.id).await?;
        let added = self.repository.actions(&entry.id).await?;
        let all = actions::entry_actions(entry, added, &settings);
        if all.is_empty() && !matches!(entry.state, caskade_discovery::InstallState::Installed) {
            return Err(LaunchError::Invalid(crate::tr!("entry is not installed")));
        }
        let action = actions::select(entry, all, &settings, choice)?;
        actions::validate_tracking(entry, &action)?;
        Ok(action)
    }

    /// What the entry launches as `entry` stands: its emulator, its game
    /// window, or its program, through its runner.
    async fn resolve_entry(
        &self,
        entry: &GameEntry,
        choice: &LaunchChoice,
        cancel: &CancellationToken,
    ) -> Result<PreparedLaunch> {
        let root = match &entry.location {
            Some(InstallLocation::Managed { root_id, .. }) => self.repository.root(root_id).await?,
            _ => None,
        };
        let runners = if matches!(entry.location, Some(InstallLocation::Managed { .. }))
            && matches!(
                entry.runtime,
                caskade_discovery::GameRuntime::Windows { .. }
            )
            && let Some(resolver) = &self.runner_resolver
        {
            // Resolving may download umu-launcher while the game's lease is
            // held: shutdown cancels it and the timeout ends it, and either
            // way returning drops the lease.
            tokio::select! {
                _ = cancel.cancelled() => return Err(LaunchError::Cancelled),
                resolved = tokio::time::timeout(self.runner_timeout, resolver.resolve(entry)) => {
                    resolved.map_err(|_| {
                        LaunchError::Invalid(crate::tr!(
                            "the compatibility tools were not ready within {seconds} s; check the network connection and try again",
                            seconds = self.runner_timeout.as_secs()
                        ))
                    })??
                }
            }
        } else {
            self.runners.clone()
        };
        if matches!(entry.launch, Some(LaunchSpec::Emulator { .. })) {
            self.emulators
                .as_ref()
                .ok_or_else(|| LaunchError::Unsupported("emulator launches".into()))?
                .resolve(entry, choice)
                .await
        } else if matches!(entry.launch, Some(LaunchSpec::Web { .. })) {
            if !matches!(entry.state, caskade_discovery::InstallState::Installed) {
                return Err(LaunchError::Invalid(crate::tr!("entry is not installed")));
            }
            self.web
                .as_ref()
                .ok_or_else(|| LaunchError::Unsupported("web games".into()))?
                .resolve(entry, root)
                .await
        } else {
            resolve(
                entry,
                root,
                self.aliases.clone(),
                Arc::clone(&self.plugins),
                runners,
            )
            .await
        }
    }
}

pub struct GameLease {
    game: String,
    running: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
    pub(super) cancel: CancellationToken,
}
impl Drop for GameLease {
    fn drop(&mut self) {
        if let Ok(mut running) = self.running.lock() {
            running.remove(&self.game);
        }
    }
}

/// Starts `command` with no stdio. The error names the program: a bare
/// "No such file or directory" doesn't say what was missing (#160).
fn spawn(command: &LaunchCommand) -> Result<Child> {
    let mut process = tokio::process::Command::new(&command.program);
    // Not inside Caskade's AppImage (#274).
    crate::managed::outside_own_appimage(process.as_std_mut());
    process
        .args(&command.args)
        .envs(&command.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = &command.cwd {
        process.current_dir(cwd);
    }
    process.spawn().map_err(|error| {
        LaunchError::Io(std::io::Error::new(
            error.kind(),
            crate::tr!(
                "couldn't start “{program}”: {error}",
                program = command.program.display(),
                error = error
            ),
        ))
    })
}

pub(super) fn ticket(s: &PlaySession) -> LaunchTicket {
    LaunchTicket {
        game_id: s.game_id.clone(),
        entry_id: s.entry_id.clone(),
        session_id: s.id.clone(),
    }
}
fn recovery(s: &PlaySession) -> Option<RecoveryState> {
    s.tracking
        .as_ref()
        .and_then(|value| serde_json::from_value::<RecoveryState>(value.clone()).ok())
}
fn event(s: &PlaySession, status: SessionStatus) -> LaunchEvent {
    LaunchEvent {
        command: recovery(s).and_then(|state| state.command),
        game_id: s.game_id.clone(),
        entry_id: s.entry_id.clone(),
        session_id: s.id.clone(),
        started_at_ms: s.started_at_ms,
        ended_at_ms: s.ended_at_ms,
        duration_ms: s.duration_ms,
        status,
    }
}
pub(super) fn millis(d: Duration) -> u64 {
    d.as_millis().min(u64::MAX as u128) as u64
}
