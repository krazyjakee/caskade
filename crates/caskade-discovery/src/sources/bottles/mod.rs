//! Bottles (§5, #92): programs users added to bottles, from the native
//! (`$XDG_DATA_HOME/bottles`) and Flatpak (`~/.var/app/com.usebottles.bottles/
//! data/bottles`) data dirs.
//!
//! Each bottle's `bottle.yml` lists `External_Programs`; `library.yml` holds
//! the programs the user put in the Bottles library. Bottles also holds
//! ordinary applications, so programs outside the library go through the
//! classifier (#79). Every bottle is a Wine prefix and is also handed to the
//! prefix scan (#91). Formats follow BoilR (Apache-2.0) as documentation and
//! Cartridges (GPL-3.0) as a format reference only; the code is original.

use super::{is_dir, is_file, join, list_or_warn, read, resolve};
use crate::{
    OperatingSystem,
    classify::{Metadata, classify_path},
    env::{HostEnv, KnownFolder},
    types::{
        Confidence, DetectedGame, Evidence, GameRuntime, LaunchTarget, LauncherKind,
        LauncherStatus, OsSet, Prefix, PrefixArch, RunnerHint, RunnerKind, ScanCtx, Source,
        SourceError, SourceId, TrackingHint, WatchPath,
    },
};
use caskade_installer::host::{FileSystemCaps, HostCaps, PackageCapabilities, RunnerInfo};
use serde_yaml_ng::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const ID: SourceId = SourceId::new("bottles");
pub const FLATPAK_ID: &str = "com.usebottles.bottles";
const LIMIT: u64 = 4 << 20;

/// One bottle: a Wine prefix with its configuration.
#[derive(Debug, Clone)]
pub(crate) struct Bottle {
    pub name: String,
    pub path: PathBuf,
    pub runner: Option<String>,
    pub arch: Option<PrefixArch>,
    pub flatpak: bool,
    pub config: PathBuf,
    pub programs: Vec<Program>,
}

#[derive(Debug, Clone)]
pub(crate) struct Program {
    pub id: String,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub folder: Option<PathBuf>,
}

/// Bottles data dirs: (path, Flatpak).
fn data_dirs(env: &HostEnv) -> Vec<(PathBuf, bool)> {
    let home = env.home();
    let data = env
        .known_folder(KnownFolder::DataHome)
        .map_or_else(|| join(home, ".local/share"), Path::to_path_buf);
    let mut dirs = vec![(join(&data, "bottles"), false)];
    let native = join(home, ".local/share/bottles");
    if !dirs.iter().any(|(dir, _)| *dir == native) {
        dirs.push((native, false));
    }
    dirs.push((
        join(home, &format!(".var/app/{FLATPAK_ID}/data/bottles")),
        true,
    ));
    dirs.into_iter()
        .filter(|(dir, _)| is_dir(env, dir))
        .collect()
}

fn yaml(env: &HostEnv, path: &Path, ctx: Option<&ScanCtx>) -> Option<Value> {
    if !is_file(env, path) {
        return None;
    }
    let parsed = read(env, path, LIMIT)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_yaml_ng::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        });
    match parsed {
        Ok(value) => Some(value),
        Err(error) => {
            if let Some(ctx) = ctx {
                ctx.warn(format!("{}: {error}", path.display()));
            }
            None
        }
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Every bottle in every data dir.
pub(crate) fn bottles(env: &HostEnv, ctx: Option<&ScanCtx>) -> Vec<Bottle> {
    let mut found = Vec::new();
    for (data, flatpak) in data_dirs(env) {
        let root = join(&data, "bottles");
        for name in list_or_warn(env, &root, ctx) {
            let config = join(&root, &format!("{name}/bottle.yml"));
            let Some(value) = yaml(env, &config, ctx) else {
                continue;
            };
            let custom = value.get("Custom_Path").and_then(Value::as_bool) == Some(true);
            let path = match text(&value, "Path") {
                Some(path) if custom => resolve(env, &path),
                Some(path) if super::plain_name(&path) => Some(join(&root, &path)),
                _ => Some(join(&root, &name)),
            };
            let Some(path) = path else { continue };
            let programs = value
                .get("External_Programs")
                .and_then(Value::as_mapping)
                .into_iter()
                .flatten()
                .filter_map(|(id, program)| {
                    if program.get("removed").and_then(Value::as_bool) == Some(true) {
                        return None;
                    }
                    let id = id.as_str()?.to_owned();
                    let name = text(program, "name")?;
                    let executable =
                        text(program, "path").and_then(|exe| program_path(env, &path, &exe));
                    let folder = text(program, "folder")
                        .and_then(|dir| program_path(env, &path, &dir))
                        .or_else(|| executable.as_ref()?.parent().map(Path::to_path_buf));
                    Some(Program {
                        id,
                        name,
                        executable,
                        folder,
                    })
                })
                .collect();
            found.push(Bottle {
                name: text(&value, "Name").unwrap_or(name),
                runner: text(&value, "Runner"),
                arch: match text(&value, "Arch").as_deref() {
                    Some("win64") => Some(PrefixArch::Win64),
                    Some("win32") => Some(PrefixArch::Win32),
                    _ => None,
                },
                flatpak,
                config,
                programs,
                path,
            });
        }
    }
    found
}

/// A program path as Bottles stores it: a host path, or a Windows path
/// inside the bottle (`C:\…` is the bottle's `drive_c`).
fn program_path(env: &HostEnv, bottle: &Path, text: &str) -> Option<PathBuf> {
    let bytes = text.as_bytes();
    if bytes.len() >= 3 && bytes[0].eq_ignore_ascii_case(&b'c') && bytes[1] == b':' {
        let relative = text[2..].trim_start_matches(['\\', '/']);
        return Some(join(&join(bottle, "drive_c"), relative));
    }
    resolve(env, text)
}

/// Program ids in `library.yml`, per bottle name.
fn library(env: &HostEnv, ctx: &ScanCtx) -> BTreeSet<(String, String)> {
    let mut entries = BTreeSet::new();
    for (data, _) in data_dirs(env) {
        let Some(value) = yaml(env, &join(&data, "library.yml"), Some(ctx)) else {
            continue;
        };
        for (_, entry) in value.as_mapping().into_iter().flatten() {
            let bottle = entry.get("bottle").and_then(|bottle| text(bottle, "name"));
            if let (Some(bottle), Some(id)) = (bottle, text(entry, "id")) {
                entries.insert((bottle, id));
            }
        }
    }
    entries
}

/// Programs in a bottle are Windows programs, whatever the host is.
fn windows_caps() -> HostCaps {
    HostCaps {
        os: OperatingSystem::Windows,
        arch: "x86_64".into(),
        runners: vec![RunnerInfo {
            kind: caskade_installer::host::RunnerKind::Native,
            program: PathBuf::new(),
        }],
        seven_zip: None,
        proton_builds: vec![],
        filesystem: FileSystemCaps {
            writable: false,
            exec_bits: false,
            symlinks: false,
            case_sensitive: false,
        },
        packages: PackageCapabilities::default(),
    }
}

#[derive(Debug, Default)]
pub struct Bottles;

impl Source for Bottles {
    fn id(&self) -> SourceId {
        ID
    }

    fn hosts(&self) -> OsSet {
        OsSet::LINUX
    }

    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let statuses: Vec<_> = data_dirs(env)
            .into_iter()
            .map(|(dir, flatpak)| {
                let kind = LauncherKind::native_or_flatpak(flatpak, FLATPAK_ID);
                LauncherStatus::found(ID, kind, None, Some(dir))
            })
            .collect();
        if statuses.is_empty() {
            vec![LauncherStatus::not_found(ID)]
        } else {
            statuses
        }
    }

    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let library = library(env, ctx);
        let caps = windows_caps();
        let mut games = BTreeMap::new();
        for bottle in bottles(env, Some(ctx)) {
            for program in &bottle.programs {
                ctx.check()?;
                let key = format!("{}/{}", bottle.name, program.id);
                if games.contains_key(&key) {
                    continue;
                }
                let in_library = library.contains(&(bottle.name.clone(), program.id.clone()));
                let (confidence, evidence) = if in_library {
                    (Confidence::Certain, Vec::new())
                } else {
                    let Some(folder) = &program.folder else {
                        continue;
                    };
                    let metadata = Metadata {
                        title: Some(program.name.clone()),
                        executable: program.executable.clone(),
                        install_dir: Some(folder.clone()),
                        ..Metadata::default()
                    };
                    match classify_path(env, &caps, folder, &metadata, &ctx.config().classifier) {
                        Ok(classification) => match classification.confidence() {
                            Some(confidence)
                                if confidence >= ctx.config().classifier.min_confidence =>
                            {
                                (confidence, classification.evidence)
                            }
                            _ => continue,
                        },
                        Err(error) => {
                            ctx.warn(format!("{}: {error}", folder.display()));
                            continue;
                        }
                    }
                };
                let mut game = DetectedGame::new(ID, key.clone(), program.name.clone());
                game.install_dir = program.folder.clone();
                game.confidence = confidence;
                game.launch = vec![launch(&bottle, program)];
                game.runtime = GameRuntime::Windows {
                    prefix: Some(Prefix {
                        path: bottle.path.clone(),
                        arch: bottle.arch,
                        owner: Some(ID),
                    }),
                    runner: bottle.runner.clone().map(|name| RunnerHint {
                        kind: if name.to_ascii_lowercase().contains("proton") {
                            RunnerKind::Proton
                        } else {
                            RunnerKind::Wine
                        },
                        name: Some(name),
                        path: None,
                    }),
                };
                game.evidence = std::iter::once(Evidence::File {
                    path: bottle.config.clone(),
                })
                .chain(evidence)
                .collect();
                game.tracking = Some(TrackingHint {
                    steam_app_id: None,
                    install_dir: program.folder.clone(),
                    exe_names: program
                        .executable
                        .as_ref()
                        .and_then(|exe| exe.file_name()?.to_str().map(str::to_owned))
                        .into_iter()
                        .collect(),
                });
                games.insert(key, game);
            }
        }
        Ok(games.into_values().collect())
    }

    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let mut paths: Vec<_> = data_dirs(env)
            .into_iter()
            .map(|(dir, _)| WatchPath {
                path: join(&dir, "library.yml"),
                recursive: false,
            })
            .collect();
        paths.extend(bottles(env, None).into_iter().map(|bottle| WatchPath {
            path: bottle.config,
            recursive: false,
        }));
        paths
    }

    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        let home = env.home();
        vec![
            join(home, ".local/share/bottles"),
            join(home, &format!(".var/app/{FLATPAK_ID}/data/bottles")),
        ]
    }
}

/// `bottles-cli run -b <bottle> -p <program>`, through `flatpak run` for
/// the Flatpak install.
fn launch(bottle: &Bottle, program: &Program) -> LaunchTarget {
    let args = vec![
        "run".into(),
        "-b".into(),
        bottle.name.clone(),
        "-p".into(),
        program.name.clone(),
    ];
    if bottle.flatpak {
        LaunchTarget::Exec {
            program: PathBuf::from("flatpak"),
            args: [
                "run".into(),
                "--command=bottles-cli".into(),
                FLATPAK_ID.into(),
            ]
            .into_iter()
            .chain(args)
            .collect(),
            cwd: None,
            env: BTreeMap::new(),
        }
    } else {
        LaunchTarget::Exec {
            program: PathBuf::from("bottles-cli"),
            args,
            cwd: None,
            env: BTreeMap::new(),
        }
    }
}
