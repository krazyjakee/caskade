//! EA app / Origin installed games (#85). Original implementation using the
//! published registry/XML/LocalContent layouts; no upstream code is ported.
//! Format references: BoilR's origin_platform.rs (Apache-2.0) and the EA
//! installerdata.xml format described by launcher integrations. Never decrypts
//! EA Desktop's hardware-bound IS file or reads account credentials.
pub mod metadata;
use super::{REGISTRY_VIEWS, expand, is_dir, is_file, join, read, registry_string};
use crate::{
    HostEnv,
    env::KnownFolder,
    registry::{RegistryKey, RegistryValue},
    types::*,
};
use metadata::{InstallerMetadata, OriginManifest};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use url::Url;
pub const ID: SourceId = SourceId::new("ea");
const MAX_FILE: u64 = 4 << 20;
const ROOTS: &[&str] = &[
    r"SOFTWARE\EA Games",
    r"SOFTWARE\Electronic Arts",
    r"SOFTWARE\BioWare",
    r"SOFTWARE\Respawn",
    r"SOFTWARE\Maxis",
];
const INSTALL_VALUES: &[&str] = &[
    "Install Dir",
    "InstallDir",
    "InstallPath",
    "InstallLocation",
];
#[derive(Debug, Default)]
pub struct Ea;
#[derive(Clone)]
struct Candidate {
    dir: PathBuf,
    title: String,
    evidence: Vec<Evidence>,
    manifest: Option<OriginManifest>,
    modern: bool,
}
struct Client {
    modern: bool,
    path: PathBuf,
}
fn clients(env: &HostEnv) -> Vec<Client> {
    let mut found = BTreeMap::new();
    for (modern, key, names) in [
        (
            true,
            r"SOFTWARE\Electronic Arts\EA Desktop",
            &[
                "DesktopAppPath",
                "ClientPath",
                "InstallLocation",
                "Install Dir",
            ][..],
        ),
        (
            false,
            r"SOFTWARE\Origin",
            &["ClientPath", "Install Dir", "InstallDir"][..],
        ),
    ] {
        for (hive, view) in REGISTRY_VIEWS {
            for name in names {
                if let Some(raw) = registry_string(env, hive, view, key, name)
                    && let Some(mut path) = expand(env, &raw)
                {
                    if path
                        .extension()
                        .is_none_or(|v| !v.eq_ignore_ascii_case("exe"))
                    {
                        path = path.join(if modern {
                            "EADesktop.exe"
                        } else {
                            "Origin.exe"
                        });
                    }
                    if is_file(env, &path) {
                        found.insert(path.clone(), Client { modern, path });
                    }
                }
            }
        }
    }
    for folder in [KnownFolder::ProgramFiles, KnownFolder::ProgramFilesX86]
        .into_iter()
        .filter_map(|f| env.known_folder(f))
    {
        for (modern, tail) in [
            (true, "Electronic Arts/EA Desktop/EA Desktop/EADesktop.exe"),
            (false, "Origin/Origin.exe"),
        ] {
            let path = join(folder, tail);
            if is_file(env, &path) {
                found.entry(path.clone()).or_insert(Client { modern, path });
            }
        }
    }
    found.into_values().collect()
}
fn origin_data(env: &HostEnv) -> Option<PathBuf> {
    env.known_folder(KnownFolder::ProgramData)
        .map(|p| p.join("Origin/LocalContent"))
        .or_else(|| expand(env, r"C:\ProgramData\Origin\LocalContent"))
}
fn publisher_candidates(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<Candidate>, SourceError> {
    let registry = env.registry();
    let mut result = Vec::new();
    let mut count = 0;
    for (hive, view) in REGISTRY_VIEWS {
        for root in ROOTS {
            let mut pending = vec![(RegistryKey::new(hive, view, *root), 0)];
            while let Some((key, depth)) = pending.pop() {
                ctx.check()?;
                count += 1;
                if count > 10_000 {
                    ctx.warn("EA publisher registry key budget reached");
                    return Ok(result);
                }
                let values = match registry.values(&key) {
                    Ok(v) => v,
                    Err(e) => {
                        ctx.warn(format!("EA publisher registry: {e}"));
                        continue;
                    }
                };
                let string = |name: &str| {
                    values
                        .iter()
                        .find(|v| v.name.eq_ignore_ascii_case(name))
                        .and_then(|v| match &v.value {
                            RegistryValue::String(s) | RegistryValue::ExpandString(s) => {
                                Some(s.as_str())
                            }
                            _ => None,
                        })
                };
                if let Some((name, raw)) = INSTALL_VALUES
                    .iter()
                    .find_map(|name| string(name).map(|v| (*name, v)))
                    && let Some(dir) = expand(env, raw)
                {
                    result.push(Candidate {
                        dir,
                        title: string("DisplayName")
                            .unwrap_or_else(|| key.path.rsplit('\\').next().unwrap_or("EA game"))
                            .into(),
                        evidence: vec![Evidence::Registry {
                            key: format!("{:?}\\{}", hive, key.path),
                            value: Some(name.into()),
                        }],
                        manifest: None,
                        modern: !key.path.to_ascii_lowercase().contains("bioware"),
                    });
                }
                if depth < 3 {
                    match registry.subkeys(&key) {
                        Ok(names) => {
                            for name in names.into_iter().rev() {
                                pending.push((
                                    RegistryKey::new(hive, view, format!("{}\\{}", key.path, name)),
                                    depth + 1,
                                ));
                            }
                        }
                        Err(e) => ctx.warn(format!("EA publisher subkeys: {e}")),
                    }
                }
            }
        }
    }
    Ok(result)
}
fn manifest_files(env: &HostEnv, root: &Path, ctx: &ScanCtx) -> Result<Vec<PathBuf>, SourceError> {
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut files = Vec::new();
    let mut count = 0;
    while let Some((dir, depth)) = pending.pop() {
        ctx.check()?;
        let path = match env.resolve_path(&dir) {
            Ok(p) => p,
            Err(e) => {
                ctx.warn(format!("Origin manifest folder: {e}"));
                continue;
            }
        };
        let entries = match fs::read_dir(&path) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                ctx.warn(format!("Origin LocalContent: {e}"));
                continue;
            }
        };
        let mut entries = entries
            .take(10_001)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SourceError::io(&path, e))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            count += 1;
            if count > 10_000 {
                ctx.warn("Origin LocalContent entry budget reached");
                return Ok(files);
            }
            let kind = match entry.file_type() {
                Ok(k) => k,
                Err(_) => continue,
            };
            if kind.is_dir() && depth < 8 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|v| v.eq_ignore_ascii_case("mfst"))
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}
fn candidates(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<Candidate>, SourceError> {
    let mut result = publisher_candidates(env, ctx)?;
    let clients = clients(env);
    let modern_only = clients.iter().any(|c| c.modern) && !clients.iter().any(|c| !c.modern);
    let legacy_only = !clients.iter().any(|c| c.modern) && clients.iter().any(|c| !c.modern);
    if legacy_only {
        for candidate in &mut result {
            candidate.modern = false;
        }
    }
    if let Some(root) = origin_data(env) {
        for file in manifest_files(env, &root, ctx)? {
            ctx.check()?;
            let manifest = match read(env, &file, MAX_FILE)
                .and_then(|b| metadata::parse_origin_manifest(&b))
            {
                Ok(m) => m,
                Err(e) => {
                    ctx.warn(format!("{}: {e}", file.display()));
                    continue;
                }
            };
            if manifest.addon {
                continue;
            }
            let Some(dir) = expand(env, &manifest.install_dir) else {
                ctx.warn("invalid Origin install path");
                continue;
            };
            let title = manifest
                .title
                .clone()
                .or_else(|| file.parent()?.file_name()?.to_str().map(str::to_owned))
                .unwrap_or_else(|| manifest.offer.clone());
            result.push(Candidate {
                dir,
                title,
                evidence: vec![Evidence::File { path: file }],
                manifest: Some(manifest),
                modern: modern_only,
            });
        }
    }
    // Registry-less modern installs still carry local installerdata.xml. This
    // bounded fallback needs neither the encrypted IS list nor an account API.
    for root in [KnownFolder::ProgramFiles, KnownFolder::ProgramFilesX86]
        .into_iter()
        .filter_map(|f| env.known_folder(f))
        .map(|p| p.join("EA Games"))
    {
        if let Ok(root) = env.resolve_path(&root)
            && let Ok(entries) = fs::read_dir(&root)
        {
            for entry in entries.take(1024).filter_map(Result::ok) {
                ctx.check()?;
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let Ok(dir) = env.resolve_path(&entry.path()) else {
                    continue;
                };
                if is_file(env, &dir.join("__Installer/installerdata.xml")) {
                    result.push(Candidate {
                        dir,
                        title: entry.file_name().to_string_lossy().into_owned(),
                        evidence: vec![],
                        manifest: None,
                        modern: true,
                    });
                }
            }
        }
    }
    Ok(result)
}
fn executable(env: &HostEnv, dir: &Path, metadata: &InstallerMetadata) -> Option<PathBuf> {
    let mut value = metadata.executable.as_deref()?.trim().trim_matches('"');
    for marker in ["__InstallLoc__", "[INSTALLDIR]", "${INSTALLDIR}"] {
        if value
            .get(..marker.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(marker))
        {
            value = &value[marker.len()..];
            break;
        }
    }
    let windows = value.as_bytes().get(1) == Some(&b':');
    let path = if windows {
        expand(env, value)?
    } else {
        let relative = PathBuf::from(value.trim_start_matches(['/', '\\']).replace('\\', "/"));
        if relative
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return None;
        }
        env.resolve_path(&dir.join(relative)).ok()?
    };
    if path
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("exe"))
    {
        return None;
    }
    let parent = crate::identity::canonical_path(env, dir, &GameRuntime::Native).ok()?;
    let target = crate::identity::canonical_path(env, &path, &GameRuntime::Native).ok()?;
    if !parent.contains(&target)
        || target.path.components().any(|p| {
            p.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("__Installer")
        })
    {
        return None;
    }
    Some(path)
}
fn game(env: &HostEnv, candidate: Candidate, ctx: &ScanCtx) -> Option<DetectedGame> {
    let file = candidate.dir.join("__Installer/installerdata.xml");
    let metadata = if is_file(env, &file) {
        match read(env, &file, MAX_FILE).and_then(|b| metadata::parse_installer_xml(&b)) {
            Ok(m) => m,
            Err(e) => {
                ctx.warn(format!("{}: {e}", file.display()));
                InstallerMetadata::default()
            }
        }
    } else {
        InstallerMetadata::default()
    };
    let mut offers = metadata.content_ids.clone();
    if let Some(manifest) = &candidate.manifest
        && !offers.contains(&manifest.offer)
    {
        offers.push(manifest.offer.clone());
    }
    let preferred = if candidate.modern {
        offers.first().cloned()
    } else {
        candidate
            .manifest
            .as_ref()
            .map(|m| m.offer.clone())
            .or_else(|| offers.first().cloned())
    }?;
    let present = is_dir(env, &candidate.dir);
    let mut game = DetectedGame::new(
        ID,
        &preferred,
        metadata.title.clone().unwrap_or(candidate.title),
    );
    game.ids = offers
        .into_iter()
        .map(|offer| ExternalId::Ea { offer })
        .collect();
    let mut link = Url::parse(if candidate.modern {
        "link2ea://launchgame/"
    } else {
        "origin2://game/launch"
    })
    .expect("static EA URI");
    if candidate.modern {
        link.path_segments_mut()
            .ok()?
            .pop_if_empty()
            .push(&preferred);
    } else {
        link.query_pairs_mut().append_pair("offerIds", &preferred);
    }
    game.launch = vec![LaunchTarget::Uri { uri: link.into() }];
    game.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    game.state = if !present {
        game.confidence = Confidence::Maybe;
        InstallState::Broken {
            reason: "EA install folder missing".into(),
        }
    } else {
        candidate
            .manifest
            .as_ref()
            .map(|m| m.state.clone())
            .unwrap_or(InstallState::Installed)
    };
    if game.state != InstallState::Installed {
        game.confidence = Confidence::Maybe;
    }
    game.size = candidate.manifest.and_then(|m| m.size);
    game.install_dir = Some(candidate.dir.clone());
    game.evidence = candidate.evidence;
    if is_file(env, &file) {
        game.evidence.push(Evidence::File { path: file });
    }
    game.tracking = Some(TrackingHint {
        steam_app_id: None,
        install_dir: game.install_dir.clone(),
        exe_names: executable(env, &candidate.dir, &metadata)
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .into_iter()
            .collect(),
    });
    Some(game)
}
impl Source for Ea {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }
    fn detect(&self, env: &HostEnv) -> Vec<LauncherStatus> {
        let clients = clients(env);
        if clients.is_empty() {
            return vec![LauncherStatus::not_found(ID)];
        }
        clients
            .into_iter()
            .map(|c| {
                LauncherStatus::found(ID, LauncherKind::Native, Some(c.path), origin_data(env))
            })
            .collect()
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games = BTreeMap::<PathBuf, DetectedGame>::new();
        for candidate in candidates(env, ctx)? {
            ctx.check()?;
            let Some(game) = game(env, candidate, ctx) else {
                continue;
            };
            let key = game.install_dir.clone().expect("EA game has a folder");
            if let Some(old) = games.get_mut(&key) {
                for id in game.ids {
                    if !old.ids.contains(&id) {
                        old.ids.push(id);
                    }
                }
                for target in game.launch {
                    if !old.launch.contains(&target) {
                        old.launch.push(target);
                    }
                }
                if game.state != InstallState::Installed {
                    old.state = game.state;
                    old.confidence = Confidence::Maybe;
                }
                if old.size.is_none() {
                    old.size = game.size;
                }
                for evidence in game.evidence {
                    if !old.evidence.contains(&evidence) {
                        old.evidence.push(evidence);
                    }
                }
            } else {
                games.insert(key, game);
            }
        }
        let mut games: Vec<_> = games.into_values().collect();
        games.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(games)
    }
    fn launcher_locations(&self, env: &HostEnv) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();
        for folder in [KnownFolder::ProgramFiles, KnownFolder::ProgramFilesX86]
            .into_iter()
            .filter_map(|f| env.known_folder(f))
        {
            paths.insert(folder.join("Electronic Arts/EA Desktop/EA Desktop/EADesktop.exe"));
            paths.insert(folder.join("Origin/Origin.exe"));
        }
        paths.into_iter().collect()
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        let mut paths = BTreeSet::new();
        if let Some(data) = origin_data(env) {
            paths.insert((data, true));
        }
        if let Ok(candidates) = publisher_candidates(env, &ScanCtx::default()) {
            for candidate in candidates {
                paths.insert((candidate.dir.join("__Installer/installerdata.xml"), false));
            }
        }
        paths
            .into_iter()
            .map(|(path, recursive)| WatchPath { path, recursive })
            .collect()
    }
}
