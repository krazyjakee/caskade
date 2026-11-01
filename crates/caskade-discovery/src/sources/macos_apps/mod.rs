//! Read-only macOS application bundle discovery (#95). Original implementation
//! of Apple's Info.plist format, using the shared bounded plist parser.
use super::{INFO_PLIST, is_file, plain_name, read};
use crate::{
    HostEnv, OperatingSystem,
    classify::{self, Metadata},
    data::Data,
    env::KnownFolder,
    parsers,
    types::*,
};
use caskade_installer::host::{
    FileSystemCaps, HostCaps, PackageCapabilities, RunnerInfo, RunnerKind,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
pub const ID: SourceId = SourceId::new("macos.apps");
#[derive(Debug, Default)]
pub struct MacosApps;
fn roots(env: &HostEnv) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for folder in [KnownFolder::Applications, KnownFolder::UserApplications]
        .into_iter()
        .filter_map(|f| env.known_folder(f))
    {
        paths.insert(folder.to_owned());
        paths.insert(folder.join("Games"));
    }
    paths.into_iter().collect()
}
fn host() -> HostCaps {
    HostCaps {
        os: OperatingSystem::Macos,
        arch: "x86_64".into(),
        runners: vec![RunnerInfo {
            kind: RunnerKind::Native,
            program: PathBuf::new(),
        }],
        seven_zip: None,
        proton_builds: vec![],
        filesystem: FileSystemCaps {
            writable: false,
            exec_bits: true,
            symlinks: true,
            case_sensitive: false,
        },
        packages: PackageCapabilities::default(),
    }
}
fn string<'a>(dict: &'a plist::Dictionary, key: &str) -> Option<&'a str> {
    dict.get(key)?
        .as_string()
        .filter(|s| !s.trim().is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
}
fn category_game(category: &str) -> bool {
    category == "public.app-category.games"
        || category
            .strip_prefix("public.app-category.")
            .is_some_and(|v| v.ends_with("-games") && !v.contains(['/', ':', ' ']))
}
fn bundle(env: &HostEnv, dir: &Path, ctx: &ScanCtx) -> Result<Option<DetectedGame>, SourceError> {
    ctx.check()?;
    let dir = env.resolve_path(dir).map_err(|e| SourceError::io(dir, e))?;
    let info = dir.join("Contents/Info.plist");
    let bytes = read(env, &info, INFO_PLIST.max_input_bytes as u64)
        .map_err(|e| SourceError::io(&info, e))?;
    let value =
        parsers::plist::parse(&bytes, &INFO_PLIST).map_err(|e| SourceError::parse(&info, e))?;
    let Some(dict) = value.as_dictionary() else {
        return Err(SourceError::parse(
            &info,
            "Info.plist root must be a dictionary",
        ));
    };
    let identifier = string(dict, "CFBundleIdentifier");
    let data = Data::builtin().map_err(|e| SourceError::Other {
        message: e.to_string(),
    })?;
    if identifier.is_some_and(|id| {
        data.launchers.launchers.iter().any(|l| {
            l.bundle_ids
                .iter()
                .any(|known| known.eq_ignore_ascii_case(id))
        })
    }) {
        return Ok(None);
    }
    let Some(exe) = string(dict, "CFBundleExecutable").filter(|s| plain_name(s)) else {
        return Err(SourceError::parse(
            &info,
            "missing or unsafe CFBundleExecutable",
        ));
    };
    let executable = env
        .resolve_path(&dir.join("Contents/MacOS").join(exe))
        .map_err(|e| SourceError::io(&info, e))?;
    // Internal symlinks are allowed only when they resolve inside this bundle.
    let canonical = crate::identity::canonical_path(env, &dir, &GameRuntime::Native)
        .map_err(|e| SourceError::parse(&info, e))?;
    let target = crate::identity::canonical_path(env, &executable, &GameRuntime::Native)
        .map_err(|e| SourceError::parse(&info, e))?;
    if !canonical.contains(&target) {
        return Err(SourceError::parse(
            &info,
            "bundle executable escapes bundle",
        ));
    }
    let title = string(dict, "CFBundleDisplayName")
        .or_else(|| string(dict, "CFBundleName"))
        .map(str::to_owned)
        .or_else(|| dir.file_stem()?.to_str().map(str::to_owned))
        .unwrap_or_else(|| "macOS game".into());
    let Some(key) = dir.to_str() else {
        ctx.warn("skipped non-Unicode app bundle path");
        return Ok(None);
    };
    let category = string(dict, "LSApplicationCategoryType");
    let steam_shortcut = identifier
        .and_then(|id| id.strip_prefix("com.valvesoftware.steam.app."))
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|id| id.parse::<u32>().ok())
        .filter(|id| *id > 0);

    let mut game = DetectedGame::new(ID, key, &title);
    game.install_dir = Some(dir.clone());
    game.launch = vec![LaunchTarget::Exec {
        program: executable.clone(),
        args: vec![],
        cwd: Some(dir.join("Contents/MacOS")),
        env: BTreeMap::new(),
    }];
    game.evidence.push(Evidence::File { path: info });
    if let Some(app_id) = steam_shortcut {
        game.ids.push(ExternalId::Steam { app_id });
        game.launch.insert(
            0,
            LaunchTarget::Uri {
                uri: format!("steam://rungameid/{app_id}"),
            },
        );
        crate::uri::mark_proxy(&mut game);
    } else if category.is_some_and(category_game) {
        game.confidence = Confidence::Certain;
        game.evidence.push(Evidence::Rule {
            rule: "metadata.game_category".into(),
            score: 100,
        });
    } else {
        let result = classify::classify_path(
            env,
            &host(),
            &dir,
            &Metadata {
                title: Some(title),
                category: category.map(str::to_owned),
                install_dir: Some(dir.clone()),
                executable: Some(executable.clone()),
                ..Metadata::default()
            },
            &ctx.config().classifier,
        )
        .map_err(|e| SourceError::Other {
            message: e.to_string(),
        })?;
        let Some(confidence) = result.confidence() else {
            return Ok(None);
        };
        game.confidence = confidence;
        game.evidence.extend(result.evidence);
        for warning in result.warnings {
            ctx.warn(warning);
        }
    }
    if !is_file(env, &executable) {
        game.confidence = Confidence::Maybe;
        game.state = InstallState::Broken {
            reason: "app bundle executable missing".into(),
        };
    }
    if let Some(icon) = string(dict, "CFBundleIconFile").filter(|s| plain_name(s)) {
        let name = if Path::new(icon).extension().is_none() {
            format!("{icon}.icns")
        } else {
            icon.into()
        };
        if Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("icns"))
        {
            let path = dir.join("Contents/Resources").join(name);
            if let Ok(path) = env.resolve_path(&path)
                && is_file(env, &path)
                && crate::identity::canonical_path(env, &path, &GameRuntime::Native)
                    .is_ok_and(|p| canonical.contains(&p))
            {
                game.icon = Some(path);
            }
        }
    }
    let receipt = dir.join("Contents/_MASReceipt");
    if let Ok(path) = env.resolve_path(&receipt)
        && path.is_dir()
        && crate::identity::canonical_path(env, &path, &GameRuntime::Native)
            .is_ok_and(|p| canonical.contains(&p))
    {
        // A receipt identifies the Mac App Store family (including Arcade),
        // not an App Store product ID or subscription entitlement.
        game.evidence.push(Evidence::Rule {
            rule: "store.mac_app_store".into(),
            score: 0,
        });
        game.evidence.push(Evidence::File { path });
    }
    game.tracking = Some(TrackingHint {
        steam_app_id: steam_shortcut,
        install_dir: Some(dir),
        exe_names: vec![exe.into()],
    });
    Ok(Some(game))
}
impl Source for MacosApps {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::MACOS
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        ctx.check()?;
        let mut games: Vec<DetectedGame> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut remaining = usize::try_from(ctx.config().classifier.max_entries)
            .unwrap_or(usize::MAX)
            .min(10_000);
        for root in roots(env) {
            ctx.check()?;
            let root = match env.resolve_path(&root) {
                Ok(p) => p,
                Err(e) => {
                    ctx.warn(format!("app directory: {e}"));
                    continue;
                }
            };
            let entries = match fs::read_dir(&root) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    ctx.warn(format!("{}: {e}", root.display()));
                    continue;
                }
            };
            let mut entries = entries
                .take(remaining + 1)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| SourceError::io(&root, e))?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                ctx.check()?;
                if remaining == 0 {
                    ctx.warn("app bundle directory entry budget reached");
                    games.sort_by(|a, b| a.key.cmp(&b.key));
                    return Ok(games);
                }
                remaining -= 1;
                if !entry.file_type().is_ok_and(|t| t.is_dir())
                    || !entry
                        .path()
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case("app"))
                {
                    continue;
                }
                let path = entry.path();
                let identity = crate::identity::canonical_path(env, &path, &GameRuntime::Native)
                    .ok()
                    .map(|p| p.key());
                if identity.is_some_and(|i| !seen.insert(i)) {
                    continue;
                }
                match bundle(env, &path, ctx) {
                    Ok(Some(game)) => games.push(game),
                    Ok(None) => {}
                    Err(e) => ctx.warn(e.to_string()),
                }
            }
        }
        ctx.check()?;
        games.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(games)
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        roots(env)
            .into_iter()
            .map(|path| WatchPath {
                path,
                recursive: true,
            })
            .collect()
    }
}
