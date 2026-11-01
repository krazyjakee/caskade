use super::RunnerPaths;
use super::{LaunchCommand, LaunchError, PluginRunner, Result, managed_path, uri_command};
use crate::{
    model::{GameEntry, InstallLocation, LaunchSpec, StorageRoot},
    tracking::TrackingTarget,
};
use caskade_discovery::{GameRuntime, InstallState, Prefix, RunnerHint, RunnerKind};
use caskade_installer::runtime::{
    ProcessCommand,
    alias::AliasStore,
    proton::{Launcher, PrefixRuntime},
};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub struct PreparedLaunch {
    pub command: LaunchCommand,
    pub target: TrackingTarget,
    /// Direct executable children are known even if they exit between polls.
    pub direct: bool,
    preparation: Option<PrefixPreparation>,
}
struct PrefixPreparation {
    launcher: Launcher,
    prefix: PathBuf,
    game_dir: PathBuf,
}
impl PreparedLaunch {
    /// A launch with nothing to prepare: emulators and the game window.
    pub(crate) fn new(command: LaunchCommand, target: TrackingTarget, direct: bool) -> Self {
        Self {
            command,
            target,
            direct,
            preparation: None,
        }
    }

    /// Run prefix initialization only after launch.before accepted the launch.
    pub async fn prepare(&mut self) -> Result<()> {
        if let Some(setup) = self.preparation.take() {
            tokio::task::spawn_blocking(move || {
                PrefixRuntime::new(setup.launcher).prepare(&setup.prefix, &setup.game_dir)
            })
            .await
            .map_err(|e| LaunchError::Invalid(e.to_string()))?
            .map_err(|e| LaunchError::Invalid(e.to_string()))?;
        }
        Ok(())
    }
}

pub async fn resolve(
    entry: &GameEntry,
    root: Option<StorageRoot>,
    aliases: Option<AliasStore>,
    plugins: Arc<dyn PluginRunner>,
) -> Result<PreparedLaunch> {
    resolve_with_runners(entry, root, aliases, plugins, RunnerPaths::default()).await
}

pub async fn resolve_with_runners(
    entry: &GameEntry,
    root: Option<StorageRoot>,
    aliases: Option<AliasStore>,
    plugins: Arc<dyn PluginRunner>,
    runners: RunnerPaths,
) -> Result<PreparedLaunch> {
    if !matches!(entry.state, InstallState::Installed) {
        return Err(LaunchError::Invalid(crate::tr!("entry is not installed")));
    }
    let entry = entry.clone();
    let spec = entry
        .launch
        .clone()
        .ok_or_else(|| LaunchError::Invalid(crate::tr!("entry has no launch specification")))?;
    if let LaunchSpec::Plugin {
        plugin_id,
        runner_id,
        payload,
    } = &spec
    {
        let command = plugins.resolve(plugin_id, runner_id, payload).await?;
        command.validate()?;
        let target = TrackingTarget {
            executables: vec![command.program.clone()],
            ..Default::default()
        };
        return Ok(PreparedLaunch::new(command, target, true));
    }
    tokio::task::spawn_blocking(move || {
        let (real, alias) = locations(&entry, root.as_ref(), aliases.as_ref())?;
        let managed = matches!(entry.location, Some(InstallLocation::Managed { .. }));
        let mut target = TrackingTarget {
            real_dir: real.clone(),
            alias_dir: alias.clone(),
            ..Default::default()
        };
        if let Some(hint) = &entry.tracking {
            let mut hint = hint.clone();
            if managed && let (Some(dir), Some(path)) = (&real, &hint.install_dir) {
                hint.install_dir = Some(dir.join(path));
            }
            target = target.with_hint(&hint);
        }
        for id in &entry.ids {
            if let caskade_discovery::ExternalId::Steam { app_id } = id {
                target.steam_app_id = Some(*app_id);
            }
        }
        let mut direct = false;
        let mut command = match spec {
            LaunchSpec::Uri { uri } => uri_command(&uri)?,
            LaunchSpec::Flatpak { app_id, args } => {
                if !super::command::flatpak_id(&app_id) {
                    return Err(LaunchError::Invalid(crate::tr!("invalid Flatpak app id")));
                }
                let mut c = LaunchCommand::executable("flatpak");
                c.args = vec!["run".into(), app_id];
                c.args.extend(args);
                c
            }
            LaunchSpec::Executable {
                path,
                args,
                cwd,
                env,
            } => {
                // A launch action (#177) may run a tool outside a managed
                // install by its absolute path; an entry's own launch never
                // stores one.
                let managed = managed && !path.is_absolute();
                let program = if managed {
                    managed_path(
                        real.as_deref().expect("managed location"),
                        alias.as_deref(),
                        &path,
                        false,
                    )?
                } else if path.is_absolute() {
                    path
                } else {
                    real.as_ref()
                        .ok_or_else(|| {
                            LaunchError::Invalid(crate::tr!(
                                "relative executable has no install directory"
                            ))
                        })?
                        .join(path)
                };
                let cwd = if managed {
                    Some(managed_path(
                        real.as_deref().expect("managed location"),
                        alias.as_deref(),
                        cwd.as_deref().unwrap_or(Path::new(".")),
                        true,
                    )?)
                } else {
                    cwd.map(|p| {
                        if p.is_absolute() {
                            p
                        } else {
                            real.as_ref().map(|r| r.join(&p)).unwrap_or(p)
                        }
                    })
                    .or_else(|| real.clone())
                };
                direct = real.as_ref().is_none_or(|dir| {
                    std::fs::canonicalize(&program)
                        .ok()
                        .is_some_and(|exe| exe.starts_with(dir))
                });
                if direct {
                    target.executables.push(program.clone());
                }
                LaunchCommand {
                    emulation: None,
                    program,
                    args,
                    cwd,
                    env,
                }
            }
            LaunchSpec::Emulator { .. } => {
                return Err(LaunchError::Unsupported(
                    "emulator execution is outside M0–M2".into(),
                ));
            }
            LaunchSpec::Web { .. } => {
                return Err(LaunchError::Unsupported("web games".into()));
            }
            LaunchSpec::Plugin { .. } => unreachable!("handled before blocking work"),
        };
        let excluded_program = (!direct).then(|| command.program.clone());
        let mut preparation = None;
        if let GameRuntime::Windows { prefix, runner } = &entry.runtime
            && !cfg!(windows)
            && matches!(entry.launch, Some(LaunchSpec::Executable { .. }))
        {
            (command, preparation) = windows_command(
                &entry.game_id,
                prefix.as_ref(),
                runner.as_ref(),
                command,
                real.as_deref(),
                alias.as_deref(),
                managed,
                &runners,
            )?;
            if let Some(name) = target
                .executables
                .first()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
            {
                target.exe_names.push(name.into());
            }
        }
        if let Some(program) = excluded_program {
            target.excluded_executables.push(program);
        }
        command.validate()?;
        Ok(PreparedLaunch {
            command,
            target,
            direct,
            preparation,
        })
    })
    .await
    .map_err(|e| LaunchError::Invalid(e.to_string()))?
}

#[allow(clippy::too_many_arguments)]
fn windows_command(
    game_id: &str,
    prefix: Option<&Prefix>,
    runner: Option<&RunnerHint>,
    command: LaunchCommand,
    real: Option<&Path>,
    alias: Option<&Path>,
    managed: bool,
    runners: &RunnerPaths,
) -> Result<(LaunchCommand, Option<PrefixPreparation>)> {
    let kind = runner
        .map(|r| r.kind)
        .unwrap_or(if cfg!(target_os = "linux") {
            RunnerKind::Umu
        } else {
            RunnerKind::Wine
        });
    let configured = runner.and_then(|r| r.path.clone());
    let real = real
        .ok_or_else(|| LaunchError::Invalid(crate::tr!("Windows game has no install directory")))?;
    let prefix = prefix
        .map(|p| p.path.clone())
        .unwrap_or_else(|| PathBuf::from(".caskade/prefix"));
    let prefix = if managed {
        relative(&prefix)?;
        confined_prefix(real, &prefix)?
    } else if prefix.is_absolute() {
        prefix
    } else {
        real.join(prefix)
    };
    let launcher = match kind {
        RunnerKind::Wine => Launcher::Wine {
            wine: configured.unwrap_or_else(|| runners.wine.clone()),
        },
        RunnerKind::CrossOver => Launcher::Wine {
            wine: configured
                .or_else(|| runners.crossover.clone())
                .ok_or_else(|| {
                    LaunchError::Unsupported("CrossOver runner is not configured".into())
                })?,
        },
        RunnerKind::Proton => Launcher::Umu {
            umu_run: runners.umu_run.clone(),
            proton_path: configured
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| runners.proton_path.clone()),
            game_id: format!("caskade-{game_id}"),
        },
        RunnerKind::Umu => Launcher::Umu {
            umu_run: configured.unwrap_or_else(|| runners.umu_run.clone()),
            proton_path: runners.proton_path.clone(),
            game_id: format!("caskade-{game_id}"),
        },
        RunnerKind::Other => return Err(LaunchError::Unsupported("unknown Windows runner".into())),
    };
    let preparation = if managed {
        Some(PrefixPreparation {
            launcher: launcher.clone(),
            prefix: prefix.clone(),
            game_dir: real.into(),
        })
    } else {
        None
    };
    let runtime = PrefixRuntime::new(launcher);
    if !managed && !prefix.is_dir() {
        return Err(LaunchError::Invalid(crate::tr!(
            "external Wine prefix is missing"
        )));
    }
    let pc = ProcessCommand {
        program: command.program,
        args: command.args,
        cwd: command.cwd.unwrap_or_else(|| real.into()),
        env: command.env,
    };
    let mut wrapped = runtime.wrap(&pc, &prefix, alias.unwrap_or(real));
    if !managed {
        // External launcher prefixes do not own Caskade's G: mapping. Use
        // their existing drive mappings without adding or retargeting links.
        wrapped.args[0] = external_windows_path(&prefix, &pc.program)?;
    }
    Ok((
        LaunchCommand {
            emulation: None,
            program: wrapped.program,
            args: wrapped.args,
            cwd: Some(wrapped.cwd),
            env: wrapped.env,
        },
        preparation,
    ))
}

pub(super) fn locations(
    entry: &GameEntry,
    root: Option<&StorageRoot>,
    aliases: Option<&AliasStore>,
) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    match &entry.location {
        Some(InstallLocation::Managed { root_id, rel_dir }) => {
            let root = root
                .filter(|r| &r.id == root_id)
                .ok_or_else(|| {
                    LaunchError::Invalid(crate::tr!("The game's storage location isn't available. Reconnect its drive, then try again."))
                })?;
            relative(rel_dir)?;
            let marker =
                caskade_installer::manifest::RootManifest::read(&root.path).map_err(|e| {
                    LaunchError::Invalid(crate::tr!(
                        "storage root identity unavailable: {error}",
                        error = e
                    ))
                })?;
            if marker.root_id() != root_id {
                return Err(LaunchError::Invalid(crate::tr!(
                    "storage root identity changed"
                )));
            }
            let canonical = std::fs::canonicalize(&root.path)
                .map_err(|error| folder_error(error, "storage location", &root.path))?;
            let real = std::fs::canonicalize(canonical.join(rel_dir))
                .map_err(|error| folder_error(error, "game's folder", &canonical.join(rel_dir)))?;
            if !real.starts_with(&canonical) || real == canonical || !real.is_dir() {
                return Err(LaunchError::Invalid(crate::tr!(
                    "managed game directory escapes storage root"
                )));
            }
            let alias = match (aliases, &entry.alias_id) {
                (Some(store), Some(id)) => Some(
                    store
                        .link(id, &real)
                        .map_err(|e| LaunchError::Invalid(e.to_string()))?,
                ),
                _ => None,
            };
            Ok((Some(real), alias))
        }
        Some(InstallLocation::External { path }) => Ok((
            Some(
                std::fs::canonicalize(path)
                    .map_err(|error| folder_error(error, "game's folder", path))?,
            ),
            None,
        )),
        None => Ok((None, None)),
    }
}

/// `error` resolving `what` at `path`, naming the folder: a bare "No such
/// file or directory" doesn't say the game was moved or deleted (#160).
fn folder_error(error: std::io::Error, what: &str, path: &Path) -> LaunchError {
    let missing = error.kind() == std::io::ErrorKind::NotFound;
    let path = path.display();
    let text = match (what, missing) {
        ("storage location", true) => crate::tr!(
            "The storage location “{path}” is missing. It may have been moved, deleted or on a drive that isn't connected.",
            path = path
        ),
        ("storage location", false) => crate::tr!(
            "Can't open the storage location “{path}”: {error}",
            path = path,
            error = error
        ),
        (_, true) => crate::tr!(
            "The game's folder “{path}” is missing. It may have been moved, deleted or on a drive that isn't connected.",
            path = path
        ),
        (_, false) => crate::tr!(
            "Can't open the game's folder “{path}”: {error}",
            path = path,
            error = error
        ),
    };
    LaunchError::Io(std::io::Error::new(error.kind(), text))
}
fn relative(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(LaunchError::Invalid(crate::tr!(
            "managed location is not relative"
        )));
    }
    Ok(())
}

fn confined_prefix(real: &Path, relative_path: &Path) -> Result<PathBuf> {
    let root = std::fs::canonicalize(real)?;
    let mut path = root.clone();
    for component in relative_path.components() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                if !std::fs::canonicalize(&path)?.starts_with(&root) {
                    return Err(LaunchError::Invalid(crate::tr!(
                        "managed prefix escapes game directory"
                    )));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}

fn external_windows_path(prefix: &Path, program: &Path) -> Result<String> {
    let program = std::fs::canonicalize(program)?;
    let mut mappings = vec![];
    for item in std::fs::read_dir(prefix.join("dosdevices"))? {
        let item = item?;
        let name = item.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.len() != 2
            || name.as_bytes()[1] != b':'
            || !name.as_bytes()[0].is_ascii_alphabetic()
        {
            continue;
        }
        let Ok(root) = std::fs::canonicalize(item.path()) else {
            continue;
        };
        if let Ok(relative) = program.strip_prefix(&root) {
            let Some(relative) = relative.to_str() else {
                return Err(LaunchError::Invalid(crate::tr!(
                    "Wine executable path is not UTF-8"
                )));
            };
            mappings.push((
                root.components().count(),
                format!(
                    "{}:\\{}",
                    name.as_bytes()[0].to_ascii_uppercase() as char,
                    relative.replace('/', "\\")
                ),
            ));
        }
    }
    mappings.sort();
    mappings.pop().map(|(_, path)| path).ok_or_else(|| {
        LaunchError::Invalid(crate::tr!(
            "game executable is not mapped in the external Wine prefix"
        ))
    })
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
