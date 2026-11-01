//! What a file or folder dropped on the library installs as. A shortcut
//! (`.lnk`, `.url`, `.desktop`) stands for its target. A game's own
//! executable stands for the folder it lives in, with that executable as the
//! launch target. Installers, archives, packages and folders install as they
//! are. Planning only reads: nothing is copied, run or resolved online.

use crate::{Error, Result};
use caskade_discovery::{
    Cancel, HostEnv, LaunchTarget, ScanCtx,
    parsers::{self, Limits},
    sources::{desktop, startmenu},
    uri,
};
use caskade_installer::{
    Format, Role,
    discover::{DiscoveryHints, LaunchHint},
    probe::{ProbeEngine, ProbeLimits},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// A drop, ready to install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DropPlan {
    /// What was dropped.
    pub dropped: String,
    pub kind: DropKind,
    /// What gets installed: a file or a folder.
    pub input: String,
    /// The shortcut's name, when a shortcut was dropped.
    pub title: Option<String>,
    /// The executable to launch, when the drop named one.
    pub launch: Option<DropLaunch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DropKind {
    Shortcut,
    /// A game's own executable; its folder is installed.
    Executable,
    Folder,
    /// An installer, archive, package or anything else installed as it is.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DropLaunch {
    /// Relative to the installed folder, `/`-separated.
    pub path: String,
    pub args: Vec<String>,
    /// Relative to the installed folder; `None` is the executable's folder.
    pub working_dir: Option<String>,
}

impl DropPlan {
    /// The plan as installer discovery hints: the shortcut's title outranks
    /// what the files say, and the launch target outranks discovery's guess.
    pub fn hints(&self) -> DiscoveryHints {
        DiscoveryHints {
            installer_title: self.title.clone(),
            launch_targets: self
                .launch
                .iter()
                .map(|launch| LaunchHint {
                    path: launch.path.clone().into(),
                    args: launch.args.clone(),
                    working_dir: launch.working_dir.clone().map(PathBuf::from),
                    reason: "dropped".into(),
                })
                .collect(),
            ..Default::default()
        }
    }
}

/// Plans an install of the dropped `path`. Errors say why it can't be
/// installed (a launcher's shortcut, an uninstaller, an executable that has
/// no folder of its own).
pub fn plan(path: &Path) -> Result<DropPlan> {
    plan_at(path, &HostEnv::live(), &user_dirs())
}

fn plan_at(path: &Path, env: &HostEnv, user_dirs: &[PathBuf]) -> Result<DropPlan> {
    if !path.is_absolute() {
        return Err(Error::Invalid(crate::tr!(
            "a dropped path must be absolute"
        )));
    }
    refuse_network(path)?;
    let metadata = std::fs::metadata(path)?;
    let dropped = path.to_string_lossy().into_owned();
    if metadata.is_dir() {
        return Ok(DropPlan {
            dropped: dropped.clone(),
            kind: DropKind::Folder,
            input: dropped,
            title: None,
            launch: None,
        });
    }
    if !metadata.is_file() {
        return Err(Error::Invalid(crate::tr!(
            "{name} isn't a file or folder",
            name = name(path)
        )));
    }
    let shortcut = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["lnk", "url", "desktop"].contains(&e.to_ascii_lowercase().as_str()));
    if !shortcut {
        return file(path, None, user_dirs);
    }
    let (title, target) = read_shortcut(env, path)?;
    let mut plan = match target {
        Target::Exec {
            program, args, cwd, ..
        } => file(&program, Some((args, cwd)), user_dirs)?,
        // Wine's menu entries run another shortcut inside the prefix.
        Target::Shortcut(inner) => plan_at(&inner, env, user_dirs)?,
        Target::Url(url) => DropPlan {
            dropped: String::new(),
            kind: DropKind::Shortcut,
            input: url,
            title: None,
            launch: None,
        },
    };
    plan.dropped = dropped;
    plan.kind = DropKind::Shortcut;
    plan.title = Some(title);
    Ok(plan)
}

/// A shortcut's game, to add in place rather than install (#174): its
/// title and what it starts, with the shortcut's arguments, folder and
/// environment. A Wine menu entry's game comes with its `WINEPREFIX`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShortcutTarget {
    pub title: String,
    pub launch: crate::model::LaunchSpec,
}

/// What the shortcut at `path` (`.lnk`, `.url`, `.desktop`) starts. Like a
/// drop, a launcher's or a Flatpak's shortcut is refused: discovery finds
/// those games itself.
pub fn shortcut_target(path: &Path) -> Result<ShortcutTarget> {
    shortcut_target_at(path, &HostEnv::live())
}

fn shortcut_target_at(path: &Path, env: &HostEnv) -> Result<ShortcutTarget> {
    let (title, target) = read_shortcut(env, path)?;
    let launch = match target {
        Target::Exec {
            program,
            args,
            cwd,
            env,
        } => crate::model::LaunchSpec::Executable {
            path: program,
            args,
            cwd,
            env,
        },
        Target::Shortcut(inner) => shortcut_target_at(&inner, env)?.launch,
        Target::Url(uri) => crate::model::LaunchSpec::Uri { uri },
    };
    Ok(ShortcutTarget { title, launch })
}

enum Target {
    Exec {
        program: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: BTreeMap<String, String>,
    },
    Shortcut(PathBuf),
    /// A web link, installed like a pasted URL.
    Url(String),
}

fn read_shortcut(env: &HostEnv, path: &Path) -> Result<(String, Target)> {
    let ctx = ScanCtx::new(Cancel::new());
    let unreadable = |error: caskade_discovery::SourceError| Error::Invalid(error.to_string());
    let not_a_game = || {
        Error::Invalid(crate::tr!(
            "{name} isn't a shortcut to a game",
            name = name(path)
        ))
    };
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    // The Start Menu reader keeps only launcher links; a web link installs
    // like a pasted URL.
    if extension.as_deref() == Some("url")
        && let Ok(bytes) = std::fs::read(path)
        && let Ok(link) = parsers::url::parse(&bytes, &Limits::default())
        && (link.url.starts_with("https://") || link.url.starts_with("http://"))
    {
        let title = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok((title, Target::Url(link.url)));
    }
    let is_desktop = extension.as_deref() == Some("desktop");
    let (title, launch) = if is_desktop {
        let app = desktop::desktop_file(env, &ctx, path)
            .map_err(unreadable)?
            .ok_or_else(not_a_game)?;
        (app.title, app.launch)
    } else {
        let app = startmenu::shortcut_file(env, &ctx, path)
            .map_err(unreadable)?
            .ok_or_else(not_a_game)?;
        (app.title, app.launch)
    };
    if !uri::target_identities(&launch).is_empty() {
        return Err(Error::Invalid(crate::tr!(
            "{title} starts through a launcher. Caskade finds launchers' games itself: \
             turn it on in Settings → Launchers.",
            title = title
        )));
    }
    match launch {
        LaunchTarget::Exec {
            program, args, env, ..
        } if is_wine(&program) => {
            let inner = wine_target(&args, &env).ok_or_else(|| {
                Error::Invalid(crate::tr!(
                    "{title} runs Wine without a game",
                    title = title
                ))
            })?;
            Ok((title, inner))
        }
        LaunchTarget::Exec {
            program,
            args,
            cwd,
            env,
            ..
        } => Ok((
            title,
            Target::Exec {
                program,
                args,
                cwd,
                env,
            },
        )),
        LaunchTarget::Flatpak { app_id, .. } => Err(Error::Invalid(crate::tr!(
            "{title} is the Flatpak app {app_id}. Caskade finds Flatpak games with \
             library discovery (Settings → Library Discovery).",
            title = title,
            app_id = app_id
        ))),
        LaunchTarget::Uri { .. } => Err(not_a_game()),
    }
}

fn is_wine(program: &Path) -> bool {
    program
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "wine" || n == "wine64")
}

/// `wine C:\…\Game.exe` (or `.lnk`) in a menu entry Wine wrote: the path in
/// the entry's `WINEPREFIX`, or `~/.wine`.
fn wine_target(args: &[String], env: &BTreeMap<String, String>) -> Option<Target> {
    let windows = args.iter().find(|arg| !arg.starts_with('/'))?;
    let prefix = env
        .get("WINEPREFIX")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".wine")))?;
    let rest = windows
        .strip_prefix("C:")
        .or_else(|| windows.strip_prefix("c:"))?;
    let path = rest
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .fold(prefix.join("drive_c"), |path, part| path.join(part));
    let rest_args = args
        .iter()
        .skip_while(|arg| *arg != windows)
        .skip(1)
        .cloned()
        .collect();
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
    {
        Some(Target::Shortcut(path))
    } else {
        Some(Target::Exec {
            program: path,
            args: rest_args,
            cwd: None,
            env: env.clone(),
        })
    }
}

/// A file the user dropped, or a shortcut's target. `shortcut` carries the
/// shortcut's arguments and working directory.
fn file(
    path: &Path,
    shortcut: Option<(Vec<String>, Option<PathBuf>)>,
    user_dirs: &[PathBuf],
) -> Result<DropPlan> {
    refuse_network(path)?;
    if !path.is_file() {
        return Err(Error::Invalid(crate::tr!(
            "{path} doesn't exist any more",
            path = path.display()
        )));
    }
    let probe = ProbeEngine::with_builtin_formats(ProbeLimits::default())
        .probe_path(path)
        .map_err(|error| Error::Invalid(format!("{}: {error}", name(path))))?
        .root;
    let input = path.to_string_lossy().into_owned();
    match probe.role {
        Role::Uninstaller => {
            return Err(Error::Invalid(crate::tr!(
                "{name} is an uninstaller, not a game",
                name = name(path)
            )));
        }
        Role::Redistributable => {
            return Err(Error::Invalid(crate::tr!(
                "{name} installs a runtime library, not a game",
                name = name(path)
            )));
        }
        Role::Application | Role::Launcher
            if matches!(
                probe.format,
                Format::Pe | Format::Elf | Format::MachO | Format::Script
            ) => {}
        _ => {
            return Ok(DropPlan {
                dropped: input.clone(),
                kind: DropKind::File,
                input,
                title: None,
                launch: None,
            });
        }
    }
    let folder = game_folder(path);
    if user_dirs.iter().any(|dir| dir == &folder) || folder.parent().is_none() {
        return Err(Error::Invalid(crate::tr!(
            "{name} isn't in a folder of its own. Move the game into one, then drop it again.",
            name = name(path)
        )));
    }
    let relative = |p: &Path| {
        p.strip_prefix(&folder).ok().map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
    };
    let (args, cwd) = shortcut.unwrap_or_default();
    Ok(DropPlan {
        dropped: input,
        kind: DropKind::Executable,
        input: folder.to_string_lossy().into_owned(),
        title: None,
        launch: Some(DropLaunch {
            path: relative(path).unwrap_or_default(),
            args,
            working_dir: cwd.as_deref().and_then(relative).filter(|p| !p.is_empty()),
        }),
    })
}

/// The game's folder for one of its executables: the executable's folder, or
/// above it when that is a `bin`-style folder (`Game/Binaries/Win64/…`).
fn game_folder(executable: &Path) -> PathBuf {
    const BINARY_DIRS: &[&str] = &[
        "bin", "bin32", "bin64", "binaries", "x64", "x86", "x86_64", "amd64", "win32", "win64",
        "windows", "linux", "linux64", "macos",
    ];
    let mut folder = executable.parent().unwrap_or(executable).to_path_buf();
    while let (Some(name), Some(parent)) = (folder.file_name(), folder.parent()) {
        let name = name.to_string_lossy().to_ascii_lowercase();
        if !BINARY_DIRS.contains(&name.as_str()) || parent.parent().is_none() {
            break;
        }
        folder = parent.to_path_buf();
    }
    folder
}

/// Folders a game's files may sit in without being the game's own folder.
fn user_dirs() -> Vec<PathBuf> {
    let Some(dirs) = directories::UserDirs::new() else {
        return vec![];
    };
    [
        Some(dirs.home_dir()),
        dirs.desktop_dir(),
        dirs.download_dir(),
        dirs.document_dir(),
    ]
    .into_iter()
    .flatten()
    .map(Path::to_path_buf)
    .collect()
}

/// A shortcut may point at a share: on Windows, merely statting
/// `\\host\share` connects to that host over SMB.
fn refuse_network(path: &Path) -> Result<()> {
    let text = path.to_string_lossy();
    if text.starts_with("\\\\") || text.starts_with("//") {
        return Err(Error::Invalid(crate::tr!(
            "{path} is on a network share; copy the game to this computer first",
            path = path.display()
        )));
    }
    Ok(())
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
#[path = "dropped_tests.rs"]
mod tests;
