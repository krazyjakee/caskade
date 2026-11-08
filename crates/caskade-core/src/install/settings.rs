//! Fresh per-job settings and the managed-prefix launch boundary.
use super::*;
use crate::launch::{LaunchError, RunnerPaths, RunnerResolver};
use crate::settings::{InstallerSettings, SettingsService};
use caskade_installer::{
    host::{HostCaps, RunnerKind, automatic_proton_build, proton_build_name},
    pipeline::CompatibilityRunner,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

impl InstallService {
    pub fn with_settings(self, settings: SettingsService) -> Self {
        *lock(&self.inner.settings) = Some(settings);
        self
    }
    pub(super) async fn preferences(&self) -> Result<InstallerSettings> {
        let settings = lock(&self.inner.settings).clone();
        match settings {
            None => Ok(InstallerSettings {
                concurrent_downloads: self.inner.options.downloads.clamp(1, 8) as u32,
                ..Default::default()
            }),
            Some(settings) => serde_json::from_value(
                settings
                    .get("core.installer")
                    .await
                    .map_err(|e| Error::Invalid(e.to_string()))?
                    .value,
            )
            .map_err(Error::from),
        }
    }
    /// Explicit host injection for private fixture/profile tests.
    pub fn with_host(self, host: HostCaps) -> Self {
        *lock(&self.inner.host) = Some(host);
        self
    }
}

/// Resolve selected builds solely from an injected capability snapshot.
pub fn runner_paths(settings: &InstallerSettings, host: &HostCaps) -> Result<RunnerPaths> {
    if settings.compatibility_runner == CompatibilityRunner::SystemWine
        && !host.has_runner(RunnerKind::Wine)
    {
        return Err(Error::Invalid(crate::tr!(
            "system Wine is unavailable; install Wine or select another compatibility runner"
        )));
    }
    let mut filtered = host.clone();
    settings.compatibility_runner.configure_host(&mut filtered);
    let wine = host
        .runners
        .iter()
        .find(|r| r.kind == RunnerKind::Wine)
        .map(|r| r.program.clone())
        .unwrap_or_else(|| "wine".into());
    let umu_run = host
        .runners
        .iter()
        .find(|r| r.kind == RunnerKind::Proton)
        .map(|r| r.program.clone())
        .unwrap_or_else(|| "umu-run".into());
    let proton_path = settings
        .compatibility_runner
        .proton_path(&filtered)
        .map_err(|_| {
            Error::Invalid(crate::tr!(
                "the selected GE-Proton/Steam Proton build is not installed"
            ))
        })?;
    Ok(RunnerPaths {
        wine,
        crossover: None,
        umu_run,
        proton_path,
    })
}

pub struct InstallerRunnerResolver(pub SettingsService);
#[async_trait::async_trait]
impl RunnerResolver for InstallerRunnerResolver {
    async fn resolve(&self, entry: &crate::model::GameEntry) -> crate::launch::Result<RunnerPaths> {
        let mut settings: InstallerSettings = serde_json::from_value(
            self.0
                .get("core.installer")
                .await
                .map_err(|e| LaunchError::Invalid(e.to_string()))?
                .value,
        )
        .map_err(|e| LaunchError::Invalid(e.to_string()))?;
        let wine = matches!(&entry.runtime,caskade_discovery::GameRuntime::Windows{runner:Some(r),..} if matches!(r.kind,caskade_discovery::RunnerKind::Wine|caskade_discovery::RunnerKind::CrossOver));
        // Existing prefix families are authoritative. Switching the default
        // runner affects new jobs, never converts an installed prefix.
        if wine {
            settings.compatibility_runner = CompatibilityRunner::SystemWine;
        } else if settings.compatibility_runner == CompatibilityRunner::SystemWine {
            settings.compatibility_runner = CompatibilityRunner::Proton;
        }
        let host = tokio::task::spawn_blocking(HostCaps::detect)
            .await
            .map_err(|e| LaunchError::Invalid(e.to_string()))?;
        // The Proton build the launch action chose, when this device has it;
        // without one (or without it here) the best installed build runs.
        let chosen = proton_choice(entry).filter(|_| !wine).and_then(|name| {
            let build = host
                .proton_builds
                .iter()
                .find(|path| proton_build_name(path) == name);
            if build.is_none() {
                tracing::warn!(
                    build = name,
                    "the launch action's Proton build isn't installed here; using the best installed one"
                );
            }
            build
        });
        if chosen.is_some() {
            settings.compatibility_runner = CompatibilityRunner::Proton;
        }
        let mut paths =
            runner_paths(&settings, &host).map_err(|e| LaunchError::Invalid(e.to_string()))?;
        if let Some(build) = chosen {
            paths.proton_path = build.to_string_lossy().into_owned();
        }
        let paths = if !wine && !host.has_runner(RunnerKind::Proton) {
            #[cfg(feature = "net")]
            {
                let bootstrap = caskade_installer::runtime::umu::UmuBootstrap::for_host()
                    .ok_or_else(|| {
                        LaunchError::Invalid(crate::tr!("umu data directory unavailable"))
                    })?;
                let program = bootstrap
                    .ensure()
                    .await
                    .map_err(|e| LaunchError::Invalid(e.to_string()))?;
                RunnerPaths {
                    umu_run: program,
                    ..paths
                }
            }
            #[cfg(not(feature = "net"))]
            return Err(LaunchError::Invalid(crate::tr!(
                "umu-launcher is unavailable; install it or enable runtime downloads"
            )));
        } else {
            paths
        };
        Ok(paths)
    }
}

/// The Proton build a Windows entry's runner names (`GE-Proton10-34`), as a
/// launch action sets it (`launch::actions::apply`).
fn proton_choice(entry: &crate::model::GameEntry) -> Option<&str> {
    match &entry.runtime {
        caskade_discovery::GameRuntime::Windows {
            runner: Some(runner),
            ..
        } if matches!(
            runner.kind,
            caskade_discovery::RunnerKind::Umu | caskade_discovery::RunnerKind::Proton
        ) =>
        {
            runner.name.as_deref()
        }
        _ => None,
    }
}

/// The Proton builds a launch action can choose on this device (#177).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProtonBuilds {
    /// Installed builds by name, best first.
    pub installed: Vec<String>,
    /// The build an action without its own runs now, under the installer's
    /// compatibility runner setting. `None` when none is installed: umu
    /// downloads UMU-Proton (or, for GE-Proton or Steam Proton, the launch
    /// says it is missing).
    pub automatic: Option<String>,
}

/// The Proton builds installed on this device. Empty off Linux.
pub async fn proton_builds(settings: &SettingsService) -> Result<ProtonBuilds> {
    let mut preferences: InstallerSettings = serde_json::from_value(
        settings
            .get("core.installer")
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?
            .value,
    )?;
    if preferences.compatibility_runner == CompatibilityRunner::SystemWine {
        preferences.compatibility_runner = CompatibilityRunner::Proton;
    }
    let host = tokio::task::spawn_blocking(HostCaps::detect)
        .await
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let mut filtered = host.clone();
    preferences
        .compatibility_runner
        .configure_host(&mut filtered);
    let mut installed: Vec<String> = vec![];
    for name in host
        .proton_builds
        .iter()
        .map(|path| proton_build_name(path))
    {
        if !installed.contains(&name) {
            installed.push(name);
        }
    }
    Ok(ProtonBuilds {
        installed,
        automatic: automatic_proton_build(&filtered.proton_builds)
            .map(|path| proton_build_name(path)),
    })
}
