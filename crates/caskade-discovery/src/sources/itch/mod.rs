//! itch app/butler database and receipt discovery (#94), original reader.
//! Format references: itchio/butler database/models/{cave,install_location}.go,
//! itchio/hush bfs/receipt.go and itch.io's public manifest-actions specification.
#[cfg(feature = "sqlite")]
mod db;
#[cfg(feature = "sqlite")]
mod metadata;
use super::{canonical, is_file};
use crate::{HostEnv, OperatingSystem, env::KnownFolder, types::*};
#[cfg(feature = "sqlite")]
use std::{collections::BTreeMap, path::Path};
use std::{collections::BTreeSet, path::PathBuf};
pub const ID: SourceId = SourceId::new("itch");
#[derive(Default, Debug)]
pub struct Itch;
fn databases(env: &HostEnv) -> Vec<PathBuf> {
    let mut candidates = vec![];
    for folder in [KnownFolder::AppData, KnownFolder::ConfigHome] {
        if let Some(root) = env.known_folder(folder) {
            candidates.push(root.join("itch/butler.db"));
        }
    }
    if env.os() == OperatingSystem::Macos {
        candidates.push(
            env.home()
                .join("Library/Application Support/itch/butler.db"),
        );
    }
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|p| is_file(env, p))
        .filter(|p| seen.insert(canonical(env, p)))
        .collect()
}
impl Source for Itch {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::ALL
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let statuses: Vec<_> = databases(env)
            .into_iter()
            .map(|p| {
                LauncherStatus::found(
                    ID,
                    LauncherKind::Native,
                    None,
                    p.parent().map(|p| p.to_owned()),
                )
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
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = env;
            ctx.warn("itch database discovery requires the sqlite feature");
            Ok(vec![])
        }
        #[cfg(feature = "sqlite")]
        {
            let mut games = vec![];
            for path in databases(env) {
                ctx.check()?;
                let path = env
                    .resolve_path(&path)
                    .map_err(|e| SourceError::io(&path, e))?;
                let tables = match db::read(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        ctx.warn(e.to_string());
                        continue;
                    }
                };
                for cave in &tables.caves {
                    ctx.check()?;
                    if let Some(game) = game(env, ctx, &path, cave, &tables) {
                        games.push(game);
                    }
                }
            }
            games.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(games)
        }
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        databases(env)
            .into_iter()
            .flat_map(|p| {
                [
                    WatchPath {
                        path: p.with_file_name("butler.db-wal"),
                        recursive: false,
                    },
                    WatchPath {
                        path: p,
                        recursive: false,
                    },
                ]
            })
            .collect()
    }
}
#[cfg(feature = "sqlite")]
fn game(
    env: &HostEnv,
    ctx: &ScanCtx,
    file: &Path,
    cave: &db::Record,
    tables: &db::Tables,
) -> Option<DetectedGame> {
    let id = cave.text("id")?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        ctx.warn("itch cave id is not a safe URI segment");
        return None;
    }
    let root = if let Some(custom) = cave.text("custom_install_folder") {
        env.expand_path(&custom).ok()?
    } else {
        let location = tables.locations.get(&cave.text("install_location_id")?)?;
        let base = env.expand_path(&location.text("path")?).ok()?;
        let folder = cave.text("install_folder_name")?;
        if !super::plain_name(&folder) {
            ctx.warn("itch folder name cannot leave its install location");
            return None;
        }
        env.resolve_path(&base.join(folder)).ok()?
    };
    let receipt = match metadata::receipt(env, &root) {
        Ok(r) => r,
        Err(e) => {
            ctx.warn(e.to_string());
            None
        }
    };
    let game_id = cave
        .number("game_id")
        .filter(|id| *id > 0)
        .or_else(|| receipt.as_ref()?.get("game")?.get("id")?.as_u64())?;
    let receipt = receipt.filter(|r| {
        r.get("game")
            .and_then(|g| g.get("id"))
            .and_then(|id| id.as_u64())
            .is_none_or(|n| n == game_id)
    });
    let cached = tables.games.get(&game_id.to_string());
    let title = cached
        .and_then(|g| g.text("title"))
        .or_else(|| {
            Some(
                receipt
                    .as_ref()?
                    .get("game")?
                    .get("title")?
                    .as_str()?
                    .to_owned(),
            )
        })
        .unwrap_or_else(|| format!("itch game {game_id}"));
    let upload = cave
        .text("upload_id")
        .and_then(|id| tables.uploads.get(&id));
    let platform = |name: &str| {
        upload.is_some_and(|u| u.platform(name))
            || receipt
                .as_ref()
                .and_then(|r| r.get("upload"))
                .and_then(|u| u.get("platforms"))
                .and_then(|p| p.get(name))
                .is_some_and(|v| {
                    v.as_bool() == Some(true) || v.as_str().is_some_and(|s| !s.is_empty())
                })
    };
    let windows = platform("windows")
        && match env.os() {
            OperatingSystem::Windows => true,
            OperatingSystem::Linux => !platform("linux"),
            OperatingSystem::Macos => !platform("osx"),
        };
    let mut game = DetectedGame::new(ID, id.clone(), title);
    game.ids.push(ExternalId::Itch { game_id });
    game.install_dir = Some(root.clone());
    game.launch.push(LaunchTarget::Uri {
        uri: format!("itch://caves/{id}/launch"),
    });
    if windows {
        game.runtime = GameRuntime::Windows {
            prefix: None,
            runner: None,
        };
    }
    if !root.is_dir() {
        game.state = InstallState::Broken {
            reason: "installed directory missing".into(),
        };
    }
    game.size = cave.number("installed_size");
    game.playtime_secs = cave.number("seconds_run");
    game.evidence.push(Evidence::File {
        path: file.to_owned(),
    });
    if receipt.is_some() {
        game.evidence.push(Evidence::File {
            path: root.join(".itch/receipt.json.gz"),
        });
    }
    let mut targets = match metadata::targets(env, &root, windows) {
        Ok(t) => t,
        Err(e) => {
            ctx.warn(e.to_string());
            vec![]
        }
    };
    if is_file(env, &root.join(".itch.toml")) {
        game.evidence.push(Evidence::File {
            path: root.join(".itch.toml"),
        });
    }
    if targets.is_empty()
        && let Some(files) = receipt
            .as_ref()
            .and_then(|r| r.get("files"))
            .and_then(|v| v.as_array())
        && files.len() <= 10_000
    {
        for path in files.iter().filter_map(|v| v.as_str()) {
            let lower = path.to_ascii_lowercase();
            if ((windows && lower.ends_with(".exe")) || (!windows && lower.ends_with(".sh")))
                && let Some(program) = super::relative(env, &root, path)
            {
                targets.push(metadata::Target {
                    program,
                    args: vec![],
                });
                break;
            }
        }
    }
    game.tracking = Some(TrackingHint {
        install_dir: Some(root.clone()),
        exe_names: targets
            .iter()
            .filter_map(|t| {
                t.program
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .collect(),
        steam_app_id: None,
    });
    game.launch
        .extend(targets.into_iter().map(|t| LaunchTarget::Exec {
            program: t.program,
            args: t.args,
            cwd: Some(root.clone()),
            env: BTreeMap::new(),
        }));
    Some(game)
}
