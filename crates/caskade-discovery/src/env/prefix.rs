//! The Windows view of a Wine-family prefix (#91): Windows sources run
//! against it unchanged. `C:` is `drive_c`, other letters follow the
//! prefix's `dosdevices` links (resolved through the parent env, so fixture
//! confinement holds), the registry is the prefix's `.reg` files, and the
//! known folders are the prefix user's.

use super::{HostEnv, KnownFolder};
use crate::{
    OperatingSystem,
    registry::{Limits, WineRegistry},
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl HostEnv {
    /// A Windows env for the prefix at `prefix` (a physical path this env
    /// handed out). The clock, drives and fixture root are shared.
    pub fn wine_prefix(&self, prefix: &Path) -> io::Result<HostEnv> {
        let prefix = self.resolve_path(prefix)?;
        let drive_c = prefix.join("drive_c");
        if !drive_c.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "prefix has no drive_c",
            ));
        }
        let mut drives = BTreeMap::from([('c', drive_c.clone())]);
        for (letter, target) in dos_devices(&prefix) {
            if letter == 'c' {
                continue;
            }
            // Absolute targets are host paths (Z: → /); relative ones are
            // relative to dosdevices.
            let target = if target.is_absolute() {
                self.resolve_path(&target)
            } else {
                Ok(prefix.join("dosdevices").join(target))
            };
            if let Ok(target) = target
                && self.confined(&target)
            {
                drives.insert(letter, target);
            }
        }
        let user = prefix_user(&drive_c);
        let profile = drive_c.join("users").join(&user);
        let (roaming, local) =
            if profile.join("AppData").is_dir() || !profile.join("Application Data").is_dir() {
                (
                    profile.join("AppData/Roaming"),
                    profile.join("AppData/Local"),
                )
            } else {
                (
                    profile.join("Application Data"),
                    profile.join("Local Settings/Application Data"),
                )
            };
        let program_data = if drive_c.join("ProgramData").is_dir() {
            drive_c.join("ProgramData")
        } else {
            drive_c.join("users/Public")
        };
        let start_menu = |base: &Path| base.join("Microsoft/Windows/Start Menu/Programs");
        let folders = BTreeMap::from([
            (KnownFolder::Home, profile.clone()),
            (KnownFolder::Documents, profile.join("Documents")),
            (KnownFolder::Desktop, profile.join("Desktop")),
            (KnownFolder::Downloads, profile.join("Downloads")),
            (KnownFolder::ConfigHome, roaming.clone()),
            (KnownFolder::DataHome, local.clone()),
            (KnownFolder::CacheHome, local.join("Cache")),
            (KnownFolder::StateHome, local.clone()),
            (KnownFolder::AppData, roaming.clone()),
            (KnownFolder::LocalAppData, local.clone()),
            (KnownFolder::ProgramData, program_data.clone()),
            (KnownFolder::ProgramFiles, drive_c.join("Program Files")),
            (
                KnownFolder::ProgramFilesX86,
                drive_c.join("Program Files (x86)"),
            ),
            (KnownFolder::StartMenu, start_menu(&roaming)),
            (KnownFolder::CommonStartMenu, start_menu(&program_data)),
        ]);
        let win = |path: &str| OsString::from(path.to_owned());
        let vars = BTreeMap::from([
            (win("USERPROFILE"), win(&format!(r"C:\users\{user}"))),
            (win("USERNAME"), win(&user)),
            (
                win("APPDATA"),
                win(&format!(r"C:\users\{user}\AppData\Roaming")),
            ),
            (
                win("LOCALAPPDATA"),
                win(&format!(r"C:\users\{user}\AppData\Local")),
            ),
            (win("ProgramData"), win(r"C:\ProgramData")),
            (win("ProgramFiles"), win(r"C:\Program Files")),
            (win("ProgramFiles(x86)"), win(r"C:\Program Files (x86)")),
            (win("SystemDrive"), win("C:")),
            (win("SystemRoot"), win(r"C:\windows")),
            (win("WINEPREFIX"), prefix.clone().into_os_string()),
        ]);
        let registry = WineRegistry::from_prefix(&prefix, Limits::default())?;
        let mut env = self.clone();
        env.os = OperatingSystem::Windows;
        env.vars = vars;
        env.folders = folders;
        env.xdg_data_dirs = Vec::new();
        env.registry = Arc::new(registry);
        env.wine_drives = Some(Arc::new(drives));
        Ok(env)
    }

    /// The prefix's drive map, when this env is a prefix view.
    pub fn wine_drives(&self) -> Option<&BTreeMap<char, PathBuf>> {
        self.wine_drives.as_deref()
    }

    /// In fixture mode, `path` must stay below the fixture root.
    fn confined(&self, path: &Path) -> bool {
        if !self.is_fixture() {
            return true;
        }
        let root = self
            .fs_root
            .canonicalize()
            .unwrap_or_else(|_| self.fs_root.clone());
        path.canonicalize()
            .map(|path| path.starts_with(&root))
            .unwrap_or_else(|_| path.starts_with(&self.fs_root))
    }

    /// `X:\a\b` through the prefix drive map; each component matches without
    /// regard to case, as Windows programs expect.
    pub(super) fn map_prefix_path(
        &self,
        drives: &BTreeMap<char, PathBuf>,
        text: &str,
    ) -> io::Result<PathBuf> {
        let bytes = text.as_bytes();
        let (base, tail) = if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
        {
            if text.len() > 2 && bytes[2] != b'\\' {
                return Err(invalid("drive-relative paths are ambiguous"));
            }
            let letter = (bytes[0] as char).to_ascii_lowercase();
            let base = drives.get(&letter).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "drive is not mapped in this prefix",
                )
            })?;
            (base.clone(), &text[2..])
        } else if let Some(tail) = text.strip_prefix('\\') {
            if tail.starts_with('\\') {
                return Err(invalid("UNC paths are not mapped in a prefix"));
            }
            (
                drives
                    .get(&'c')
                    .cloned()
                    .ok_or_else(|| invalid("no C: drive"))?,
                tail,
            )
        } else {
            return Err(invalid("relative Windows path"));
        };
        let mut path = base;
        let mut depth = 0usize;
        for part in tail.split('\\') {
            match part {
                "" | "." => {}
                ".." => {
                    if depth == 0 {
                        return Err(invalid("path escapes its drive"));
                    }
                    depth -= 1;
                    path.pop();
                }
                part if part.contains(':') => {
                    return Err(invalid("alternate streams are not paths"));
                }
                part => {
                    depth += 1;
                    path = case_insensitive_child(&path, part);
                }
            }
        }
        if !self.confined(&path) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes fixture root",
            ));
        }
        Ok(path)
    }
}

/// `dir/name`, or the existing entry whose name differs only in case.
fn case_insensitive_child(dir: &Path, name: &str) -> PathBuf {
    let exact = dir.join(name);
    if exact.exists() {
        return exact;
    }
    fs::read_dir(dir)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .filter(|candidate| {
                    candidate
                        .to_str()
                        .is_some_and(|text| text.eq_ignore_ascii_case(name))
                })
                .min()
        })
        .map_or(exact, |found| dir.join(found))
}

/// `dosdevices/<x>:` links (raw `<x>::` devices are skipped).
fn dos_devices(prefix: &Path) -> Vec<(char, PathBuf)> {
    let Ok(entries) = fs::read_dir(prefix.join("dosdevices")) else {
        return Vec::new();
    };
    let mut devices: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?.to_ascii_lowercase();
            let mut chars = name.chars();
            let (letter, colon) = (chars.next()?, chars.next()?);
            if !letter.is_ascii_alphabetic() || colon != ':' || chars.next().is_some() {
                return None;
            }
            Some((letter, fs::read_link(entry.path()).ok()?))
        })
        .collect();
    devices.sort();
    devices
}

/// The prefix's user: `steamuser` (Proton), else the single real user
/// under `drive_c/users`, else the first; `Public`-style dirs never count.
fn prefix_user(drive_c: &Path) -> String {
    let mut users: Vec<String> = fs::read_dir(drive_c.join("users"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            !matches!(
                name.to_ascii_lowercase().as_str(),
                "public" | "default" | "all users" | "default user"
            )
        })
        .collect();
    users.sort();
    if users.iter().any(|user| user == "steamuser") {
        return "steamuser".into();
    }
    users
        .into_iter()
        .next()
        .unwrap_or_else(|| "steamuser".into())
}
