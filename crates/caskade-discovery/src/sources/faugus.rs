//! Original Faugus launcher reader (#93); upstream JSON/path definitions are
//! format references only, https://github.com/Faugus/faugus-launcher.
use super::{canonical, is_file, json, resolve, tokens};
use crate::{HostEnv, env::KnownFolder, types::*};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
pub const ID: SourceId = SourceId::new("faugus");
pub const FLATPAK_ID: &str = "io.github.Faugus.faugus-launcher";
#[derive(Default, Debug)]
pub struct Faugus;
fn files(env: &HostEnv) -> Vec<(PathBuf, bool)> {
    let mut candidates = vec![];
    // Current versions moved games.json from config to data; prefer data.
    for folder in [KnownFolder::DataHome, KnownFolder::ConfigHome] {
        if let Some(root) = env.known_folder(folder) {
            candidates.push((root.join("faugus-launcher/games.json"), false));
        }
    }
    for folder in ["data", "config"] {
        candidates.push((
            env.home().join(format!(
                ".var/app/{FLATPAK_ID}/{folder}/faugus-launcher/games.json"
            )),
            true,
        ));
    }
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|(p, _)| is_file(env, p))
        .filter(|(p, _)| seen.insert(canonical(env, p)))
        .collect()
}
impl Source for Faugus {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let mut seen = BTreeSet::new();
        let statuses: Vec<_> = files(env)
            .into_iter()
            .filter(|(_, flat)| seen.insert(*flat))
            .map(|(file, flat)| {
                let kind = LauncherKind::native_or_flatpak(flat, FLATPAK_ID);
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
        let mut games = BTreeMap::new();
        for (file, flat) in files(env) {
            ctx.check()?;
            let values = match json(env, &file, 8 << 20) {
                Ok(value) => value,
                Err(e) => {
                    ctx.warn(e.to_string());
                    continue;
                }
            };
            let Some(values) = values.as_array() else {
                ctx.warn(format!("{}: expected game array", file.display()));
                continue;
            };
            if values.len() > 10_000 {
                ctx.warn("Faugus game count budget exceeded");
                continue;
            }
            for value in values {
                ctx.check()?;
                if let Some(game) = game(env, ctx, &file, flat, value) {
                    games.entry(game.key.clone()).or_insert(game);
                }
            }
        }
        Ok(games.into_values().collect())
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        files(env)
            .into_iter()
            .map(|(path, _)| WatchPath {
                path,
                recursive: false,
            })
            .collect()
    }
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.contains('\0'))
}
fn game(
    env: &HostEnv,
    ctx: &ScanCtx,
    file: &Path,
    flat: bool,
    value: &Value,
) -> Option<DetectedGame> {
    let title = text(value, "title")?;
    let id = text(value, "gameid")?;
    let path = text(value, "path")?;
    let mut game = DetectedGame::new(
        ID,
        format!("{}:{id}", if flat { "flatpak" } else { "native" }),
        title,
    );
    let args = text(value, "game_arguments")
        .map(tokens)
        .unwrap_or_default();
    if args.len() > 256 {
        ctx.warn(format!("Faugus {id}: argument budget exceeded"));
        return None;
    }
    if path.starts_with("steam://") {
        game.launch.push(LaunchTarget::Uri { uri: path.into() });
    } else {
        let program = env.expand_path(path).ok()?;
        game.install_dir = program.parent().map(Path::to_owned);
        game.launch.push(LaunchTarget::Exec {
            program: program.clone(),
            args,
            cwd: game.install_dir.clone(),
            env: BTreeMap::new(),
        });
        if text(value, "runner") != Some("Linux-Native")
            && (text(value, "prefix").is_some()
                || program
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("exe")))
        {
            game.runtime = GameRuntime::Windows {
                prefix: text(value, "prefix")
                    .and_then(|p| env.expand_path(p).ok())
                    .map(|path| Prefix {
                        path,
                        arch: None,
                        owner: Some(ID),
                    }),
                runner: Some(RunnerHint {
                    kind: RunnerKind::Umu,
                    name: text(value, "runner").map(str::to_owned),
                    path: None,
                }),
            };
        }
        game.tracking = Some(TrackingHint {
            install_dir: game.install_dir.clone(),
            exe_names: program
                .file_name()
                .map(|n| vec![n.to_string_lossy().into_owned()])
                .unwrap_or_default(),
            steam_app_id: None,
        });
    }
    game.icon = text(value, "icon")
        .and_then(|p| resolve(env, p))
        .filter(|p| is_file(env, p));
    if let Some(cover) = text(value, "cover")
        .and_then(|p| resolve(env, p))
        .filter(|p| is_file(env, p))
    {
        game.artwork.push(Artwork {
            kind: ArtworkKind::Cover,
            path: cover,
        });
    }
    game.playtime_secs = value.get("playtime").and_then(Value::as_u64);
    // Faugus writes naive local ISO timestamps. Without the captured timezone
    // their epoch is ambiguous, so no last_played_unix is invented.
    if ["pre_launch", "post_launch", "launch_arguments"]
        .iter()
        .any(|key| text(value, key).is_some())
    {
        ctx.warn(format!("Faugus {id}: launcher hooks/environment require the owning launcher; discovery does not execute them"));
    }
    game.evidence.push(Evidence::File {
        path: file.to_owned(),
    });
    Some(game)
}
