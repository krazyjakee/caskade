//! Shared host/prefix relocation backends for CLI and application storage.
#[cfg(not(windows))]
use crate::relocate::backends::NoShortcuts;
use crate::{
    RuntimeSpec,
    disk::FileSystemDiskSpace,
    events::EventSink,
    host::{HostCaps, RunnerKind},
    manifest::InstallManifest,
    reconcile::{self as maintenance, ReconcileError},
    relocate::{
        RegistryStore, Relocator, RunningCheck, ShortcutEditor,
        backends::{NoRegistry, PrefixRegistry},
    },
    runtime::{
        NativeRuntime, Runtime,
        proton::{Launcher, PrefixRuntime},
    },
};
use std::{fs, path::Path, sync::Arc};
/// Development CLI observation; application storage supplies its authoritative
/// game process tracker through the same RunningCheck seam.
struct Processes;
impl RunningCheck for Processes {
    fn running(&self, game: &Path) -> bool {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        system.processes().iter().any(|(pid, process)| {
            pid.as_u32() != std::process::id()
                && [process.exe(), process.cwd()]
                    .into_iter()
                    .flatten()
                    .any(|path| dunce::canonicalize(path).is_ok_and(|path| path.starts_with(game)))
        })
    }
}

pub fn engine(
    game: Option<(&Path, &InstallManifest)>,
    host: &HostCaps,
    sink: Arc<dyn EventSink>,
) -> maintenance::Result<Arc<Relocator>> {
    engine_at(game, None, host, sink)
}

/// Validate the source prefix before the move, but bind all mutation scratch
/// and registry paths to the destination where the repair transaction runs.
pub fn engine_for_move(
    source: &Path,
    destination: &Path,
    manifest: &InstallManifest,
    host: &HostCaps,
    sink: Arc<dyn EventSink>,
) -> maintenance::Result<Arc<Relocator>> {
    engine_at(Some((source, manifest)), Some(destination), host, sink)
}

fn engine_at(
    game: Option<(&Path, &InstallManifest)>,
    destination: Option<&Path>,
    host: &HostCaps,
    sink: Arc<dyn EventSink>,
) -> maintenance::Result<Arc<Relocator>> {
    let native: Arc<dyn Runtime> = Arc::new(NativeRuntime::new());
    let registry: Arc<dyn RegistryStore> = match game {
        Some((path, manifest)) => match &manifest.record().runtime {
            RuntimeSpec::Native => {
                #[cfg(windows)]
                {
                    Arc::new(crate::relocate::backends::WindowsRegistry {
                        runtime: native.clone(),
                        scratch: destination.unwrap_or(path).join(".caskade/reconcile"),
                    })
                }
                #[cfg(not(windows))]
                {
                    let _ = path;
                    if manifest
                        .record()
                        .relocation
                        .references
                        .iter()
                        .any(|reference| {
                            matches!(reference.location, crate::scan::Location::Registry { .. })
                        })
                    {
                        return Err(ReconcileError::Invalid(
                            "native Windows registry references require a Windows backend".into(),
                        ));
                    }
                    Arc::new(NoRegistry)
                }
            }
            RuntimeSpec::WinePrefix { .. } | RuntimeSpec::ProtonPrefix { .. } => {
                let launcher = match &manifest.record().runtime {
                    RuntimeSpec::WinePrefix { .. } => Launcher::Wine {
                        wine: host
                            .runners
                            .iter()
                            .find(|runner| runner.kind == RunnerKind::Wine)
                            .ok_or_else(|| {
                                ReconcileError::Invalid(
                                    "Wine is unavailable for this game's prefix".into(),
                                )
                            })?
                            .program
                            .clone(),
                    },
                    RuntimeSpec::ProtonPrefix { id } => Launcher::Umu {
                        umu_run: host
                            .runners
                            .iter()
                            .find(|runner| runner.kind == RunnerKind::Proton)
                            .ok_or_else(|| {
                                ReconcileError::Invalid(
                                    "umu is unavailable for this game's prefix".into(),
                                )
                            })?
                            .program
                            .clone(),
                        proton_path: crate::host::automatic_proton(&host.proton_builds)
                            .unwrap_or_else(|| crate::host::UMU_PROTON.into()),
                        game_id: id.clone(),
                    },
                    RuntimeSpec::Native => unreachable!(),
                };
                let prefix = path.join(".caskade/prefix");
                if !prefix.is_dir()
                    || fs::symlink_metadata(&prefix)?.file_type().is_symlink()
                    || !dunce::canonicalize(&prefix)?.starts_with(path)
                {
                    return Err(ReconcileError::Invalid(
                        "CLI prefix must be a real game-local .caskade/prefix directory".into(),
                    ));
                }
                Arc::new(PrefixRegistry {
                    runtime: Arc::new(PrefixRuntime::new(launcher)),
                    prefix: destination.unwrap_or(path).join(".caskade/prefix"),
                    game_dir: destination.unwrap_or(path).into(),
                })
            }
        },
        None => Arc::new(NoRegistry),
    };
    #[cfg(windows)]
    let shortcuts: Arc<dyn ShortcutEditor> =
        Arc::new(crate::relocate::backends::ShellShortcuts { runtime: native });
    #[cfg(not(windows))]
    let shortcuts: Arc<dyn ShortcutEditor> = {
        let _ = native;
        Arc::new(NoShortcuts)
    };
    Ok(Arc::new(Relocator {
        disk: Arc::new(FileSystemDiskSpace),
        sink,
        registry,
        shortcuts,
        running: Arc::new(Processes),
    }))
}

/// A live per-game repair factory, shared by managed-root adoption and the app.
pub fn reconciler(
    host: HostCaps,
    sink: Arc<dyn EventSink>,
    aliases: Option<crate::runtime::alias::AliasStore>,
) -> maintenance::Result<maintenance::Reconciler> {
    let base = engine(None, &host, sink.clone())?;
    let factory = move |path: &Path, manifest: &InstallManifest| {
        engine(Some((path, manifest)), &host, sink.clone())
    };
    let mut reconciler = maintenance::Reconciler::new(base).with_repair_factory(Arc::new(factory));
    if let Some(aliases) = aliases {
        reconciler = reconciler.with_aliases(aliases);
    }
    Ok(reconciler)
}
