use super::record::DesktopFile;
use crate::{
    HostEnv, ScanCtx, SourceError,
    env::KnownFolder,
    parsers::{Limits, desktop},
};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

pub(super) const MAX_RECORDS: usize = 20_000;
pub(crate) fn application_dirs(env: &HostEnv) -> Vec<PathBuf> {
    let mut paths = vec![];
    if let Some(data) = env.known_folder(KnownFolder::DataHome) {
        paths.push(data.join("applications"));
    }
    paths.extend(env.xdg_data_dirs().iter().map(|p| p.join("applications")));
    paths.extend(
        super::super::flatpak::installation_paths(env)
            .into_iter()
            .map(|p| p.join("exports/share/applications")),
    );
    if let Ok(path) = env.resolve_path(Path::new("/var/lib/snapd/desktop/applications")) {
        paths.push(path)
    }
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

pub(crate) fn records(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DesktopFile>, SourceError> {
    let mut result = vec![];
    let mut seen = BTreeSet::new();
    let mut visited = 0usize;
    for root in application_dirs(env) {
        ctx.check()?;
        let root = match env.resolve_path(&root) {
            Ok(path) => path,
            Err(error) => {
                ctx.warn(format!("{}: {error}", root.display()));
                continue;
            }
        };
        let mut stack = vec![(root.clone(), 0)];
        while let Some((dir, depth)) = stack.pop() {
            ctx.check()?;
            let iter = match fs::read_dir(&dir) {
                Ok(iter) => iter,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    ctx.warn(format!("{}: {error}", dir.display()));
                    continue;
                }
            };
            let remaining = MAX_RECORDS.saturating_sub(visited);
            let mut entries = iter
                .take(remaining + 1)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| SourceError::io(&dir, e))?;
            if entries.len() > remaining {
                return Err(SourceError::Other {
                    message: "desktop directory entry limit exceeded".into(),
                });
            }
            visited += entries.len();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                ctx.check()?;
                let path = entry.path();
                let kind = entry.file_type().map_err(|e| SourceError::io(&path, e))?;
                if kind.is_dir() {
                    if depth < 8 {
                        stack.push((path, depth + 1))
                    } else {
                        ctx.warn("desktop directory depth limit reached")
                    }
                    continue;
                }
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                let Some(id) = path
                    .strip_prefix(&root)
                    .ok()
                    .and_then(Path::to_str)
                    .map(|s| s.replace(['/', '\\'], "-"))
                else {
                    ctx.warn("skipped non-Unicode desktop file id");
                    continue;
                };
                // Mask before parsing/visibility checks: Hidden=true in a
                // higher priority directory also hides the system record.
                if !seen.insert(id.clone()) {
                    continue;
                }
                let bytes = match crate::sources::read(env, &path, 1 << 20) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        ctx.warn(format!("{}: {error}", path.display()));
                        continue;
                    }
                };
                match desktop::parse(&bytes, &Limits::default()) {
                    Ok(entry) => result.push(DesktopFile { id, path, entry }),
                    Err(error) => ctx.warn(format!("{}: {error}", path.display())),
                }
            }
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

pub(crate) fn executable(env: &HostEnv, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\0') {
        return None;
    }
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return check_executable(env, Path::new(name));
    }
    if name.contains('/') {
        return None;
    }
    env.var("PATH")?
        .to_str()?
        .split(':')
        .filter(|p| p.starts_with('/') || Path::new(p).is_absolute())
        .find_map(|p| check_executable(env, &Path::new(p).join(name)))
}
fn check_executable(env: &HostEnv, path: &Path) -> Option<PathBuf> {
    let path = env.resolve_path(path).ok()?;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path)
}

pub(crate) fn icon(env: &HostEnv, name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\0') {
        return None;
    }
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return env
            .resolve_path(Path::new(name))
            .ok()
            .filter(|p| p.is_file());
    }
    if name.contains(['/', '\\']) {
        return None;
    }
    let mut roots = extra.to_vec();
    roots.extend(
        env.known_folder(KnownFolder::DataHome)
            .map(Path::to_path_buf),
    );
    roots.extend(env.xdg_data_dirs().iter().cloned());
    for root in roots {
        for size in ["256x256", "128x128", "64x64", "48x48", "scalable"] {
            for suffix in ["png", "svg", "xpm"] {
                let path = root.join(format!("icons/hicolor/{size}/apps/{name}.{suffix}"));
                if let Ok(path) = env.resolve_path(&path)
                    && path.is_file()
                {
                    return Some(path);
                }
            }
        }
        for suffix in ["png", "svg", "xpm"] {
            let path = root.join(format!("pixmaps/{name}.{suffix}"));
            if let Ok(path) = env.resolve_path(&path)
                && path.is_file()
            {
                return Some(path);
            }
        }
    }
    None
}
