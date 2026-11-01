use super::{executable, icon};
use crate::{
    Confidence, DetectedGame, Evidence, GameRuntime, HostEnv, LaunchTarget, ScanCtx, SourceError,
    SourceId, TrackingHint,
    classify::{self, Metadata},
    parsers::desktop::DesktopEntry,
    uri,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct DesktopFile {
    pub id: String,
    pub path: PathBuf,
    pub entry: DesktopEntry,
}

/// Parsed, visible OS application data shared with emulator probes (#102).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopApplication {
    pub desktop_id: String,
    pub desktop_file: PathBuf,
    pub title: String,
    pub categories: Vec<String>,
    pub launch: LaunchTarget,
    pub install_dir: Option<PathBuf>,
    pub icon: Option<PathBuf>,
    pub flatpak_id: Option<String>,
    pub snap_instance: Option<String>,
}
impl DesktopApplication {
    pub fn emulator_only(&self) -> bool {
        self.categories.iter().any(|c| c == "Emulator")
            && !self.categories.iter().any(|c| {
                matches!(
                    c.as_str(),
                    "ActionGame"
                        | "AdventureGame"
                        | "ArcadeGame"
                        | "BoardGame"
                        | "BlocksGame"
                        | "CardGame"
                        | "KidsGame"
                        | "LogicGame"
                        | "RolePlaying"
                        | "Shooter"
                        | "Simulation"
                        | "SportsGame"
                        | "StrategyGame"
                )
            })
    }
}

pub(crate) fn locale(env: &HostEnv) -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| {
            env.var(key)
                .and_then(|s| s.into_string().ok())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "C".into())
}
pub(crate) fn visible(entry: &DesktopEntry) -> bool {
    entry.get("Type") == Some("Application")
        && !entry.hidden()
        && entry.get("NoDisplay") != Some("true")
}
pub(crate) fn application(
    env: &HostEnv,
    file: &DesktopFile,
    ctx: &ScanCtx,
) -> Option<DesktopApplication> {
    let entry = &file.entry;
    if !visible(entry) {
        return None;
    }
    if entry
        .try_exec()
        .is_some_and(|name| executable(env, name).is_none())
    {
        return None;
    }
    let locale = locale(env);
    let title = entry.localized("Name", &locale)?.trim().to_owned();
    if title.is_empty() {
        return None;
    }
    let argv = match entry.argv(&locale, file.path.to_str()) {
        Ok(argv) => argv,
        Err(error) => {
            ctx.warn(format!("{}: {error}", file.path.display()));
            return None;
        }
    };
    let (program, args, variables) = unwrap_env(argv)?;
    let flatpak_id = match entry.get("X-Flatpak") {
        Some(id) if super::super::flatpak::valid_id(id) => Some(id.to_owned()),
        Some(_) => return None,
        None => flatpak_command(&program, &args).map(|(id, _)| id.to_owned()),
    };
    let cwd = entry
        .get("Path")
        .filter(|s| s.starts_with('/') || Path::new(s).is_absolute())
        .and_then(|s| env.resolve_path(Path::new(s)).ok());
    let program = if program.starts_with('/') || Path::new(&program).is_absolute() {
        match env.resolve_path(Path::new(&program)) {
            Ok(path) => path,
            Err(error) => {
                ctx.warn(format!("{}: Exec path: {error}", file.path.display()));
                return None;
            }
        }
    } else {
        executable(env, &program).unwrap_or_else(|| PathBuf::from(&program))
    };
    let snap_root = env
        .resolve_path(Path::new("/var/lib/snapd/desktop/applications"))
        .ok();
    let snap_bin = env.resolve_path(Path::new("/snap/bin")).ok();
    let snap_instance = entry
        .get("X-SnapInstanceName")
        .filter(|s| valid_snap(s))
        .map(str::to_owned)
        .or_else(|| {
            if snap_root
                .as_ref()
                .is_some_and(|root| file.path.starts_with(root))
                || snap_bin
                    .as_ref()
                    .is_some_and(|root| program.starts_with(root))
            {
                Some(
                    file.id
                        .trim_end_matches(".desktop")
                        .split('_')
                        .next()
                        .unwrap_or(&file.id)
                        .to_owned(),
                )
            } else {
                None
            }
        });
    let install_dir = if flatpak_id.is_some() {
        None
    } else {
        candidate_dir(env, cwd.as_deref(), &program)
    };
    let launch = if let Some(id) = &flatpak_id {
        let args = flatpak_command(&program.to_string_lossy(), &args)
            .map(|(_, args)| {
                args.iter()
                    .filter(|s| s.as_str() != "@@" && s.as_str() != "@@u")
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        LaunchTarget::Flatpak {
            app_id: id.clone(),
            args,
        }
    } else {
        LaunchTarget::Exec {
            program,
            args,
            cwd,
            env: variables,
        }
    };
    Some(DesktopApplication {
        desktop_id: file.id.clone(),
        desktop_file: file.path.clone(),
        title,
        categories: entry
            .get("Categories")
            .unwrap_or_default()
            .split(';')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        launch,
        install_dir,
        icon: entry.get("Icon").and_then(|name| icon(env, name, &[])),
        flatpak_id,
        snap_instance,
    })
}

fn valid_snap(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn unwrap_env(mut argv: Vec<String>) -> Option<(String, Vec<String>, BTreeMap<String, String>)> {
    if argv.is_empty() {
        return None;
    }
    let program = argv.remove(0);
    let mut variables = BTreeMap::new();
    if Path::new(&program).file_name().is_none_or(|p| p != "env") {
        return Some((program, argv, variables));
    }
    let mut count = 0;
    for arg in &argv {
        if arg == "--" {
            count += 1;
            break;
        }
        let Some((name, value)) = arg.split_once('=') else {
            break;
        };
        if name.is_empty()
            || !name
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
        {
            return None;
        }
        variables.insert(name.into(), value.into());
        count += 1;
    }
    let mut tail = argv.split_off(count);
    if tail.is_empty() || tail[0].starts_with('-') {
        return None;
    }
    Some((tail.remove(0), tail, variables))
}
fn flatpak_command<'a>(program: &str, args: &'a [String]) -> Option<(&'a str, &'a [String])> {
    if Path::new(program)
        .file_name()
        .is_none_or(|p| p != "flatpak")
        || args.first().is_none_or(|s| s != "run")
    {
        return None;
    }
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            index += 1;
            break;
        }
        if !arg.starts_with('-') {
            break;
        }
        if matches!(
            arg.as_str(),
            "--arch" | "--branch" | "--command" | "--cwd" | "--env" | "--unset-env"
        ) {
            index += 1;
            args.get(index)?;
        }
        index += 1;
    }
    let id = args.get(index)?;
    super::super::flatpak::valid_id(id).then(|| (id.as_str(), &args[index + 1..]))
}
fn candidate_dir(env: &HostEnv, cwd: Option<&Path>, program: &Path) -> Option<PathBuf> {
    if program
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("appimage"))
    {
        return None;
    }
    let path = cwd.or_else(|| program.is_absolute().then(|| program.parent()).flatten())?;
    let path = env.resolve_path(path).ok()?;
    if [
        "/",
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/games",
        "/usr/share/games",
        "/opt",
        "/snap/bin",
    ]
    .into_iter()
    .filter_map(|p| env.resolve_path(Path::new(p)).ok())
    .any(|p| p == path)
    {
        return None;
    }
    path.is_dir().then_some(path)
}
pub(crate) fn proxy(app: &DesktopApplication) -> bool {
    !uri::target_identities(&app.launch).is_empty()
}

pub(crate) fn game(
    env: &HostEnv,
    ctx: &ScanCtx,
    app: DesktopApplication,
    source: SourceId,
) -> Result<Option<DetectedGame>, SourceError> {
    ctx.check()?;
    let identities = uri::target_identities(&app.launch);
    let mut game = DetectedGame::new(source, app.desktop_id.clone(), app.title.clone());
    game.confidence = Confidence::Likely;
    game.runtime = GameRuntime::Native;
    game.install_dir = app.install_dir.clone();
    game.icon = app.icon.clone();
    game.evidence.push(Evidence::File {
        path: app.desktop_file.clone(),
    });
    game.launch.push(app.launch.clone());
    if !identities.is_empty() {
        game.ids = identities
            .into_iter()
            .filter_map(|id| match id {
                uri::UriIdentity::External(id) => Some(id),
                _ => None,
            })
            .collect();
        uri::mark_proxy(&mut game);
        return Ok(Some(game));
    }
    if app.emulator_only() {
        return Ok(None);
    }
    let executable = match &app.launch {
        LaunchTarget::Exec { program, .. } => Some(program.clone()),
        _ => None,
    };
    let metadata = Metadata {
        title: Some(app.title),
        category: Some(app.categories.join(";")),
        install_dir: app.install_dir,
        executable: executable.clone(),
        ..Metadata::default()
    };
    let has_game = app.categories.iter().any(|s| s == "Game");
    let classified = if has_game {
        classify::classify_inventory(&metadata, &[], &ctx.config().classifier)
    } else {
        let path = metadata.install_dir.as_deref().or(executable.as_deref());
        let Some(path) = path.filter(|path| deep_enabled(env, ctx, path)) else {
            return Ok(None);
        };
        let host = profile(env);
        classify::classify_path(env, &host, path, &metadata, &ctx.config().classifier)
    };
    let classified = match classified {
        Ok(classified) => classified,
        Err(error) => {
            ctx.warn(format!("{}: {error}", app.desktop_file.display()));
            return Ok(None);
        }
    };
    if classified
        .evidence
        .iter()
        .any(|e| matches!(e,Evidence::Rule{rule,..} if rule.starts_with("category.")))
    {
        return Ok(None);
    }
    if has_game {
        game.confidence = Confidence::Likely;
    } else {
        let Some(confidence) = classified.confidence() else {
            return Ok(None);
        };
        game.confidence = confidence;
    }
    for warning in classified.warnings {
        ctx.warn(warning)
    }
    game.evidence.extend(classified.evidence);
    game.tracking = Some(TrackingHint {
        install_dir: game.install_dir.clone(),
        exe_names: executable
            .and_then(|p| p.file_name().and_then(|p| p.to_str()).map(str::to_owned))
            .into_iter()
            .collect(),
        ..TrackingHint::default()
    });
    Ok(Some(game))
}
fn deep_enabled(env: &HostEnv, ctx: &ScanCtx, path: &Path) -> bool {
    let config = &ctx.config().deep;
    config.portable
        || config.full_disk
        || config
            .folders
            .iter()
            .filter_map(|p| env.resolve_path(p).ok())
            .any(|root| path.starts_with(root))
        || (config.external_drives
            && env
                .volumes()
                .unwrap_or_default()
                .iter()
                .any(|v| v.kind == crate::env::VolumeKind::Removable && path.starts_with(&v.root)))
}
fn profile(env: &HostEnv) -> caskade_installer::host::HostCaps {
    caskade_installer::host::HostCaps {
        os: env.os(),
        arch: std::env::consts::ARCH.into(),
        runners: vec![],
        seven_zip: None,
        proton_builds: vec![],
        filesystem: caskade_installer::host::FileSystemCaps {
            writable: false,
            exec_bits: true,
            symlinks: true,
            case_sensitive: true,
        },
        packages: Default::default(),
    }
}
