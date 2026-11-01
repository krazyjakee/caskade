//! Captured PATH/PATHEXT lookup, independent of the OS running a fixture.
use super::*;
use crate::OperatingSystem;
use std::collections::BTreeSet;
/// First executable per requested leaf name, preserving request and PATH order.
/// Relative/empty PATH entries are ignored; no implicit current-directory lookup.
pub fn executables(
    env: &HostEnv,
    ctx: &ScanCtx,
    names: &[&str],
) -> Result<Vec<PathBuf>, SourceError> {
    ctx.check()?;
    if names.len() > 256 {
        return Err(SourceError::Other {
            message: "PATH probe name budget exceeded".into(),
        });
    }
    let windows = env.os() == OperatingSystem::Windows;
    let raw = env
        .var("PATH")
        .and_then(|s| s.into_string().ok())
        .unwrap_or_default();
    if raw.len() > 65536 {
        return Err(SourceError::Other {
            message: "PATH probe byte budget exceeded".into(),
        });
    }
    let paths: Vec<_> = raw
        .split(if windows { ';' } else { ':' })
        .filter(|s| !s.is_empty())
        .take(1025)
        .collect();
    if paths.len() > 1024 {
        return Err(SourceError::Other {
            message: "PATH probe directory budget exceeded".into(),
        });
    }
    let pathext = env
        .var("PATHEXT")
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    if pathext.len() > 4096 {
        return Err(SourceError::Other {
            message: "PATHEXT byte budget exceeded".into(),
        });
    }
    let extensions: Vec<_> = pathext
        .split(';')
        .filter(|ext| {
            ext.starts_with('.')
                && ext.len() <= 16
                && ext[1..].bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .take(33)
        .collect();
    if extensions.len() > 32 {
        return Err(SourceError::Other {
            message: "PATHEXT entry budget exceeded".into(),
        });
    }
    let mut output = vec![];
    let mut seen = BTreeSet::new();
    for name in names {
        ctx.check()?;
        if !safe_text(name) || matches!(*name, "." | "..") || name.contains(['/', '\\', ':']) {
            return Err(SourceError::Other {
                message: "PATH probe requires executable leaf names".into(),
            });
        }
        let mut variants = vec![name.to_string()];
        if windows && Path::new(name).extension().is_none() {
            variants.extend(extensions.iter().map(|ext| format!("{name}{ext}")));
        }
        'directories: for directory in &paths {
            ctx.check()?;
            let absolute = if windows {
                directory.len() >= 3
                    && directory.as_bytes()[0].is_ascii_alphabetic()
                    && directory.as_bytes()[1] == b':'
                    && matches!(directory.as_bytes()[2], b'/' | b'\\')
                    || directory.starts_with(r"\\")
            } else {
                directory.starts_with('/')
            };
            if !absolute {
                continue;
            }
            for variant in &variants {
                let joined = if windows {
                    PathBuf::from(format!(
                        "{}\\{variant}",
                        directory.trim_end_matches(['/', '\\'])
                    ))
                } else {
                    Path::new(directory).join(variant)
                };
                let Ok(path) = env.resolve_path(&joined) else {
                    continue;
                };
                if !regular_executable(env, &path) {
                    continue;
                }
                if seen.insert(path.clone()) {
                    output.push(path);
                }
                break 'directories;
            }
        }
    }
    Ok(output)
}
pub(crate) fn regular_executable(env: &HostEnv, path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if env.os() != OperatingSystem::Windows && metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    #[cfg(not(unix))]
    let _ = env;
    true
}
