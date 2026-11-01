use super::*;
use crate::{
    Evidence, GameRuntime, LaunchTarget, TrackingHint,
    classify::{self, Metadata},
    parsers::{self, Limits},
    uri::{self, UriIdentity},
};
use std::{collections::BTreeMap, path::Path};
const MAX_BYTES: u64 = 1024 * 1024;
fn discarded(name: &str) -> bool {
    let text = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    text.starts_with("unins")
        || text.split(|c: char| !c.is_alphanumeric()).any(|part| {
            matches!(
                part,
                "uninstall"
                    | "uninstaller"
                    | "readme"
                    | "manual"
                    | "website"
                    | "support"
                    | "help"
                    | "documentation"
                    | "license"
                    | "eula"
                    | "changelog"
            )
        })
        || [".pdf", ".txt", ".html", ".htm", ".chm"]
            .iter()
            .any(|ext| text.ends_with(ext))
}
fn resolve(env: &HostEnv, text: &str, relative_to: &Path) -> Option<PathBuf> {
    let expanded = env.expand_vars(text.trim().trim_matches('"'));
    let text = expanded.to_str()?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    let drive = text.len() >= 3
        && text.as_bytes()[0].is_ascii_alphabetic()
        && text.as_bytes()[1] == b':'
        && matches!(text.as_bytes()[2], b'/' | b'\\');
    if text.as_bytes().get(1) == Some(&b':') && !drive {
        return None;
    }
    let path = if drive || text.starts_with(['/', '\\']) {
        PathBuf::from(text)
    } else {
        super::super::join(relative_to, text)
    };
    // Normalize relative .lnk paths before HostEnv's idempotent mapped-path check.
    let path = if path.starts_with(env.fs_root()) {
        let mut clean = PathBuf::new();
        for part in path.components() {
            match part {
                std::path::Component::ParentDir => {
                    if !clean.pop() {
                        return None;
                    }
                }
                std::path::Component::CurDir => (),
                part => clean.push(part.as_os_str()),
            }
        }
        clean
    } else {
        path
    };
    env.resolve_path(&path).ok()
}
pub(super) fn read(
    env: &HostEnv,
    ctx: &ScanCtx,
    path: &Path,
    extension: &str,
    total: &mut usize,
) -> Result<Option<ShortcutApplication>, SourceError> {
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if title.is_empty() || discarded(title) {
        return Ok(None);
    }
    let bytes =
        super::super::read(env, path, MAX_BYTES).map_err(|error| SourceError::io(path, error))?;
    *total = total.saturating_add(bytes.len());
    if *total > 16 * 1024 * 1024 {
        return Ok(None);
    }
    let limits = Limits {
        max_input_bytes: MAX_BYTES as usize,
        max_string_bytes: 65536,
        max_nodes: 4096,
        ..Default::default()
    };
    let parent = path.parent().unwrap_or(env.fs_root());
    let (target, args, cwd, icon) = if extension == "lnk" {
        let link = parsers::lnk::parse(&bytes, &limits)
            .map_err(|error| SourceError::parse(path, error))?;
        for warning in link.warnings {
            ctx.warn(format!("{}: {warning}", path.display()));
        }
        let link = link.value;
        let Some(target) = link
            .target
            .or(link.environment_target)
            .or(link.relative_path)
        else {
            ctx.warn(format!(
                "{}: unresolved Shell namespace target",
                path.display()
            ));
            return Ok(None);
        };
        (target, link.arguments, link.working_dir, link.icon)
    } else {
        let link = parsers::url::parse(&bytes, &limits)
            .map_err(|error| SourceError::parse(path, error))?;
        (link.url, vec![], link.working_dir, link.icon)
    };
    if discarded(&target)
        || target.to_ascii_lowercase().contains("://uninstall/")
        || args
            .iter()
            .any(|arg| arg.to_ascii_lowercase().contains("://uninstall/"))
    {
        return Ok(None);
    }
    let target = if target.contains("://") && !target.to_ascii_lowercase().starts_with("file://") {
        LaunchTarget::Uri { uri: target }
    } else {
        let target = if target.to_ascii_lowercase().starts_with("file://") {
            let url = url::Url::parse(&target).map_err(|error| SourceError::parse(path, error))?;
            if url
                .host_str()
                .is_some_and(|host| !host.is_empty() && host != "localhost")
            {
                return Ok(None);
            }
            let decoded = percent_encoding::percent_decode_str(url.path())
                .decode_utf8()
                .map_err(|error| SourceError::parse(path, error))?
                .into_owned();
            if decoded.as_bytes().get(2) == Some(&b':') {
                decoded[1..].to_owned()
            } else {
                decoded
            }
        } else {
            target
        };
        let Some(program) = resolve(env, &target, parent) else {
            return Ok(None);
        };
        if discarded(&program.to_string_lossy()) {
            return Ok(None);
        }
        LaunchTarget::Exec {
            program,
            args,
            cwd: cwd.as_deref().and_then(|cwd| resolve(env, cwd, parent)),
            env: BTreeMap::new(),
        }
    };
    let ids = identities(&target);
    if let LaunchTarget::Exec { program, .. } = &target {
        if ids.is_empty() && !program.is_file() {
            ctx.warn(format!("{}: shortcut target is missing", path.display()));
            return Ok(None);
        }
    } else if ids.is_empty() {
        return Ok(None);
    }
    let icon = icon
        .as_deref()
        .and_then(|text| resolve(env, text, parent))
        .filter(|p| {
            p.is_file()
                && p.extension().is_some_and(|ext| {
                    ["ico", "png", "jpg", "jpeg", "bmp"]
                        .iter()
                        .any(|v| ext.eq_ignore_ascii_case(v))
                })
        });
    let mut target = target;
    if let LaunchTarget::Exec { program, cwd, .. } = &mut target
        && cwd.is_none()
    {
        *cwd = program.parent().map(Path::to_path_buf);
    }
    Ok(Some(ShortcutApplication {
        path: path.into(),
        title: title.into(),
        launch: target,
        icon,
    }))
}
fn identities(target: &LaunchTarget) -> std::collections::BTreeSet<UriIdentity> {
    let mut ids = uri::target_identities(target);
    if let LaunchTarget::Exec { program, args, .. } = target
        && program
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("steam.exe"))
        && let Some(index) = args
            .iter()
            .position(|s| s.eq_ignore_ascii_case("-applaunch"))
        && let Some(app_id) = args
            .get(index + 1)
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0)
    {
        ids.insert(UriIdentity::External(crate::ExternalId::Steam { app_id }));
    }
    ids
}
pub(super) fn classify(
    env: &HostEnv,
    ctx: &ScanCtx,
    app: ShortcutApplication,
) -> Result<Option<DetectedGame>, SourceError> {
    let path = &app.path;
    let mut game = DetectedGame::new(ID, path.to_string_lossy(), &app.title);
    game.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    game.evidence.push(Evidence::File { path: path.clone() });
    let ids = identities(&app.launch);
    if !ids.is_empty() {
        game.ids = ids
            .into_iter()
            .filter_map(|id| match id {
                UriIdentity::External(id) => Some(id),
                _ => None,
            })
            .collect();
        game.launch.push(app.launch);
        uri::mark_proxy(&mut game);
        return Ok(Some(game));
    }
    let LaunchTarget::Exec { program, .. } = &app.launch else {
        return Ok(None);
    };
    let Some(dir) = program.parent() else {
        return Ok(None);
    };
    let metadata = Metadata {
        title: Some(app.title),
        install_dir: Some(dir.into()),
        executable: Some(program.clone()),
        ..Default::default()
    };
    let result = classify::classify_path(
        env,
        &super::super::uninstall::host_profile(env),
        dir,
        &metadata,
        &ctx.config().classifier,
    )
    .map_err(|error| SourceError::parse(path, error))?;
    let Some(confidence) = result.confidence() else {
        return Ok(None);
    };
    game.confidence = confidence;
    game.install_dir = Some(dir.into());
    game.evidence.extend(result.evidence);
    for warning in result.warnings {
        ctx.warn(format!("{}: {warning}", path.display()));
    }
    game.icon = app.icon;
    game.tracking = Some(TrackingHint {
        install_dir: game.install_dir.clone(),
        exe_names: program
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
            .into_iter()
            .collect(),
        ..Default::default()
    });
    game.launch.push(app.launch);
    Ok(Some(game))
}
