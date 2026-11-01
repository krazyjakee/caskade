use super::path;
use crate::{
    Evidence, HostEnv, LaunchTarget, ScanCtx,
    env::KnownFolder,
    parsers::{Limits, lnk},
};
use std::{collections::BTreeMap, fs, path::Path};
/// Only the bounded PlayTasks tree is inspected. SupportTasks are never launch
/// candidates, and a GDF DLL is never substituted for a game executable.
pub(super) fn primary(
    env: &HostEnv,
    ctx: &ScanCtx,
    guid: &str,
    install: Option<&Path>,
    evidence: &mut Vec<Evidence>,
) -> Option<LaunchTarget> {
    for folder in [KnownFolder::ProgramData, KnownFolder::LocalAppData]
        .into_iter()
        .filter_map(|f| env.known_folder(f))
    {
        let root = folder
            .join("Microsoft/Windows/GameExplorer")
            .join(guid)
            .join("PlayTasks");
        let mut stack = vec![(root, 0)];
        let mut visited = 0usize;
        while let Some((dir, depth)) = stack.pop() {
            if ctx.check().is_err() {
                return None;
            }
            let dir = match env.resolve_path(&dir) {
                Ok(path) => path,
                Err(error) => {
                    ctx.warn(error.to_string());
                    continue;
                }
            };
            let iter = match fs::read_dir(&dir) {
                Ok(iter) => iter,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    ctx.warn(format!("{}: {error}", dir.display()));
                    continue;
                }
            };
            let mut entries = iter
                .take(257usize.saturating_sub(visited))
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            visited += entries.len();
            if visited > 256 {
                ctx.warn("GameUX play-task entry limit reached");
                return None;
            }
            entries.sort_by_key(|e| e.file_name());
            // Stack in reverse, so primary task 0 precedes task 1.
            let mut directories = vec![];
            for entry in entries {
                if ctx.check().is_err() {
                    return None;
                }
                let file = entry.path();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    if depth < 2 {
                        directories.push((file, depth + 1))
                    }
                    continue;
                }
                if file
                    .extension()
                    .is_none_or(|s| !s.eq_ignore_ascii_case("lnk"))
                {
                    continue;
                }
                let Ok(file) = env.resolve_path(&file) else {
                    continue;
                };
                if !fs::metadata(&file).is_ok_and(|m| m.is_file() && m.len() <= 1024 * 1024) {
                    continue;
                }
                let bytes = match super::super::read(env, &file, 1024 * 1024) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        ctx.warn(format!("{}: {error}", file.display()));
                        continue;
                    }
                };
                let link = match lnk::parse(&bytes, &Limits::default()) {
                    Ok(link) => link,
                    Err(error) => {
                        ctx.warn(format!("{}: {error}", file.display()));
                        continue;
                    }
                };
                for warning in link.warnings {
                    ctx.warn(warning)
                }
                let link = link.value;
                let target = link
                    .target
                    .or(link.environment_target)
                    .or(link.relative_path);
                let Some(program) = target
                    .and_then(|text| path(env, &text, true, install))
                    .filter(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("exe")))
                else {
                    continue;
                };
                let cwd = link
                    .working_dir
                    .as_deref()
                    .and_then(|text| path(env, text, true, install))
                    .or_else(|| install.map(Path::to_path_buf));
                evidence.push(Evidence::File { path: file });
                return Some(LaunchTarget::Exec {
                    program,
                    args: link.arguments,
                    cwd,
                    env: BTreeMap::new(),
                });
            }
            stack.extend(directories.into_iter().rev());
        }
    }
    None
}
