//! One module per source (§5, §6), plus host-path helpers they share.
//!
//! Paths are the ones `HostEnv` hands out: physical paths this process can
//! read (in fixture mode, rebased below the fixture root). A path that a
//! launcher file or the registry reports goes through `HostEnv::resolve_path`
//! before use.

#[cfg(feature = "sqlite")]
pub mod amazon;
pub mod battlenet;
pub mod bottles;
#[cfg(feature = "linux-sources")]
pub mod desktop;
pub mod ea;
pub mod epic;
#[cfg(feature = "linux-sources")]
pub mod faugus;
#[cfg(feature = "linux-sources")]
pub mod flatpak;
#[cfg(feature = "windows-sources")]
pub mod gameux;
pub mod gog;
pub mod heroic;
pub mod itch;
pub mod legendary;
#[cfg(feature = "linux-sources")]
pub mod lutris;
#[cfg(feature = "macos-sources")]
pub mod macos_apps;
#[cfg(feature = "linux-sources")]
pub mod minigalaxy;
pub mod prefixes;
pub mod riot;
pub mod rockstar;
#[cfg(feature = "linux-sources")]
pub mod snap;
#[cfg(feature = "windows-sources")]
pub mod startmenu;
pub mod steam;
pub mod ubisoft;
#[cfg(feature = "windows-sources")]
pub mod uninstall;
#[cfg(feature = "windows-sources")]
pub mod xbox;

use crate::{
    env::HostEnv,
    registry::{RegistryHive, RegistryKey, RegistryValue, RegistryView},
    types::{LaunchTarget, SourceError, UninstallInfo},
};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

/// Joins a relative path written with either separator.
pub(crate) fn join(base: &Path, relative: &str) -> PathBuf {
    relative
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// A path reported by a launcher file or the registry, made readable.
pub(crate) fn resolve(env: &HostEnv, reported: &str) -> Option<PathBuf> {
    env.resolve_path(Path::new(reported.trim())).ok()
}

/// A relative path from a launcher file, resolved under `root`. Absolute
/// paths, drive letters, `..` and anything that resolves outside `root` are
/// rejected.
pub(crate) fn relative(env: &HostEnv, root: &Path, text: &str) -> Option<PathBuf> {
    let text = text.replace('\\', "/");
    let path = Path::new(&text);
    if text.is_empty()
        || text.contains([':', '\0'])
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let path = env.resolve_path(&root.join(path)).ok()?;
    path.starts_with(root).then_some(path)
}

/// A launcher-reported path that may be quoted or hold `%VARS%`, made readable.
pub(crate) fn expand(env: &HostEnv, reported: &str) -> Option<PathBuf> {
    env.expand_path(reported.trim().trim_matches('"')).ok()
}

/// Reads a regular host file, failing when it is larger than `limit` bytes.
pub(crate) fn read(env: &HostEnv, path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let path = env.resolve_path(path)?;
    // Opening a named pipe can block even though its reported length is zero.
    if !fs::metadata(&path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("larger than {limit} bytes"),
        ));
    }
    Ok(bytes)
}

pub(crate) fn is_dir(env: &HostEnv, path: &Path) -> bool {
    env.resolve_path(path).is_ok_and(|path| path.is_dir())
}

pub(crate) fn is_file(env: &HostEnv, path: &Path) -> bool {
    env.resolve_path(path).is_ok_and(|path| path.is_file())
}

/// Sorted UTF-8 entry names of a host directory; others are skipped.
pub(crate) fn list(env: &HostEnv, path: &Path) -> io::Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(env.resolve_path(path)?)?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// [`list`], empty when the folder isn't there. A folder the OS won't read
/// (no access, a failing drive) is a warning on the scan as well, so it
/// doesn't pass for "no games" (#160). The sandbox's own refusals stay
/// quiet, as before.
pub(crate) fn list_or_warn(
    env: &HostEnv,
    path: &Path,
    ctx: Option<&crate::types::ScanCtx>,
) -> Vec<String> {
    match list(env, path) {
        Ok(names) => names,
        Err(error)
            if error.raw_os_error().is_some()
                && !matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
        {
            let warning = format!("can't read {}: {error}", path.display());
            match ctx {
                Some(ctx) => ctx.warn(warning),
                None => tracing::warn!(%warning, "discovery folder unreadable"),
            }
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

/// The physical, link-free location of a host path, for deduplication; the
/// path itself when it can't be resolved.
pub(crate) fn canonical(env: &HostEnv, path: &Path) -> PathBuf {
    env.resolve_path(path)
        .and_then(fs::canonicalize)
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Reads and parses a bounded JSON file.
pub(crate) fn json(
    env: &HostEnv,
    path: &Path,
    limit: u64,
) -> Result<serde_json::Value, SourceError> {
    let bytes = read(env, path, limit).map_err(|error| SourceError::io(path, error))?;
    serde_json::from_slice(&bytes).map_err(|error| SourceError::parse(path, error))
}

/// A trimmed, non-empty string field of a JSON object.
pub(crate) fn json_text<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// A file or folder name that cannot leave its directory.
pub(crate) fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// Both hives in both registry views, where launchers register their keys.
pub(crate) const REGISTRY_VIEWS: [(RegistryHive, RegistryView); 4] = [
    (RegistryHive::LocalMachine, RegistryView::Registry64),
    (RegistryHive::LocalMachine, RegistryView::Registry32),
    (RegistryHive::CurrentUser, RegistryView::Registry64),
    (RegistryHive::CurrentUser, RegistryView::Registry32),
];

/// A string (or expandable string) registry value, if present.
pub(crate) fn registry_string(
    env: &HostEnv,
    hive: RegistryHive,
    view: RegistryView,
    key: &str,
    name: &str,
) -> Option<String> {
    let registry = env.registry();
    let key = registry.open_key(hive, view, key).ok()??;
    match registry.value(&key, name).ok()?? {
        RegistryValue::String(text) | RegistryValue::ExpandString(text) => {
            Some(text).filter(|text| !text.trim().is_empty())
        }
        _ => None,
    }
}

/// Splits launch options on whitespace; double quotes group, `\"` escapes.
pub(crate) fn tokens(options: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = options.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            '\\' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => {
                quoted = !quoted;
                started = true;
            }
            char if char.is_whitespace() && !quoted => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            char => {
                current.push(char);
                started = true;
            }
        }
    }
    if started {
        tokens.push(current);
    }
    tokens
}

/// One `…\CurrentVersion\Uninstall\<name>` entry.
#[derive(Debug, Clone)]
pub(crate) struct UninstallEntry {
    /// `HKLM\…\Uninstall\<name>` as shown in evidence.
    pub key: String,
    pub display_name: Option<String>,
    pub install_location: Option<PathBuf>,
    pub uninstall: Option<String>,
    pub quiet_uninstall: Option<String>,
}

impl UninstallEntry {
    /// The entry's uninstall commands, when it has a usable one.
    pub(crate) fn info(&self, env: &HostEnv) -> Option<UninstallInfo> {
        Some(UninstallInfo {
            target: command_target(env, self.uninstall.as_deref()?)?,
            quiet: self
                .quiet_uninstall
                .as_deref()
                .and_then(|command| command_target(env, command)),
            registry_key: Some(self.key.clone()),
        })
    }
}

/// Uninstall entries of HKLM (both views) and HKCU.
pub(crate) fn uninstall_entries(env: &HostEnv) -> Vec<UninstallEntry> {
    let registry = env.registry();
    let mut entries = Vec::new();
    for (hive, view, path, label) in [
        (
            RegistryHive::LocalMachine,
            RegistryView::Registry64,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "HKLM",
        ),
        (
            RegistryHive::LocalMachine,
            RegistryView::Registry32,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "HKLM32",
        ),
        (
            RegistryHive::CurrentUser,
            RegistryView::Native,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            "HKCU",
        ),
    ] {
        let Ok(Some(key)) = registry.open_key(hive, view, path) else {
            continue;
        };
        for name in registry.subkeys(&key).unwrap_or_default() {
            let sub = RegistryKey::new(hive, view, format!(r"{path}\{name}"));
            let value = |field: &str| registry_string(env, sub.hive, sub.view, &sub.path, field);
            entries.push(UninstallEntry {
                key: format!(r"{label}\{}", sub.path),
                display_name: value("DisplayName"),
                install_location: value("InstallLocation").and_then(|path| resolve(env, &path)),
                uninstall: value("UninstallString"),
                quiet_uninstall: value("QuietUninstallString"),
            });
        }
    }
    entries
}

/// An uninstall command line as a launch target.
fn command_target(env: &HostEnv, command: &str) -> Option<LaunchTarget> {
    let mut parts = tokens(command).into_iter();
    let program = parts.next()?;
    if program.contains("://") {
        return Some(LaunchTarget::Uri { uri: program });
    }
    Some(LaunchTarget::Exec {
        program: resolve(env, &program).unwrap_or_else(|| PathBuf::from(&program)),
        args: parts.collect(),
        cwd: None,
        env: std::collections::BTreeMap::new(),
    })
}

/// Whether two physical paths name the same folder, ignoring case and a
/// trailing separator (Windows paths are case-insensitive).
pub(crate) fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |path: &Path| {
        path.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// Bounds for a macOS bundle's `Info.plist`.
pub(crate) const INFO_PLIST: crate::parsers::Limits = crate::parsers::Limits {
    max_input_bytes: 2 << 20,
    max_string_bytes: 65_536,
    max_nodes: 20_000,
    max_depth: 32,
};

/// A launcher executable from a macOS bundle's bounded Info.plist. Reading
/// metadata works on every host; the target must resolve inside its bundle.
pub(crate) fn macos_bundle_executable(env: &HostEnv, bundle: &Path) -> Option<PathBuf> {
    let bytes = read(
        env,
        &bundle.join("Contents/Info.plist"),
        INFO_PLIST.max_input_bytes as u64,
    )
    .ok()?;
    let value = crate::parsers::plist::parse(&bytes, &INFO_PLIST).ok()?;
    let name = value
        .as_dictionary()?
        .get("CFBundleExecutable")?
        .as_string()?;
    if !plain_name(name) {
        return None;
    }
    let executable = env
        .resolve_path(&bundle.join("Contents/MacOS").join(name))
        .ok()?;
    let bundle = crate::identity::canonical_path(env, bundle, &crate::GameRuntime::Native).ok()?;
    let target =
        crate::identity::canonical_path(env, &executable, &crate::GameRuntime::Native).ok()?;
    (bundle.contains(&target) && is_file(env, &executable)).then_some(executable)
}
