//! Validated application preferences translated into planner/runtime inputs.
use super::PipelineResult;
use crate::{
    OperatingSystem,
    host::{HostCaps, RunnerKind},
    plan::Policy,
    runtime::{ProcessCommand, ProcessExit, ProcessId, RuntimeError, proton::Launcher},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityRunner {
    #[default]
    #[schemars(title = "Proton via umu")]
    Proton,
    #[schemars(title = "GE-Proton via umu")]
    GeProton,
    #[schemars(title = "Steam Proton via umu")]
    SteamProton,
    #[schemars(title = "System Wine")]
    SystemWine,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct InstallerConfig {
    pub policy: Policy,
    pub compatibility_runner: CompatibilityRunner,
}

impl CompatibilityRunner {
    /// Filter an injected capability snapshot; never detect the host here.
    pub fn configure_host(self, host: &mut HostCaps) {
        if host.os == OperatingSystem::Windows {
            return;
        }
        match self {
            Self::SystemWine => host.runners.retain(|r| r.kind != RunnerKind::Proton),
            _ if host.os == OperatingSystem::Linux => {
                host.runners.retain(|r| r.kind != RunnerKind::Wine);
                host.proton_builds.retain(|path| {
                    let name = || {
                        path.file_name()
                            .map(|s| s.to_string_lossy().to_ascii_lowercase())
                            .unwrap_or_default()
                    };
                    match self {
                        Self::GeProton => {
                            let name = name();
                            name.starts_with("ge-proton") || name.starts_with("ge_proton")
                        }
                        Self::SteamProton => {
                            name().starts_with("proton")
                                && path
                                    .parent()
                                    .is_some_and(|p| p.file_name().is_some_and(|n| n == "common"))
                        }
                        _ => true,
                    }
                });
            }
            _ => (),
        }
    }

    /// `PROTONPATH` from a host [configured](Self::configure_host) for this
    /// choice: the best Proton build installed, or for plain Proton, when
    /// none is, umu's own download of UMU-Proton.
    pub fn proton_path(self, host: &HostCaps) -> Result<String, &'static str> {
        match crate::host::automatic_proton(&host.proton_builds) {
            Some(path) => Ok(path),
            None if self == Self::Proton => Ok(crate::host::UMU_PROTON.into()),
            None => Err("the selected GE-Proton/Steam Proton build is not installed"),
        }
    }

    pub fn launcher(self, host: &HostCaps, game_id: &str) -> PipelineResult<Launcher> {
        let mut selected = host.clone();
        self.configure_host(&mut selected);
        let host = &selected;
        if self == Self::SystemWine || host.os == OperatingSystem::Macos {
            return Ok(Launcher::Wine { wine:host.runners.iter().find(|r|r.kind==RunnerKind::Wine)
                .ok_or("system Wine is unavailable; install Wine or select another compatibility runner")?.program.clone() });
        }
        let umu_run = host
            .runners
            .iter()
            .find(|r| r.kind == RunnerKind::Proton)
            .ok_or("umu-launcher is unavailable for the selected compatibility runner")?
            .program
            .clone();
        let proton_path = self.proton_path(host)?;
        Ok(Launcher::Umu {
            umu_run,
            proton_path,
            game_id: game_id.into(),
        })
    }
}

/// The non-native runtime a step runs under.
fn step_runtime(step: &crate::Step) -> Option<&crate::RuntimeSpec> {
    match step {
        crate::Step::Run { runtime, .. }
        | crate::Step::RunInteractive { runtime, .. }
        | crate::Step::RegistryImport { runtime, .. } => Some(runtime),
        _ => None,
    }
}

fn candidate_runtimes(candidate: &crate::Candidate) -> impl Iterator<Item = &crate::RuntimeSpec> {
    candidate
        .requires
        .runtime
        .iter()
        .chain(candidate.steps.iter().filter_map(step_runtime))
        .filter(|spec| **spec != crate::RuntimeSpec::Native)
}

/// Registers every candidate's runtime without downloading anything: umu is
/// fetched when a candidate first runs a program under it, so a plan that
/// only extracts installs offline. A runtime the host lacks fails just the
/// candidates that use it and the executor moves on to the next; the job
/// fails up front, with the reason, only when no candidate can run.
pub(super) async fn register_runtimes(
    worker: &mut crate::exec::Executor,
    plans: &crate::PlanSet,
    host: &HostCaps,
    ctx: &super::Context,
) -> PipelineResult<()> {
    let mut specs = Vec::new();
    for spec in plans.candidates.iter().flat_map(candidate_runtimes) {
        if !specs.contains(spec) {
            specs.push(spec.clone());
        }
    }
    let runtimes = prefix_runtimes(specs, host, ctx);
    let unavailable = |candidate: &crate::Candidate| {
        candidate_runtimes(candidate).find_map(|spec| {
            runtimes
                .iter()
                .find(|(registered, _)| registered == spec)
                .and_then(|(_, runtime)| runtime.as_ref().err())
        })
    };
    if !plans.candidates.is_empty()
        && plans
            .candidates
            .iter()
            .all(|candidate| unavailable(candidate).is_some())
    {
        let selected = plans
            .selected
            .and_then(|index| plans.candidates.get(index as usize))
            .unwrap_or(&plans.candidates[0]);
        let reason = unavailable(selected)
            .or_else(|| plans.candidates.iter().find_map(unavailable))
            .expect("every candidate has an unavailable runtime");
        return Err(reason.clone().into());
    }
    register(worker, runtimes);
    Ok(())
}

/// Registers the runtimes an uninstall may need. Nothing is downloaded and
/// nothing fails here: a game folder is deleted offline, and only an
/// uninstaller that actually runs needs its runtime.
pub(super) fn register_uninstall_runtime(
    worker: &mut crate::exec::Executor,
    spec: crate::RuntimeSpec,
    host: &HostCaps,
    ctx: &super::Context,
) {
    register(worker, prefix_runtimes(vec![spec], host, ctx));
}

type Prepared = Vec<(
    crate::RuntimeSpec,
    Result<std::sync::Arc<dyn crate::runtime::Runtime>, String>,
)>;

fn register(worker: &mut crate::exec::Executor, runtimes: Prepared) {
    for (spec, runtime) in runtimes {
        let runtime = runtime.unwrap_or_else(|message| {
            tracing::info!(?spec, %message, "runtime unavailable");
            std::sync::Arc::new(Unavailable(message))
        });
        worker.register_runtime(spec, runtime);
    }
}

/// Prefixes live inside the game folder and need symlinks and exec bits.
pub(super) const PREFIX_FILESYSTEM_UNSUPPORTED: &str = "the selected root cannot host an executable game-local compatibility prefix; choose an executable filesystem with symlink support";

/// Where umu-launcher is bootstrapped when the host has none.
pub(super) fn umu_bootstrap(ctx: &super::Context) -> Option<crate::runtime::umu::UmuBootstrap> {
    ctx.umu
        .clone()
        .or_else(crate::runtime::umu::UmuBootstrap::for_host)
}

fn prefix_runtimes(
    specs: Vec<crate::RuntimeSpec>,
    host: &HostCaps,
    ctx: &super::Context,
) -> Prepared {
    specs
        .into_iter()
        .map(|spec| {
            let runtime = prefix_runtime(&spec, host, ctx);
            (spec, runtime)
        })
        .collect()
}

fn prefix_runtime(
    spec: &crate::RuntimeSpec,
    host: &HostCaps,
    ctx: &super::Context,
) -> Result<std::sync::Arc<dyn crate::runtime::Runtime>, String> {
    let (crate::RuntimeSpec::ProtonPrefix { id } | crate::RuntimeSpec::WinePrefix { id }) = spec
    else {
        return Err(format!("{spec:?} is not a compatibility prefix"));
    };
    if !host.filesystem.symlinks || !host.filesystem.exec_bits {
        return Err(PREFIX_FILESYSTEM_UNSUPPORTED.into());
    }
    let preference = if matches!(spec, crate::RuntimeSpec::WinePrefix { .. }) {
        CompatibilityRunner::SystemWine
    } else if ctx.config.compatibility_runner == CompatibilityRunner::SystemWine {
        CompatibilityRunner::Proton
    } else {
        ctx.config.compatibility_runner
    };
    let mut host = host.clone();
    // Without a umu-run on the host, point the launcher at the bootstrapped
    // one and fetch it when a program first runs.
    let bootstrap = if host.os == OperatingSystem::Linux
        && preference != CompatibilityRunner::SystemWine
        && !host
            .runners
            .iter()
            .any(|runner| runner.kind == RunnerKind::Proton && runner.program.is_file())
    {
        umu_bootstrap(ctx)
    } else {
        None
    };
    if let Some(bootstrap) = &bootstrap {
        host.runners
            .retain(|runner| runner.kind != RunnerKind::Proton);
        host.runners.push(crate::host::RunnerInfo {
            kind: RunnerKind::Proton,
            program: bootstrap.umu_run(),
        });
    }
    let launcher = preference
        .launcher(&host, id)
        .map_err(|error| error.to_string())?;
    let runtime = crate::runtime::proton::PrefixRuntime::new(launcher);
    Ok(match bootstrap {
        Some(umu) if umu.installed().is_none() => std::sync::Arc::new(Bootstrapping {
            runtime,
            umu,
            cancel: ctx.cancel.clone(),
            handle: tokio::runtime::Handle::try_current().ok(),
            ready: std::sync::OnceLock::new(),
        }),
        _ => std::sync::Arc::new(runtime),
    })
}

/// A runtime the host lacks: the candidates that need it fail with its reason.
struct Unavailable(String);
impl crate::runtime::Runtime for Unavailable {
    fn spawn(&self, _: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        Err(RuntimeError::Io {
            message: self.0.clone(),
        })
    }
    fn wait(&self, process: ProcessId, _: Duration) -> Result<ProcessExit, RuntimeError> {
        Err(RuntimeError::UnknownProcess { id: process })
    }
    fn kill(&self, process: ProcessId) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnknownProcess { id: process })
    }
}

/// A Proton prefix whose umu-launcher is downloaded when the first program
/// starts. Runtimes are called from the executor's blocking worker, which
/// waits for the download there.
struct Bootstrapping {
    runtime: crate::runtime::proton::PrefixRuntime,
    umu: crate::runtime::umu::UmuBootstrap,
    cancel: crate::exec::CancellationToken,
    handle: Option<tokio::runtime::Handle>,
    /// The first download's outcome; a failure is not retried in this job.
    ready: std::sync::OnceLock<Result<(), String>>,
}
impl Bootstrapping {
    fn ready(&self) -> Result<(), RuntimeError> {
        self.ready
            .get_or_init(|| {
                #[cfg(feature = "fetch")]
                {
                    let handle = self
                        .handle
                        .as_ref()
                        .ok_or("umu-launcher cannot be downloaded outside an async runtime")?;
                    handle
                        .block_on(self.umu.ensure_with(&self.cancel))
                        .map(drop)
                        .map_err(|error| error.to_string())
                }
                #[cfg(not(feature = "fetch"))]
                {
                    let _ = (&self.umu, &self.cancel, &self.handle);
                    Err(
                        "umu-launcher is unavailable; install it or enable runtime downloads"
                            .into(),
                    )
                }
            })
            .clone()
            .map_err(|message| RuntimeError::Io { message })
    }
}
impl crate::runtime::Runtime for Bootstrapping {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        self.ready()?;
        self.runtime.spawn(command)
    }
    fn wait(&self, process: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.runtime.wait(process, timeout)
    }
    fn kill(&self, process: ProcessId) -> Result<(), RuntimeError> {
        self.runtime.kill(process)
    }
    fn spawn_with_input(
        &self,
        command: &ProcessCommand,
        input: &[u8],
    ) -> Result<ProcessId, RuntimeError> {
        self.ready()?;
        self.runtime.spawn_with_input(command, input)
    }
}

#[cfg(all(test, feature = "fetch"))]
mod tests {
    use super::*;
    use crate::{RuntimeSpec, runtime::umu::UmuBootstrap, testing::RecordingSink};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

    fn context(data: &std::path::Path, server: &MockServer) -> super::super::Context {
        let mut ctx = super::super::Context::new(
            std::sync::Arc::new(RecordingSink::default()),
            crate::exec::CancellationToken::new(),
        );
        ctx.umu = Some(UmuBootstrap::new(data).with_url(format!("{}/umu.tar", server.uri())));
        ctx
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn umu_downloads_when_a_program_first_runs_and_its_failure_fails_only_that_run() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let data = tempfile::tempdir().unwrap();
        let ctx = context(data.path(), &server);
        let host = super::super::input::host(Some("linux-no-umu")).unwrap();
        let runtime = prefix_runtime(
            &RuntimeSpec::ProtonPrefix { id: "game".into() },
            &host,
            &ctx,
        )
        .unwrap();
        assert!(server.received_requests().await.unwrap().is_empty());
        let command = ProcessCommand {
            program: "setup.exe".into(),
            args: vec![],
            env: Default::default(),
            cwd: data.path().into(),
        };
        let error = tokio::task::spawn_blocking(move || {
            let first = runtime.spawn(&command).unwrap_err();
            // The outcome is kept: a second program does not download again.
            let second = runtime.spawn(&command).unwrap_err();
            assert_eq!(first, second);
            first
        })
        .await
        .unwrap();
        assert!(
            matches!(&error, RuntimeError::Io { message } if message.contains("umu-launcher download failed")),
            "{error}"
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_installed_umu_is_used_without_the_network() {
        let server = MockServer::start().await;
        let data = tempfile::tempdir().unwrap();
        let umu = UmuBootstrap::new(data.path()).with_url(format!("{}/umu.tar", server.uri()));
        std::fs::create_dir_all(umu.umu_run().parent().unwrap()).unwrap();
        std::fs::write(umu.umu_run(), "#!/bin/sh\n").unwrap();
        drop(server);
        assert_eq!(umu.ensure().await.unwrap(), umu.umu_run());
        // Planning against it registers a plain Proton runtime.
        let ctx = super::super::Context::new(
            std::sync::Arc::new(RecordingSink::default()),
            crate::exec::CancellationToken::new(),
        );
        let mut host = super::super::input::host(Some("linux-no-umu")).unwrap();
        host.runners.push(crate::host::RunnerInfo {
            kind: RunnerKind::Proton,
            program: umu.umu_run(),
        });
        assert!(
            prefix_runtime(
                &RuntimeSpec::ProtonPrefix { id: "game".into() },
                &host,
                &ctx
            )
            .is_ok()
        );
    }
}
