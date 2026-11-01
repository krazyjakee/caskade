//! Xbox app / Store GDK games, discovered from drive markers and game configs.
//! All filesystem access is read-only. Store apps without MicrosoftGame.config
//! are excluded. Package code, PowerShell, and Windows activation are never run.
mod library;
mod metadata;

use crate::{HostEnv, types::*};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("xbox");
#[derive(Debug, Default)]
pub struct Xbox;

impl Source for Xbox {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }
    fn detect(&self, _env: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let volumes = env
            .volumes()
            .map_err(|e| SourceError::io(env.fs_root(), e))?;
        if volumes.len() > 256 {
            return Err(SourceError::parse(env.fs_root(), "volume limit"));
        }
        let mut libraries = BTreeSet::new();
        for volume in volumes {
            ctx.check()?;
            match library::roots(env, &volume.root) {
                Ok(roots) => libraries.extend(roots),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => ctx.warn(format!(
                    "{}: {e}",
                    volume.root.join(".GamingRoot").display()
                )),
            }
        }
        let mut games = Vec::new();
        let mut seen = BTreeSet::new();
        let mut count = 0;
        for root in libraries {
            ctx.check()?;
            let entries = match fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(e) => {
                    ctx.warn(format!("{}: {e}", root.display()));
                    continue;
                }
            };
            let mut paths = Vec::new();
            for entry in entries {
                ctx.check()?;
                count += 1;
                if count > 4096 {
                    return Err(SourceError::parse(&root, "game directory limit"));
                }
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        ctx.warn(e.to_string());
                        continue;
                    }
                };
                // Never recurse into a directory link controlled by a library.
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    paths.push(entry.path());
                }
            }
            paths.sort();
            for path in paths {
                ctx.check()?;
                match record(env, &path) {
                    Ok(Some(game)) if seen.insert(game.key.to_ascii_lowercase()) => {
                        games.push(game)
                    }
                    Ok(_) => (),
                    Err(e) => ctx.warn(format!("{}: {e}", path.display())),
                }
                ctx.check()?;
            }
        }
        Ok(games)
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let mut paths = Vec::new();
        if let Ok(volumes) = env.volumes() {
            for volume in volumes.into_iter().take(256) {
                if let Ok(root) = env.resolve_path(&volume.root) {
                    paths.push(WatchPath {
                        path: root.join(".GamingRoot"),
                        recursive: false,
                    });
                    if let Ok(roots) = library::roots(env, &root) {
                        paths.extend(roots.into_iter().map(|path| WatchPath {
                            path,
                            recursive: true,
                        }));
                    }
                }
            }
        }
        paths.sort_by(|a, b| a.path.cmp(&b.path));
        paths.dedup_by(|a, b| a.path == b.path);
        paths
    }
    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        // Markers themselves are the pickup locations, including absent ones.
        env.volumes()
            .unwrap_or_default()
            .into_iter()
            .take(256)
            .filter_map(|v| env.resolve_path(&v.root).ok())
            .map(|p| p.join(".GamingRoot"))
            .collect()
    }
}
fn record(env: &HostEnv, game_dir: &Path) -> io::Result<Option<DetectedGame>> {
    let content = library::child(env, game_dir, "Content")?;
    // An absent Content directory is an ordinary non-game library entry.
    if !content.is_dir() {
        return Ok(None);
    }
    let config_path = library::child(env, &content, "MicrosoftGame.config")?;
    let bytes = match crate::sources::read(env, &config_path, 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let config = metadata::game(&bytes)?;
    let family = config.identity.family()?;
    let Some(exe) = config.executables.iter().find(|e| e.pc && !e.dev) else {
        return Ok(None);
    };
    if !valid_app_id(&exe.id) {
        return Err(metadata::invalid(
            "missing/invalid executable application ID",
        ));
    }
    let executable = library::child(env, &content, &exe.path)?;
    let appx_path = library::child(env, &content, "AppxManifest.xml")?;
    let appx = match crate::sources::read(env, &appx_path, 1024 * 1024) {
        Ok(bytes) => Some(metadata::appx(&bytes)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(appx) = &appx {
        // Publisher is case sensitive in the Windows identity contract.
        if !config
            .identity
            .name
            .eq_ignore_ascii_case(&appx.identity.name)
            || config.identity.publisher != appx.identity.publisher
        {
            return Err(metadata::invalid("game/AppX identity mismatch"));
        }
        let app = appx
            .applications
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(&exe.id))
            .ok_or_else(|| metadata::invalid("game executable is absent from AppX applications"))?;
        if !app.executable.is_empty() {
            // AppX may register a launch helper rather than the game's EXE.
            // Validate its path, but retain the config's real EXE for tracking.
            library::child(env, &content, &app.executable)?;
        }
    }
    let title = if literal_title(&config.title) {
        &config.title
    } else {
        appx.as_ref()
            .and_then(|a| {
                a.applications
                    .iter()
                    .find(|a| a.id.eq_ignore_ascii_case(&exe.id))
            })
            .map(|a| &a.title)
            .filter(|t| literal_title(t))
            .ok_or_else(|| metadata::invalid("no literal game display name"))?
    };
    let mut game = DetectedGame::new(ID, format!("{family}!{}", exe.id), title.trim());
    game.ids.push(ExternalId::Xbox {
        pfn: family.clone(),
    });
    if !config.store_id.is_empty() {
        if config.store_id.len() > 64 || !config.store_id.bytes().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(metadata::invalid("invalid StoreId"));
        }
        game.ids.push(ExternalId::MicrosoftStore {
            store_id: config.store_id,
        });
    }
    game.install_dir = Some(content.clone());
    game.launch.push(LaunchTarget::Uri {
        uri: format!("shell:AppsFolder\\{family}!{}", exe.id),
    });
    game.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    game.confidence = Confidence::Certain;
    game.state = if executable.is_file() {
        InstallState::Installed
    } else {
        InstallState::Broken {
            reason: "declared game executable is missing".into(),
        }
    };
    game.evidence.push(Evidence::File {
        path: env.resolve_path(&config_path)?,
    });
    if appx.is_some() {
        game.evidence.push(Evidence::File {
            path: env.resolve_path(&appx_path)?,
        });
    }
    if !config.logo.is_empty() {
        let icon = library::child(env, &content, &config.logo)?;
        if icon.is_file() {
            game.icon = Some(icon);
        }
    }
    game.tracking = Some(TrackingHint {
        install_dir: Some(content),
        exe_names: executable
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .into_iter()
            .collect(),
        ..Default::default()
    });
    Ok(Some(game))
}
fn valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.')
}
fn literal_title(value: &str) -> bool {
    let text = value.trim();
    !text.is_empty()
        && !text.to_ascii_lowercase().starts_with("ms-resource:")
        && !text.chars().any(char::is_control)
}
