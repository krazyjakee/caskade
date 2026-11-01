//! Amazon Games (§5, #88): `%LOCALAPPDATA%\Amazon Games\Data\Games\Sql\
//! GameInstallInfo.sqlite`, table `DbSet` (`Id`, `ProductTitle`,
//! `InstallDirectory`, `Installed`), read through the shared bounded SQLite
//! reader. Layout from BoilR (Apache-2.0) and game-scanner (MIT) as
//! documentation; the code is original.

use super::{is_dir, is_file, join, resolve};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    parsers::sqlite::{Database, DatabaseLimits, SqlValue},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use std::path::PathBuf;

pub const ID: SourceId = SourceId::new("amazon");

fn root(env: &HostEnv) -> Option<PathBuf> {
    let local = env.known_folder(KnownFolder::LocalAppData)?;
    Some(join(local, "Amazon Games")).filter(|dir| is_dir(env, dir))
}

fn database(env: &HostEnv) -> Option<PathBuf> {
    Some(join(&root(env)?, "Data/Games/Sql/GameInstallInfo.sqlite")).filter(|db| is_file(env, db))
}

#[derive(Debug, Default)]
pub struct AmazonGames;

impl Source for AmazonGames {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let Some(root) = root(env) else {
            return vec![LauncherStatus::not_found(ID)];
        };
        let exe = join(&root, "App/Amazon Games.exe");
        vec![LauncherStatus::found(
            ID,
            LauncherKind::Native,
            is_file(env, &exe).then_some(exe),
            Some(join(&root, "Data")),
        )]
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let Some(db) = database(env) else {
            return Ok(Vec::new());
        };
        let mut database = Database::open(&db, DatabaseLimits::default())
            .map_err(|error| SourceError::parse(&db, error))?;
        let rows = database
            .query(
                "SELECT Id, ProductTitle, InstallDirectory, Installed FROM DbSet",
                &[],
            )
            .map_err(|error| SourceError::parse(&db, error))?
            .rows;
        let text = |value: &SqlValue| match value {
            SqlValue::Text(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
            _ => None,
        };
        let mut games: Vec<DetectedGame> = Vec::new();
        for row in rows {
            ctx.check()?;
            let [id, title, dir, installed] = row.as_slice() else {
                continue;
            };
            if !matches!(installed, SqlValue::Integer(1)) {
                continue;
            }
            let Some(id) = text(id) else { continue };
            if games.iter().any(|game| game.key == id) {
                continue;
            }
            let dir = text(dir).and_then(|dir| resolve(env, &dir));
            let Some(title) =
                text(title).or_else(|| dir.as_ref()?.file_name()?.to_str().map(str::to_owned))
            else {
                ctx.warn(format!("Amazon game {id} has no title"));
                continue;
            };
            let mut game = DetectedGame::new(ID, id.clone(), title);
            game.ids = vec![ExternalId::Amazon { id: id.clone() }];
            game.launch = vec![LaunchTarget::Uri {
                uri: format!("amazon-games://play/{id}"),
            }];
            game.runtime = if env.os() == OperatingSystem::Windows {
                GameRuntime::Windows {
                    prefix: None,
                    runner: None,
                }
            } else {
                GameRuntime::Native
            };
            game.state = if dir.as_ref().is_some_and(|dir| is_dir(env, dir)) {
                InstallState::Installed
            } else {
                InstallState::Broken {
                    reason: "install folder missing".into(),
                }
            };
            game.evidence = vec![Evidence::File { path: db.clone() }];
            game.tracking = Some(TrackingHint {
                steam_app_id: None,
                install_dir: dir.clone(),
                exe_names: Vec::new(),
            });
            game.install_dir = dir;
            games.push(game);
        }
        games.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(games)
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        database(env)
            .map(|path| WatchPath {
                path,
                recursive: false,
            })
            .into_iter()
            .collect()
    }
}
