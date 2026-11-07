//! Flatpak's installed metadata, not `flatpak list` or a running portal.
//! Format references only (original code):
//! https://github.com/flatpak/flatpak/wiki/Filesystem
//! https://docs.flatpak.org/en/latest/conventions.html
mod metainfo;
use super::desktop::{self, DesktopApplication, record::DesktopFile};
use crate::{
    DetectedGame, Evidence, HostEnv, LaunchTarget, LauncherStatus, OsSet, ScanCtx, Source,
    SourceError, SourceId, WatchPath,
    env::KnownFolder,
    parsers::{Limits, desktop as parser},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
pub const ID: SourceId = SourceId::new("flatpak");
#[derive(Debug, Default)]
pub struct Flatpak;

pub(crate) fn valid_id(id: &str) -> bool {
    id.len() <= 255
        && id.split('.').count() >= 3
        && id.split('.').all(|part| {
            !part.is_empty()
                && !part.as_bytes()[0].is_ascii_digit()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        })
}
pub(crate) fn installation_paths(env: &HostEnv) -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Some(user) = env
        .var("FLATPAK_USER_DIR")
        .filter(|p| Path::new(p).has_root())
        .and_then(|p| env.resolve_path(Path::new(&p)).ok())
    {
        roots.push(user)
    } else if let Some(data) = env.known_folder(KnownFolder::DataHome) {
        roots.push(data.join("flatpak"));
    }
    let system = env
        .var("FLATPAK_SYSTEM_DIR")
        .filter(|p| Path::new(p).has_root())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/flatpak"));
    if let Ok(path) = env.resolve_path(&system) {
        roots.push(path)
    }
    if let Ok(config) = env.resolve_path(Path::new("/etc/flatpak/installations.d"))
        && let Ok(entries) = fs::read_dir(config)
    {
        let mut files: Vec<_> = entries
            .take(129)
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|s| s == "conf"))
            .collect();
        files.sort();
        for path in files.into_iter().take(128) {
            if let Ok(bytes) = super::read(env, &path, 1 << 20)
                && let Ok(text) = std::str::from_utf8(&bytes)
            {
                let mut installation = false;
                for line in text.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        installation = line.starts_with("[Installation ") && line.ends_with(']');
                    } else if installation
                        && let Some(path) = line
                            .strip_prefix("Path=")
                            .filter(|p| Path::new(p).has_root())
                        && let Ok(path) = env.resolve_path(Path::new(path))
                    {
                        roots.push(path)
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    roots
        .into_iter()
        .filter(|root| seen.insert(root.clone()))
        .collect()
}
impl Source for Flatpak {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        // An install or removal adds or drops `app/<id>`; its launcher lands
        // in the exports below. Never the whole installation: its apps and
        // runtimes hold tens of thousands of folders, a watch each.
        let mut paths: Vec<_> = installation_paths(env)
            .into_iter()
            .map(|root| WatchPath {
                path: root.join("app"),
                recursive: false,
            })
            .collect();
        paths.extend(
            desktop::application_dirs(env)
                .into_iter()
                .map(|path| WatchPath {
                    path,
                    recursive: true,
                }),
        );
        if let Ok(path) = env.resolve_path(Path::new("/etc/flatpak/installations.d")) {
            paths.push(WatchPath {
                path,
                recursive: true,
            })
        }
        paths
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let files = desktop::records(env, ctx)?;
        let mut games = BTreeMap::new();
        for root in installation_paths(env) {
            ctx.check()?;
            let apps = root.join("app");
            let entries = match env.resolve_path(&apps).and_then(fs::read_dir) {
                Ok(iter) => iter,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    ctx.warn(format!("{}: {e}", apps.display()));
                    continue;
                }
            };
            let mut apps = entries
                .take(4097)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| SourceError::io(&apps, e))?;
            if apps.len() > 4096 {
                return Err(SourceError::Other {
                    message: "Flatpak app count exceeds 4096".into(),
                });
            }
            apps.sort_by_key(|e| e.file_name());
            for app in apps {
                ctx.check()?;
                let Some(id) = app
                    .file_name()
                    .to_str()
                    .filter(|id| valid_id(id))
                    .map(str::to_owned)
                else {
                    continue;
                };
                if games.contains_key(&id) {
                    continue;
                }
                if crate::data::Data::builtin()
                    .map_err(|e| SourceError::Other {
                        message: e.to_string(),
                    })?
                    .launchers
                    .launchers
                    .iter()
                    .any(|launcher| launcher.flatpak_ids.contains(&id))
                {
                    continue;
                }
                let active = match env.resolve_path(&app.path().join("current/active")) {
                    Ok(path) if path.is_dir() => path,
                    _ => continue,
                };
                let metadata = active.join("metadata");
                let bytes = match super::read(env, &metadata, 1 << 20) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        ctx.warn(format!("{}: {e}", metadata.display()));
                        continue;
                    }
                };
                if application_name(&bytes) != Some(id.as_str()) {
                    ctx.warn(format!(
                        "{}: missing or mismatched Application/name",
                        metadata.display()
                    ));
                    continue;
                }
                let exported = files
                    .iter()
                    .find(|file| file.id == format!("{id}.desktop"))
                    .or_else(|| {
                        files
                            .iter()
                            .find(|file| file.entry.get("X-Flatpak") == Some(&id))
                    });
                let fallback = if exported.is_none() {
                    installed_desktop(env, &active, &id, ctx)
                } else {
                    None
                };
                let file = exported.or(fallback.as_ref());
                let info = metainfo(env, &active, &id, ctx);
                let mut candidate = if let Some(file) = file {
                    // Respect user masking, NoDisplay and TryExec even when
                    // the active deployment's metainfo still says Game.
                    let Some(app) = desktop::application(env, file, ctx) else {
                        continue;
                    };
                    if desktop::proxy(&app) {
                        continue;
                    }
                    app
                } else {
                    DesktopApplication {
                        desktop_id: id.clone(),
                        desktop_file: metadata.clone(),
                        title: info
                            .as_ref()
                            .and_then(|i| i.title.clone())
                            .unwrap_or_else(|| id.clone()),
                        categories: vec![],
                        launch: LaunchTarget::Flatpak {
                            app_id: id.clone(),
                            args: vec![],
                        },
                        install_dir: None,
                        icon: None,
                        flatpak_id: Some(id.clone()),
                        snap_instance: None,
                    }
                };
                if let Some(info) = &info {
                    if !info.id.is_empty() && info.id != id && info.id != format!("{id}.desktop") {
                        ctx.warn(format!("{}: metainfo app id mismatch", active.display()));
                        continue;
                    }
                    candidate.categories.extend(info.categories.iter().cloned());
                }
                candidate.desktop_id = id.clone();
                candidate.install_dir = Some(active.join("files"));
                candidate.flatpak_id = Some(id.clone());
                if !matches!(candidate.launch, LaunchTarget::Flatpak { .. }) {
                    candidate.launch = LaunchTarget::Flatpak {
                        app_id: id.clone(),
                        args: vec![],
                    };
                }
                if candidate.icon.is_none() {
                    candidate.icon = desktop::icon(
                        env,
                        &id,
                        &[active.join("files/share"), active.join("export/share")],
                    );
                }
                if let Some(mut game) = desktop::game(env, ctx, candidate, ID)? {
                    game.evidence.push(Evidence::File { path: metadata });
                    games.insert(id, game);
                }
            }
        }
        Ok(games.into_values().collect())
    }
}
pub(crate) fn application_name(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut active = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            active = line == "[Application]";
        } else if active
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            return Some(value.trim());
        }
    }
    None
}
fn installed_desktop(env: &HostEnv, active: &Path, id: &str, ctx: &ScanCtx) -> Option<DesktopFile> {
    for dir in ["export/share/applications", "files/share/applications"] {
        let path = active.join(dir).join(format!("{id}.desktop"));
        match super::read(env, &path, 1 << 20) {
            Ok(bytes) => match parser::parse(&bytes, &Limits::default()) {
                Ok(entry) => {
                    return Some(DesktopFile {
                        id: format!("{id}.desktop"),
                        path,
                        entry,
                    });
                }
                Err(e) => ctx.warn(format!("{}: {e}", path.display())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => ctx.warn(format!("{}: {e}", path.display())),
        }
    }
    None
}
fn metainfo(env: &HostEnv, active: &Path, id: &str, ctx: &ScanCtx) -> Option<metainfo::MetaInfo> {
    for (directory, suffix) in [("metainfo", "metainfo.xml"), ("appdata", "appdata.xml")] {
        let path = active.join(format!("files/share/{directory}/{id}.{suffix}"));
        match super::read(env, &path, 1 << 20) {
            Ok(bytes) => {
                return match metainfo::parse(&bytes, &desktop::record::locale(env)) {
                    Ok(info) => Some(info),
                    Err(e) => {
                        ctx.warn(format!("{}: {e}", path.display()));
                        None
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => ctx.warn(format!("{}: {e}", path.display())),
        }
    }
    None
}
