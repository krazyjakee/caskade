//! GOG Galaxy and GOG offline installs (§5, #84).
//!
//! Games come from the registry (`GOG.com\Games\<id>`, written by Galaxy
//! and by the offline installers alike), from Galaxy's `galaxy-2.0.db`
//! (installed base products, titles, playtime; with the `sqlite` feature),
//! and from each game's `goggame-<id>.info` play tasks. Field layouts follow
//! Playnite GogLibrary and game-scanner (MIT); the code is original.

#[cfg(feature = "sqlite")]
mod galaxy;

use super::{is_dir, is_file, join, json, json_text, registry_string, resolve, tokens};
use crate::{
    OperatingSystem,
    env::{HostEnv, KnownFolder},
    registry::{RegistryHive, RegistryKey, RegistryView},
    types::{
        DetectedGame, Evidence, ExternalId, GameRuntime, InstallState, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("gog");

const GAMES_KEYS: [(RegistryView, &str); 2] = [
    (RegistryView::Registry32, r"SOFTWARE\GOG.com\Games"),
    (
        RegistryView::Registry64,
        r"SOFTWARE\WOW6432Node\GOG.com\Games",
    ),
];

/// What one source of records knows about a product.
#[derive(Debug, Clone, Default)]
pub(crate) struct Record {
    pub title: Option<String>,
    pub install_dir: Option<PathBuf>,
    /// Registry `exe` / `launchCommand` fallback when there are no play tasks.
    pub exe: Option<PathBuf>,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub playtime_secs: Option<u64>,
    pub last_played_unix: Option<u64>,
    pub evidence: Vec<Evidence>,
}

impl Record {
    #[cfg(feature = "sqlite")]
    fn fill(&mut self, other: Record) {
        self.title = self.title.take().or(other.title);
        self.install_dir = self.install_dir.take().or(other.install_dir);
        self.exe = self.exe.take().or(other.exe);
        if self.args.is_empty() {
            self.args = other.args;
        }
        self.working_dir = self.working_dir.take().or(other.working_dir);
        self.playtime_secs = self.playtime_secs.max(other.playtime_secs);
        self.last_played_unix = self.last_played_unix.max(other.last_played_unix);
        self.evidence.extend(other.evidence);
    }
}

/// Every product id under `GOG.com\Games`, DLC (`dependsOn`) excluded.
fn registry_games(env: &HostEnv) -> BTreeMap<u64, Record> {
    let registry = env.registry();
    let mut games = BTreeMap::new();
    for (view, path) in GAMES_KEYS {
        let Ok(Some(key)) = registry.open_key(RegistryHive::LocalMachine, view, path) else {
            continue;
        };
        for name in registry.subkeys(&key).unwrap_or_default() {
            let Ok(id) = name.parse::<u64>() else {
                continue;
            };
            let sub = RegistryKey::new(RegistryHive::LocalMachine, view, format!(r"{path}\{name}"));
            let value = |field: &str| registry_string(env, sub.hive, sub.view, &sub.path, field);
            if value("dependsOn").is_some() {
                continue;
            }
            let launch = value("launchCommand").map(|command| tokens(&command));
            let exe = value("exe")
                .or_else(|| launch.as_ref().and_then(|tokens| tokens.first().cloned()))
                .and_then(|exe| resolve(env, &exe));
            let args = value("launchParam")
                .map(|param| tokens(&param))
                .or_else(|| launch.map(|tokens| tokens.into_iter().skip(1).collect()))
                .unwrap_or_default();
            let record = Record {
                title: value("gameName"),
                install_dir: value("path").and_then(|dir| resolve(env, &dir)),
                exe,
                args,
                working_dir: value("workingDir").and_then(|dir| resolve(env, &dir)),
                evidence: vec![Evidence::Registry {
                    key: format!(r"HKLM\{}", sub.path),
                    value: None,
                }],
                ..Record::default()
            };
            // The 32- and 64-bit views can show the same key.
            games.entry(id).or_insert(record);
        }
    }
    games
}

/// The Galaxy client, from its registry paths or the default folder.
fn galaxy_client(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        return [KnownFolder::Applications, KnownFolder::UserApplications]
            .into_iter()
            .filter_map(|f| env.known_folder(f))
            .find_map(|p| super::macos_bundle_executable(env, &p.join("GOG Galaxy.app")));
    }
    let registered = [
        (
            RegistryView::Registry32,
            r"SOFTWARE\GOG.com\GalaxyClient\paths",
        ),
        (
            RegistryView::Registry64,
            r"SOFTWARE\WOW6432Node\GOG.com\GalaxyClient\paths",
        ),
    ]
    .into_iter()
    .find_map(|(view, key)| registry_string(env, RegistryHive::LocalMachine, view, key, "client"))
    .and_then(|dir| resolve(env, &dir))
    .map(|dir| join(&dir, "GalaxyClient.exe"));
    registered.filter(|exe| is_file(env, exe)).or_else(|| {
        [KnownFolder::ProgramFilesX86, KnownFolder::ProgramFiles]
            .into_iter()
            .filter_map(|folder| env.known_folder(folder))
            .map(|folder| join(folder, "GOG Galaxy/GalaxyClient.exe"))
            .find(|exe| is_file(env, exe))
    })
}

/// `ProgramData\GOG.com\Galaxy\storage\galaxy-2.0.db`.
fn galaxy_db(env: &HostEnv) -> Option<PathBuf> {
    if env.os() == OperatingSystem::Macos {
        let path = env
            .resolve_path(Path::new(
                "/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db",
            ))
            .ok()?;
        return is_file(env, &path).then_some(path);
    }
    let program_data = env.known_folder(KnownFolder::ProgramData)?;
    Some(join(program_data, "GOG.com/Galaxy/storage/galaxy-2.0.db")).filter(|db| is_file(env, db))
}

/// The primary play task of `goggame-<id>.info`, or `None` for DLC (a
/// `rootGameId` that is another product).
struct Info {
    name: Option<String>,
    task: Option<LaunchTarget>,
    file: PathBuf,
}

fn info(env: &HostEnv, dir: &Path, id: u64, ctx: &ScanCtx) -> Option<Result<Info, ()>> {
    let file = join(dir, &format!("goggame-{id}.info"));
    if !is_file(env, &file) {
        return None;
    }
    let value = match json(env, &file, 1 << 20) {
        Ok(value) => value,
        Err(error) => {
            ctx.warn(error.to_string());
            return None;
        }
    };
    let text = |value: &Value, name: &str| json_text(value, name).map(str::to_owned);
    let root = text(&value, "rootGameId").and_then(|root| root.parse::<u64>().ok());
    if root.is_some_and(|root| root != id) {
        return Some(Err(()));
    }
    let tasks = value.get("playTasks").and_then(Value::as_array);
    let primary = tasks.and_then(|tasks| {
        tasks
            .iter()
            .find(|task| task.get("isPrimary").and_then(Value::as_bool) == Some(true))
            .or_else(|| tasks.first())
    });
    let task = primary.and_then(|task| match text(task, "type").as_deref() {
        Some("URLTask") => text(task, "link").map(|uri| LaunchTarget::Uri { uri }),
        _ => {
            let path = text(task, "path")?;
            if path.split(['/', '\\']).any(|part| part == "..") {
                ctx.warn(format!(
                    "{}: play task leaves the game folder",
                    file.display()
                ));
                return None;
            }
            Some(LaunchTarget::Exec {
                program: join(dir, &path),
                args: text(task, "arguments")
                    .map(|args| tokens(&args))
                    .unwrap_or_default(),
                cwd: Some(
                    text(task, "workingDir")
                        .map_or_else(|| dir.to_path_buf(), |cwd| join(dir, &cwd)),
                ),
                env: BTreeMap::new(),
            })
        }
    });
    Some(Ok(Info {
        name: text(&value, "name"),
        task,
        file,
    }))
}

#[derive(Debug, Default)]
pub struct GogGalaxy;

impl Source for GogGalaxy {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS.with(OperatingSystem::Macos)
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        match galaxy_client(env) {
            Some(exe) => vec![LauncherStatus::found(
                ID,
                LauncherKind::Native,
                Some(exe),
                galaxy_db(env).and_then(|db| db.parent().map(Path::to_path_buf)),
            )],
            None => vec![LauncherStatus::not_found(ID)],
        }
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let records = registry_games(env);
        ctx.check()?;
        #[cfg(feature = "sqlite")]
        let records = {
            let mut records = records;
            if let Some(db) = galaxy_db(env) {
                for (id, record) in galaxy::records(env, &db, ctx) {
                    records.entry(id).or_default().fill(record);
                }
            }
            records
        };
        let client = galaxy_client(env);
        let mut games = Vec::new();
        for (id, record) in records {
            ctx.check()?;
            if let Some(game) = game(env, id, record, client.as_deref(), ctx) {
                games.push(game);
            }
        }
        Ok(games)
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        galaxy_db(env)
            .into_iter()
            .map(|path| WatchPath {
                path,
                recursive: false,
            })
            .collect()
    }
}

fn game(
    env: &HostEnv,
    id: u64,
    mut record: Record,
    client: Option<&Path>,
    ctx: &ScanCtx,
) -> Option<DetectedGame> {
    let dir = record.install_dir.clone();
    let info = match dir.as_deref().and_then(|dir| info(env, dir, id, ctx)) {
        Some(Err(())) => return None,
        Some(Ok(info)) => Some(info),
        None => None,
    };
    let title = record
        .title
        .take()
        .or_else(|| info.as_ref().and_then(|info| info.name.clone()))
        .or_else(|| dir.as_ref()?.file_name()?.to_str().map(str::to_owned));
    let Some(title) = title else {
        ctx.warn(format!("GOG product {id} has no title or folder"));
        return None;
    };
    let mut game = DetectedGame::new(ID, id.to_string(), title);
    game.ids = vec![ExternalId::Gog { product_id: id }];
    if let (Some(client), Some(dir)) = (client, dir.as_ref()) {
        game.launch.push(LaunchTarget::Exec {
            program: client.to_path_buf(),
            args: vec![
                "/command=runGame".into(),
                format!("/gameId={id}"),
                format!("/path={}", dir.display()),
            ],
            cwd: None,
            env: BTreeMap::new(),
        });
    }
    let direct = info
        .as_ref()
        .and_then(|info| info.task.clone())
        .or_else(|| {
            let exe = record.exe.clone()?;
            Some(LaunchTarget::Exec {
                program: exe,
                args: record.args.clone(),
                cwd: record.working_dir.clone().or_else(|| dir.clone()),
                env: BTreeMap::new(),
            })
        });
    game.launch.extend(direct);
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
    game.playtime_secs = record.playtime_secs;
    game.last_played_unix = record.last_played_unix;
    game.evidence = record.evidence;
    if let Some(info) = &info {
        game.evidence.push(Evidence::File {
            path: info.file.clone(),
        });
    }
    let exe_names = game
        .launch
        .iter()
        .filter_map(|target| match target {
            LaunchTarget::Exec { program, .. } if Some(program.as_path()) != client => {
                program.file_name()?.to_str().map(str::to_owned)
            }
            _ => None,
        })
        .collect();
    game.tracking = Some(TrackingHint {
        steam_app_id: None,
        install_dir: dir.clone(),
        exe_names,
    });
    game.install_dir = dir;
    Some(game)
}
