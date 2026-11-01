//! Start Menu metadata discovery. Original code using the shared MS-SHLLINK
//! and InternetShortcut parsers; shortcuts are never executed or shell-resolved.
mod shortcut;
use crate::{
    DetectedGame, HostEnv, LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId,
    WatchPath, env::KnownFolder,
};
use std::{fs, path::PathBuf};
pub const ID: SourceId = SourceId::new("startmenu");
#[derive(Debug, Default)]
pub struct StartMenuSource;
impl StartMenuSource {
    pub fn new() -> Self {
        Self
    }
}
fn roots(env: &HostEnv) -> Vec<PathBuf> {
    let mut roots: Vec<_> = [KnownFolder::StartMenu, KnownFolder::CommonStartMenu]
        .into_iter()
        .filter_map(|key| env.known_folder(key).map(PathBuf::from))
        .collect();
    roots.sort();
    roots.dedup();
    roots
}
impl Source for StartMenuSource {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        roots(env)
            .into_iter()
            .map(|path| WatchPath {
                path,
                recursive: true,
            })
            .collect()
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut output = vec![];
        for app in applications(env, ctx)? {
            ctx.check()?;
            match shortcut::classify(env, ctx, app) {
                Ok(Some(game)) => output.push(game),
                Ok(None) => (),
                Err(error) => ctx.warn(error.to_string()),
            }
        }
        Ok(output)
    }
}

/// Parsed visible shortcut data for consumers that classify applications rather
/// than games. The same bounded walker/parser feeds StartMenuSource.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShortcutApplication {
    pub path: PathBuf,
    pub title: String,
    pub launch: crate::LaunchTarget,
    pub icon: Option<PathBuf>,
}
/// One `.lnk` or `.url` file the user handed over, e.g. by dropping it on the
/// app, read like the Start Menu's. Outside Windows, a link inside a Wine
/// prefix resolves against that prefix's drives. `None` for another file type
/// or a link that isn't a game's (an uninstaller, a manual, a missing target).
pub fn shortcut_file(
    env: &HostEnv,
    ctx: &ScanCtx,
    path: &std::path::Path,
) -> Result<Option<ShortcutApplication>, SourceError> {
    let Some(extension) = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| e == "lnk" || e == "url")
    else {
        return Ok(None);
    };
    let prefix = (env.os() != crate::OperatingSystem::Windows)
        .then(|| {
            path.ancestors()
                .skip(1)
                .find(|dir| super::prefixes::is_prefix(env, dir))
        })
        .flatten();
    let prefixed;
    let env = match prefix {
        Some(prefix) => {
            prefixed = env
                .wine_prefix(prefix)
                .map_err(|error| SourceError::io(prefix, error))?;
            &prefixed
        }
        None => env,
    };
    shortcut::read(env, ctx, path, &extension, &mut 0)
}
pub fn applications(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<ShortcutApplication>, SourceError> {
    ctx.check()?;
    let budget = ctx.config().classifier.max_entries.min(100_000) as usize;
    let mut count = 0usize;
    let mut bytes = 0usize;
    let mut output = vec![];
    let mut stack: Vec<_> = roots(env).into_iter().map(|root| (root, 0usize)).collect();
    while let Some((dir, depth)) = stack.pop() {
        ctx.check()?;
        let dir = match env.resolve_path(&dir) {
            Ok(dir) => dir,
            Err(error) => {
                ctx.warn(format!("{}: {error}", dir.display()));
                continue;
            }
        };
        match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => (),
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                ctx.warn(format!("{}: {error}", dir.display()));
                continue;
            }
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                ctx.warn(format!("{}: {error}", dir.display()));
                continue;
            }
        };
        let mut entries = entries
            .take(budget.saturating_sub(count).saturating_add(1))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| SourceError::io(&dir, error))?;
        count = count.saturating_add(entries.len());
        if count > budget {
            return Err(SourceError::Other {
                message: "Start Menu entry budget exceeded".into(),
            });
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            ctx.check()?;
            let kind = entry
                .file_type()
                .map_err(|error| SourceError::io(entry.path(), error))?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if depth < ctx.config().classifier.max_depth as usize {
                    stack.push((entry.path(), depth + 1));
                } else {
                    ctx.warn(format!(
                        "{}: Start Menu depth budget reached",
                        entry.path().display()
                    ));
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(ext) = path
                .extension()
                .and_then(|v| v.to_str())
                .map(str::to_ascii_lowercase)
            else {
                continue;
            };
            if !matches!(ext.as_str(), "lnk" | "url") {
                continue;
            }
            match shortcut::read(env, ctx, &path, &ext, &mut bytes) {
                Ok(Some(game)) => output.push(game),
                Ok(None) => (),
                Err(error) => ctx.warn(format!("{}: {error}", path.display())),
            }
            if bytes > 16 * 1024 * 1024 {
                return Err(SourceError::Other {
                    message: "Start Menu metadata byte budget exceeded".into(),
                });
            }
        }
    }
    output.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(output)
}
