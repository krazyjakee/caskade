//! Lutris pga.db + game YAML (#76). Original implementation, format reference
//! only: https://github.com/lutris/lutris (GPL-3.0); no code was ported.
#[cfg(feature = "sqlite")]
mod config;
#[cfg(feature = "sqlite")]
mod database;
use super::{canonical, is_file};
use crate::{HostEnv, env::KnownFolder, types::*};
#[cfg(feature = "sqlite")]
use std::{collections::BTreeMap, path::Path};
use std::{collections::BTreeSet, path::PathBuf};
pub const ID: SourceId = SourceId::new("lutris");
pub const FLATPAK_ID: &str = "net.lutris.Lutris";
#[derive(Default, Debug)]
pub struct Lutris;
struct Location {
    data: PathBuf,
    flatpak: bool,
}
fn locations(env: &HostEnv) -> Vec<Location> {
    let mut candidates = vec![];
    if let Some(data) = env.known_folder(KnownFolder::DataHome) {
        candidates.push(Location {
            data: data.join("lutris"),
            flatpak: false,
        });
    }
    candidates.push(Location {
        data: env.home().join(".var/app/net.lutris.Lutris/data/lutris"),
        flatpak: true,
    });
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|l| is_file(env, &l.data.join("pga.db")))
        .filter(|l| seen.insert(canonical(env, &l.data)))
        .collect()
}
impl Source for Lutris {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let statuses: Vec<_> = locations(env)
            .into_iter()
            .map(|l| {
                let kind = LauncherKind::native_or_flatpak(l.flatpak, FLATPAK_ID);
                LauncherStatus::found(ID, kind, None, Some(l.data))
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
            ctx.warn("Lutris database discovery requires the sqlite feature");
            Ok(vec![])
        }
        #[cfg(feature = "sqlite")]
        {
            let mut games = vec![];
            for location in locations(env) {
                ctx.check()?;
                let path = env
                    .resolve_path(&location.data.join("pga.db"))
                    .map_err(|e| SourceError::io(&location.data, e))?;
                let rows = match database::records(&path) {
                    Ok(rows) => rows,
                    Err(e) => {
                        ctx.warn(e.to_string());
                        continue;
                    }
                };
                for row in rows {
                    ctx.check()?;
                    match game(env, &location, &path, &row, ctx) {
                        Some(game) => games.push(game),
                        None => ctx.warn(format!(
                            "{}: invalid installed game identity",
                            path.display()
                        )),
                    }
                }
            }
            games.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(games)
        }
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        locations(env)
            .into_iter()
            .flat_map(|l| {
                let old_config = if l.flatpak {
                    Some(
                        env.home()
                            .join(".var/app/net.lutris.Lutris/config/lutris/games"),
                    )
                } else {
                    env.known_folder(KnownFolder::ConfigHome)
                        .map(|p| p.join("lutris/games"))
                };
                let mut paths = vec![
                    WatchPath {
                        path: l.data.join("pga.db"),
                        recursive: false,
                    },
                    WatchPath {
                        path: l.data.join("pga.db-wal"),
                        recursive: false,
                    },
                    WatchPath {
                        path: l.data.join("games"),
                        recursive: true,
                    },
                ];
                if let Some(path) = old_config {
                    paths.push(WatchPath {
                        path,
                        recursive: true,
                    });
                }
                paths
            })
            .collect()
    }
}
#[cfg(feature = "sqlite")]
fn mapped(env: &HostEnv, value: &str, base: Option<&Path>) -> Option<PathBuf> {
    let mut value = value.to_owned();
    if let Some(base) = base {
        value = value
            .replace("${GAMEDIR}", &base.to_string_lossy())
            .replace("$GAMEDIR", &base.to_string_lossy());
    }
    let path = Path::new(&value);
    // `has_root`, not `is_absolute`: a Unix profile's `/home/…` has no drive,
    // so a Windows host wouldn't call it absolute, and joining it to `base`
    // would keep only the base's drive.
    if path.has_root()
        || value.starts_with(['~', '$', '%'])
        || value.as_bytes().get(1) == Some(&b':')
    {
        env.expand_path(&value).ok()
    } else {
        env.resolve_path(&base?.join(path)).ok()
    }
}
#[cfg(feature = "sqlite")]
fn game(
    env: &HostEnv,
    location: &Location,
    db: &Path,
    row: &database::Record,
    ctx: &ScanCtx,
) -> Option<DetectedGame> {
    let id = row.text("id")?.parse::<u64>().ok().filter(|id| *id > 0)?;
    let title = row.text("name")?;
    let slug = row.text("slug");
    let mut game = DetectedGame::new(
        ID,
        format!(
            "{}:{id}",
            if location.flatpak {
                "flatpak"
            } else {
                "native"
            }
        ),
        title,
    );
    if let Some(slug) = &slug {
        game.ids.push(ExternalId::Lutris { slug: slug.clone() });
    }
    if let (Some(service), Some(id)) = (row.text("service"), row.text("service_id")) {
        match service.as_str() {
            "steam" => {
                if let Ok(app_id) = id.parse::<u32>()
                    && app_id > 0
                {
                    game.ids.push(ExternalId::Steam { app_id });
                }
            }
            "gog" => {
                if let Ok(product_id) = id.parse::<u64>()
                    && product_id > 0
                {
                    game.ids.push(ExternalId::Gog { product_id });
                }
            }
            _ => {}
        }
    }
    game.install_dir = row.text("directory").and_then(|p| mapped(env, &p, None));
    // A persisted launcher row is not proof that its installation still exists.
    // Retain its identity and launch metadata for repair, but do not advertise
    // an unavailable directory as a certain installed game.
    if game.install_dir.as_ref().is_some_and(|path| !path.is_dir()) {
        game.state = InstallState::Broken {
            reason: "Lutris install directory is missing or unavailable".into(),
        };
        game.confidence = Confidence::Maybe;
    }
    // Lutris persists decimal hours, while discovery's public unit is seconds.
    game.playtime_secs = row
        .number("playtime")
        .map(|hours| (hours * 3600.).round().min(u64::MAX as f64) as u64);
    game.last_played_unix = row
        .number("lastplayed")
        .filter(|n| *n > 0.)
        .map(|n| n.min(u64::MAX as f64) as u64);
    let uri = format!("lutris:rungameid/{id}");
    game.launch.push(if location.flatpak {
        LaunchTarget::Flatpak {
            app_id: FLATPAK_ID.into(),
            args: vec![uri],
        }
    } else {
        LaunchTarget::Uri { uri }
    });
    game.evidence.push(Evidence::File {
        path: db.to_owned(),
    });
    let key = row.text("configpath").or(slug);
    let config =
        key.and_then(
            |key| match config::load(env, &location.data, location.flatpak, &key) {
                Ok(config) => config,
                Err(e) => {
                    ctx.warn(e.to_string());
                    None
                }
            },
        );
    let runner = row.text("runner").unwrap_or_default();
    let mut exe = row.text("executable");
    let mut args = vec![];
    let mut cwd = game.install_dir.clone();
    if runner.starts_with("wine") || runner.contains("proton") {
        game.runtime = GameRuntime::Windows {
            prefix: None,
            runner: Some(RunnerHint {
                kind: if runner.contains("proton") {
                    RunnerKind::Proton
                } else {
                    RunnerKind::Wine
                },
                name: Some(runner),
                path: None,
            }),
        };
    }
    if let Some((file, config)) = config {
        game.evidence.push(Evidence::File { path: file });
        exe = config.game.exe.or(exe);
        match config.game.args.argv() {
            Ok(argv) => args = argv,
            Err(e) => ctx.warn(format!("Lutris {id}: {e}")),
        }
        cwd = config
            .game
            .working_dir
            .as_deref()
            .and_then(|p| mapped(env, p, game.install_dir.as_deref()))
            .or(cwd);
        if let GameRuntime::Windows { prefix, runner } = &mut game.runtime {
            *prefix = config
                .game
                .prefix
                .as_deref()
                .and_then(|p| mapped(env, p, game.install_dir.as_deref()))
                .map(|path| Prefix {
                    path,
                    arch: match config.game.arch.as_deref() {
                        Some("win32") => Some(PrefixArch::Win32),
                        Some("win64") => Some(PrefixArch::Win64),
                        _ => None,
                    },
                    owner: Some(ID),
                });
            if let (Some(runner), Some(version)) = (runner, config.wine.version) {
                runner.name = Some(version);
            }
        }
    }
    let program = exe.and_then(|exe| {
        if exe.as_bytes().get(1) == Some(&b':')
            && let GameRuntime::Windows {
                prefix: Some(prefix),
                ..
            } = &game.runtime
        {
            let drive = exe[..1].to_ascii_lowercase();
            if drive.bytes().all(|b| b.is_ascii_lowercase()) {
                return env
                    .resolve_path(
                        &prefix
                            .path
                            .join(format!("drive_{drive}"))
                            .join(exe[2..].trim_start_matches(['\\', '/']).replace('\\', "/")),
                    )
                    .ok();
            }
        }
        mapped(env, &exe, game.install_dir.as_deref())
    });
    if let Some(program) = &program {
        game.launch.push(LaunchTarget::Exec {
            program: program.clone(),
            args,
            cwd,
            env: BTreeMap::new(),
        });
    }
    game.tracking = Some(TrackingHint {
        steam_app_id: game.ids.iter().find_map(|id| {
            if let ExternalId::Steam { app_id } = id {
                Some(*app_id)
            } else {
                None
            }
        }),
        install_dir: game.install_dir.clone(),
        exe_names: program
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| vec![n.to_string_lossy().into_owned()])
            .unwrap_or_default(),
    });
    Some(game)
}
