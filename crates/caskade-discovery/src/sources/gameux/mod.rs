//! Windows Game Explorer registrations (#81), through the read-only registry
//! seam. GDF DLLs are metadata references: they are never loaded or executed.
//! Original code. Format references (no implementation code copied):
//! https://learn.microsoft.com/en-us/windows/win32/dxtecharts/windows-game-explorer-integration
//! https://github.com/wine-mirror/wine/tree/master/dlls/gameux
mod tasks;
use crate::{
    DetectedGame, Evidence, GameRuntime, HostEnv, InstallState, LaunchTarget, LauncherStatus,
    OsSet, ScanCtx, Source, SourceError, SourceId, TrackingHint, WatchPath,
    env::KnownFolder,
    registry::{RegistryEntry, RegistryHive, RegistryKey, RegistryValue, RegistryView},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
pub const ID: SourceId = SourceId::new("gameux");
pub const REGISTRY_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\GameUX";
const MAX_KEYS: usize = 4096;
#[derive(Debug, Default)]
pub struct GameUx;
impl Source for GameUx {
    fn id(&self) -> SourceId {
        ID
    }
    fn hosts(&self) -> OsSet {
        OsSet::WINDOWS
    }
    fn detect(&self, _: &HostEnv) -> Vec<LauncherStatus> {
        vec![]
    }
    fn watch_paths(&self, env: &HostEnv) -> Vec<WatchPath> {
        [KnownFolder::ProgramData, KnownFolder::LocalAppData]
            .into_iter()
            .filter_map(|folder| env.known_folder(folder))
            .map(|folder| WatchPath {
                path: folder.join("Microsoft/Windows/GameExplorer"),
                recursive: true,
            })
            .collect()
    }
    fn scan(&self, env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<DetectedGame>, SourceError> {
        let mut games = vec![];
        for registration in registrations(env, ctx)? {
            ctx.check()?;
            let game = game(env, ctx, &registration);
            ctx.check()?;
            if let Some(game) = game {
                games.push(game)
            }
        }
        Ok(games)
    }
}
struct Registration {
    key: RegistryKey,
    guid: String,
    values: Vec<RegistryEntry>,
}
fn hive(hive: RegistryHive) -> &'static str {
    match hive {
        RegistryHive::LocalMachine => "HKLM",
        RegistryHive::CurrentUser => "HKCU",
        RegistryHive::Users => "HKU",
        _ => "HK?",
    }
}
fn label(key: &RegistryKey) -> String {
    format!("{}:{:?}\\{}", hive(key.hive), key.view, key.path)
}
fn registrations(env: &HostEnv, ctx: &ScanCtx) -> Result<Vec<Registration>, SourceError> {
    ctx.check()?;
    let mut roots = vec![];
    let mut visited = 0usize;
    for view in [RegistryView::Registry64, RegistryView::Registry32] {
        roots.push(RegistryKey::new(
            RegistryHive::LocalMachine,
            view,
            REGISTRY_PATH,
        ));
        roots.push(RegistryKey::new(
            RegistryHive::CurrentUser,
            view,
            REGISTRY_PATH,
        ));
        let users = RegistryKey::new(RegistryHive::Users, view, "");
        for sid in children(env, ctx, &users, &mut visited)? {
            if is_sid(&sid) {
                roots.push(RegistryKey::new(
                    RegistryHive::Users,
                    view,
                    format!("{sid}\\{REGISTRY_PATH}"),
                ))
            }
        }
    }
    let mut records = vec![];
    let mut output_bytes = 0usize;
    let mut seen = BTreeSet::new();
    for root in roots {
        let mut stack = vec![(root, 0usize)];
        while let Some((key, depth)) = stack.pop() {
            ctx.check()?;
            if !seen.insert(label(&key).to_lowercase()) {
                continue;
            }
            let leaf = key.path.rsplit('\\').next().unwrap_or_default();
            if let Some(guid) = guid(leaf) {
                match env.registry().values(&key) {
                    Ok(values) if values.len() <= 128 => {
                        let values:Vec<_>=values.into_iter().filter(|entry| {
                            matches!(&entry.value,RegistryValue::String(s)|RegistryValue::ExpandString(s) if s.len()<=32768)
                            && matches!(entry.name.to_ascii_lowercase().as_str(),"title"|"displayname"|"name"|"configapplicationpath"|"gameinstallpath"|"installlocation"|"configgdfbinarypath"|"gdfbinarypath"|"appexepath"|"executable"|"exepath")
                        }).collect();
                        for entry in &values {
                            if let RegistryValue::String(text) | RegistryValue::ExpandString(text) =
                                &entry.value
                            {
                                output_bytes = output_bytes
                                    .saturating_add(text.len())
                                    .saturating_add(entry.name.len());
                            }
                        }
                        if output_bytes > 16 * 1024 * 1024 {
                            return Err(SourceError::Other {
                                message: "GameUX metadata byte budget exceeded".into(),
                            });
                        }
                        records.push(Registration { key, guid, values })
                    }
                    Ok(_) => ctx.warn(format!("{}: GameUX value count exceeds 128", label(&key))),
                    Err(error) => ctx.warn(format!("{}: {error}", label(&key))),
                }
                continue;
            }
            if depth >= 3 {
                continue;
            }
            let mut names = children(env, ctx, &key, &mut visited)?;
            names.sort_by_key(|s| s.to_lowercase());
            for name in names.into_iter().rev() {
                if guid(&name).is_none() && !name.eq_ignore_ascii_case("Games") && !is_sid(&name) {
                    continue;
                }
                stack.push((
                    RegistryKey::new(key.hive, key.view, format!("{}\\{name}", key.path)),
                    depth + 1,
                ));
            }
        }
    }
    records.sort_by_key(|r| label(&r.key));
    Ok(records)
}
fn children(
    env: &HostEnv,
    ctx: &ScanCtx,
    key: &RegistryKey,
    count: &mut usize,
) -> Result<Vec<String>, SourceError> {
    ctx.check()?;
    let names = match env.registry().subkeys(key) {
        Ok(names) => names,
        Err(error) => {
            ctx.warn(format!("{}: {error}", label(key)));
            return Ok(vec![]);
        }
    };
    *count = count.saturating_add(names.len());
    if *count > MAX_KEYS {
        return Err(SourceError::Other {
            message: "GameUX registry key budget exceeded".into(),
        });
    }
    Ok(names)
}
fn is_sid(sid: &str) -> bool {
    sid.len() <= 184
        && sid.get(..4).is_some_and(|s| s.eq_ignore_ascii_case("S-1-"))
        && sid[4..]
            .split('-')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn guid(value: &str) -> Option<String> {
    let value = value
        .strip_prefix('{')
        .and_then(|v| v.strip_suffix('}'))
        .unwrap_or(value);
    (value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        }))
    .then(|| format!("{{{}}}", value.to_ascii_lowercase()))
}
fn string<'a>(
    values: &'a BTreeMap<String, &RegistryValue>,
    names: &[&'static str],
) -> Option<(&'a str, bool, &'static str)> {
    for name in names {
        if let Some(value) = values.get(&name.to_lowercase()) {
            let (text, expand) = match value {
                RegistryValue::String(text) => (text.as_str(), false),
                RegistryValue::ExpandString(text) => (text.as_str(), true),
                _ => continue,
            };
            if !text.is_empty() && text.len() <= 32768 && !text.chars().any(char::is_control) {
                // The selected field name is taken from one of the static
                // schema arrays below; the source only uses it for evidence.
                return Some((text, expand, *name));
            }
        }
    }
    None
}
pub(super) fn path(
    env: &HostEnv,
    text: &str,
    expand: bool,
    base: Option<&Path>,
) -> Option<PathBuf> {
    let text = text.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(text);
    if text.is_empty() || text.contains('"') || text.chars().any(char::is_control) {
        return None;
    }
    let value = if expand {
        env.expand_vars(text)
    } else {
        text.into()
    };
    let text = value.to_str()?;
    let windows = text.as_bytes().get(1) == Some(&b':')
        && text.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if windows || text.starts_with(['/', '\\']) || Path::new(text).is_absolute() {
        return env.resolve_path(Path::new(text)).ok();
    }
    if text.split(['/', '\\']).any(|p| p == "..") {
        return None;
    }
    env.resolve_path(&super::join(base?, text)).ok()
}
fn game(env: &HostEnv, ctx: &ScanCtx, registration: &Registration) -> Option<DetectedGame> {
    let values: BTreeMap<_, _> = registration
        .values
        .iter()
        .map(|entry| (entry.name.to_lowercase(), &entry.value))
        .collect();
    let Some((title, _, title_field)) = string(&values, &["Title", "DisplayName", "Name"]) else {
        ctx.warn(format!(
            "{}: missing GameUX title",
            label(&registration.key)
        ));
        return None;
    };
    let dir = string(
        &values,
        &[
            "ConfigApplicationPath",
            "GameInstallPath",
            "InstallLocation",
        ],
    )
    .and_then(|(text, expand, _)| path(env, text, expand, None));
    let gdf = string(&values, &["ConfigGDFBinaryPath", "GDFBinaryPath"])
        .and_then(|(text, expand, _)| path(env, text, expand, dir.as_deref()));
    let exe = string(&values, &["AppExePath", "Executable", "ExePath"])
        .and_then(|(text, expand, _)| path(env, text, expand, dir.as_deref()))
        .filter(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("exe")));
    let mut evidence = vec![Evidence::Registry {
        key: format!("{}\\{}", hive(registration.key.hive), registration.key.path),
        value: Some(title_field.into()),
    }];
    if let Some(gdf) = &gdf {
        evidence.push(Evidence::File { path: gdf.clone() });
    }
    let launch = exe
        .map(|program| LaunchTarget::Exec {
            program,
            args: vec![],
            cwd: dir.clone(),
            env: BTreeMap::new(),
        })
        .or_else(|| tasks::primary(env, ctx, &registration.guid, dir.as_deref(), &mut evidence))
        .or_else(|| {
            gdf.as_ref()
                .filter(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("exe")))
                .map(|program| LaunchTarget::Exec {
                    program: program.clone(),
                    args: vec![],
                    cwd: dir.clone(),
                    env: BTreeMap::new(),
                })
        });
    let executable = match &launch {
        Some(LaunchTarget::Exec { program, .. }) => Some(program.clone()),
        _ => None,
    };
    let dir = dir.or_else(|| {
        executable
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
    });
    let reason = if dir.as_ref().is_none_or(|p| !p.is_dir()) {
        Some("GameUX install directory is missing")
    } else if executable.as_ref().is_none_or(|p| !p.is_file()) {
        Some("GameUX executable or primary play task is missing")
    } else {
        None
    };
    let mut game = DetectedGame::new(ID, label(&registration.key).to_lowercase(), title);
    game.runtime = GameRuntime::Windows {
        prefix: None,
        runner: None,
    };
    game.install_dir = dir;
    game.launch = launch.into_iter().collect();
    game.evidence = evidence;
    if let Some(reason) = reason {
        game.state = InstallState::Broken {
            reason: reason.into(),
        };
    }
    game.tracking = Some(TrackingHint {
        install_dir: game.install_dir.clone(),
        exe_names: executable
            .and_then(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_owned))
            .into_iter()
            .collect(),
        ..TrackingHint::default()
    });
    Some(game)
}
