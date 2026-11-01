//! Original read-only Minigalaxy source (#93). Upstream is a format reference
//! only: https://github.com/sharkwouter/minigalaxy. No code is ported.
use super::{canonical, is_file, json, relative, tokens};
use crate::{HostEnv, env::KnownFolder, types::*};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
pub const ID: SourceId = SourceId::new("minigalaxy");
pub const FLATPAK_ID: &str = "io.github.sharkwouter.Minigalaxy";
#[derive(Default, Debug)]
pub struct Minigalaxy;
fn configs(env: &HostEnv) -> Vec<(PathBuf, bool)> {
    let mut paths = vec![];
    if let Some(config) = env.known_folder(KnownFolder::ConfigHome) {
        paths.push((config.join("minigalaxy/config.json"), false));
    }
    paths.push((
        env.home().join(format!(
            ".var/app/{FLATPAK_ID}/config/minigalaxy/config.json"
        )),
        true,
    ));
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|(p, _)| is_file(env, p))
        .filter(|(p, _)| seen.insert(canonical(env, p)))
        .collect()
}
impl Source for Minigalaxy {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let statuses: Vec<_> = configs(env)
            .into_iter()
            .map(|(file, flatpak)| {
                let kind = LauncherKind::native_or_flatpak(flatpak, FLATPAK_ID);
                LauncherStatus::found(ID, kind, None, file.parent().map(Path::to_owned))
            })
            .collect();
        if statuses.is_empty() {
            vec![LauncherStatus::not_found(ID)]
        } else {
            statuses
        }
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let mut games = vec![];
        let mut seen = BTreeSet::new();
        for (file, _) in configs(env) {
            ctx.check()?;
            let value = match json(env, &file, 1 << 20) {
                Ok(value) => value,
                Err(e) => {
                    ctx.warn(e.to_string());
                    continue;
                }
            };
            let root = match value.get("install_dir").and_then(Value::as_str) {
                Some(path) => env.expand_path(path).ok(),
                None => Some(env.home().join("GOG Games")),
            };
            let Some(root) = root else {
                ctx.warn(format!("{}: invalid install_dir", file.display()));
                continue;
            };
            let entries = match fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(e) => {
                    ctx.warn(format!("{}: {e}", root.display()));
                    continue;
                }
            };
            for (count, entry) in entries.take(4097).enumerate() {
                ctx.check()?;
                if count == 4096 {
                    ctx.warn("Minigalaxy install directory entry budget exceeded");
                    break;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                let Ok(dir) = env.resolve_path(&entry.path()) else {
                    continue;
                };
                if !dir.is_dir() {
                    continue;
                }
                let key = canonical(env, &dir);
                if !seen.insert(key) {
                    continue;
                }
                games.extend(folder(env, ctx, &dir, &file));
            }
        }
        games.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(games)
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let mut paths = vec![];
        for (file, _) in configs(env) {
            if let Ok(value) = json(env, &file, 1 << 20) {
                let root = value
                    .get("install_dir")
                    .and_then(Value::as_str)
                    .and_then(|p| env.expand_path(p).ok())
                    .unwrap_or_else(|| env.home().join("GOG Games"));
                paths.push(WatchPath {
                    path: root,
                    recursive: true,
                });
            }
            paths.push(WatchPath {
                path: file,
                recursive: false,
            });
        }
        paths
    }
}
fn folder(env: &HostEnv, ctx: &ScanCtx, dir: &Path, config: &Path) -> Vec<DetectedGame> {
    // Current Windows Minigalaxy prefixes keep c:\game beneath drive_c.
    let candidates = [
        dir.to_owned(),
        dir.join("drive_c/game"),
        dir.join("wineprefix/drive_c/game"),
    ];
    for data in candidates {
        let Ok(data) = env.resolve_path(&data) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(&data) else {
            continue;
        };
        let mut files: Vec<_> = entries
            .take(257)
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        if files.len() > 256 {
            ctx.warn("Minigalaxy metadata directory entry budget exceeded");
            continue;
        }
        let mut games = vec![];
        for file in files {
            let Some(name) = file.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(id) = name
                .strip_prefix("goggame-")
                .and_then(|s| s.strip_suffix(".info"))
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|id| *id > 0)
            else {
                continue;
            };
            let value = match json(env, &file, 2 << 20) {
                Ok(value) => value,
                Err(e) => {
                    ctx.warn(e.to_string());
                    continue;
                }
            };
            if number(value.get("gameId")).is_some_and(|actual| actual != id) {
                ctx.warn(format!("{}: conflicting GOG id", file.display()));
                continue;
            }
            let title = value
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    dir.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
            let mut game = DetectedGame::new(ID, id.to_string(), title);
            game.ids.push(ExternalId::Gog { product_id: id });
            game.install_dir = Some(data.clone());
            game.evidence = vec![
                Evidence::File {
                    path: config.to_owned(),
                },
                Evidence::File { path: file },
            ];
            let task = value
                .get("playTasks")
                .and_then(Value::as_array)
                .and_then(|tasks| {
                    tasks
                        .iter()
                        .filter(|t| t.get("type").and_then(Value::as_str) == Some("FileTask"))
                        .max_by_key(|t| {
                            t.get("isPrimary").and_then(Value::as_bool).unwrap_or(false)
                        })
                });
            let program = task
                .and_then(|t| t.get("path"))
                .and_then(Value::as_str)
                .and_then(|p| relative(env, &data, p))
                .or_else(|| is_file(env, &data.join("start.sh")).then(|| data.join("start.sh")));
            let args = task
                .and_then(|t| t.get("arguments"))
                .and_then(Value::as_str)
                .map(tokens)
                .unwrap_or_default();
            let cwd = task
                .and_then(|t| t.get("workingDir"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .and_then(|p| relative(env, &data, p))
                .unwrap_or_else(|| data.clone());
            if let Some(program) = program {
                if program
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
                {
                    let prefix = if data == dir {
                        None
                    } else if data == dir.join("drive_c/game") {
                        Some(dir.to_owned())
                    } else {
                        Some(dir.join("wineprefix"))
                    };
                    game.runtime = GameRuntime::Windows {
                        prefix: prefix.map(|path| Prefix {
                            path,
                            arch: None,
                            owner: Some(ID),
                        }),
                        runner: Some(RunnerHint {
                            kind: RunnerKind::Wine,
                            name: None,
                            path: None,
                        }),
                    };
                }
                game.tracking = Some(TrackingHint {
                    install_dir: game.install_dir.clone(),
                    exe_names: program
                        .file_name()
                        .map(|s| vec![s.to_string_lossy().into_owned()])
                        .unwrap_or_default(),
                    steam_app_id: None,
                });
                game.launch.push(LaunchTarget::Exec {
                    program,
                    args,
                    cwd: Some(cwd),
                    env: BTreeMap::new(),
                });
            }
            games.push(game);
        }
        if !games.is_empty() {
            return games;
        }
    }
    // Legacy info files hold settings/version rather than a trustworthy GOG id.
    // Keep the local game identity without inventing a store identifier.
    let file = dir.join("minigalaxy-info.json");
    if is_file(env, &file) && is_file(env, &dir.join("start.sh")) {
        match json(env, &file, 1 << 20) {
            Ok(_) => {
                let title = dir.file_name().unwrap_or_default().to_string_lossy();
                let mut game = DetectedGame::new(ID, format!("legacy:{title}"), title.as_ref());
                game.install_dir = Some(dir.to_owned());
                game.launch.push(LaunchTarget::Exec {
                    program: dir.join("start.sh"),
                    args: vec![],
                    cwd: Some(dir.to_owned()),
                    env: BTreeMap::new(),
                });
                game.evidence.push(Evidence::File { path: file });
                return vec![game];
            }
            Err(e) => ctx.warn(e.to_string()),
        }
    }
    vec![]
}
fn number(value: Option<&Value>) -> Option<u64> {
    let v = value?;
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}
