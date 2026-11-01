use super::HostEnv;
use crate::OperatingSystem;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum KnownFolder {
    Home,
    ConfigHome,
    DataHome,
    CacheHome,
    StateHome,
    Documents,
    Desktop,
    Downloads,
    ProgramData,
    AppData,
    LocalAppData,
    ProgramFiles,
    ProgramFilesX86,
    StartMenu,
    CommonStartMenu,
    UserLibrary,
    SystemLibrary,
    Applications,
    UserApplications,
}

impl HostEnv {
    pub(super) fn populate_folders(
        &mut self,
        overrides: &BTreeMap<KnownFolder, String>,
    ) -> io::Result<()> {
        let windows = self.os == OperatingSystem::Windows;
        let default_home = match self.os {
            OperatingSystem::Windows => r"C:\Users\fixture",
            OperatingSystem::Macos => "/Users/fixture",
            OperatingSystem::Linux => "/home/fixture",
        };
        let home = overrides
            .get(&KnownFolder::Home)
            .map(OsString::from)
            .or_else(|| self.var(if windows { "USERPROFILE" } else { "HOME" }))
            .unwrap_or_else(|| {
                if self.is_fixture() {
                    default_home.into()
                } else {
                    self.fs_root.as_os_str().to_owned()
                }
            });
        let join = |base: &OsStr, tail: &str| -> OsString {
            let mut joined = base.to_os_string();
            joined.push(if windows { "\\" } else { "/" });
            joined.push(if windows {
                tail.replace('/', "\\")
            } else {
                tail.into()
            });
            joined
        };
        let env_or = |name: &str, fallback: OsString| self.var(name).unwrap_or(fallback);
        let mut folders = BTreeMap::from([
            (KnownFolder::Home, home.clone()),
            (KnownFolder::Documents, join(&home, "Documents")),
            (KnownFolder::Desktop, join(&home, "Desktop")),
            (KnownFolder::Downloads, join(&home, "Downloads")),
        ]);
        let mut default_vars = vec![(if windows { "USERPROFILE" } else { "HOME" }, home.clone())];
        match self.os {
            OperatingSystem::Windows => {
                let drive = env_or("SystemDrive", "C:".into());
                let app = env_or("APPDATA", join(&home, "AppData/Roaming"));
                let local = env_or("LOCALAPPDATA", join(&home, "AppData/Local"));
                let common = env_or("ProgramData", join(&drive, "ProgramData"));
                let programs = env_or("ProgramFiles", join(&drive, "Program Files"));
                let programs_x86 = env_or("ProgramFiles(x86)", join(&drive, "Program Files (x86)"));
                folders.extend([
                    (KnownFolder::ConfigHome, app.clone()),
                    (KnownFolder::DataHome, local.clone()),
                    (KnownFolder::CacheHome, join(&local, "Cache")),
                    (KnownFolder::StateHome, local.clone()),
                    (KnownFolder::AppData, app.clone()),
                    (KnownFolder::LocalAppData, local.clone()),
                    (KnownFolder::ProgramData, common.clone()),
                    (KnownFolder::ProgramFiles, programs.clone()),
                    (KnownFolder::ProgramFilesX86, programs_x86.clone()),
                    (
                        KnownFolder::StartMenu,
                        join(&app, "Microsoft/Windows/Start Menu/Programs"),
                    ),
                    (
                        KnownFolder::CommonStartMenu,
                        join(&common, "Microsoft/Windows/Start Menu/Programs"),
                    ),
                ]);
                default_vars.extend([
                    ("SystemDrive", drive),
                    ("APPDATA", app),
                    ("LOCALAPPDATA", local),
                    ("ProgramData", common),
                    ("ProgramFiles", programs),
                    ("ProgramFiles(x86)", programs_x86),
                ]);
            }
            OperatingSystem::Linux | OperatingSystem::Macos => {
                let mac = self.os == OperatingSystem::Macos;
                let xdg = |name: &str, tail: &str| {
                    self.var(name)
                        .filter(|value| value.to_str().is_some_and(|v| v.starts_with('/')))
                        .unwrap_or_else(|| join(&home, tail))
                };
                let config = xdg(
                    "XDG_CONFIG_HOME",
                    if mac {
                        "Library/Application Support"
                    } else {
                        ".config"
                    },
                );
                let data = xdg(
                    "XDG_DATA_HOME",
                    if mac {
                        "Library/Application Support"
                    } else {
                        ".local/share"
                    },
                );
                let cache = xdg(
                    "XDG_CACHE_HOME",
                    if mac { "Library/Caches" } else { ".cache" },
                );
                let state = xdg("XDG_STATE_HOME", ".local/state");
                folders.extend([
                    (KnownFolder::ConfigHome, config.clone()),
                    (KnownFolder::DataHome, data.clone()),
                    (KnownFolder::CacheHome, cache.clone()),
                    (KnownFolder::StateHome, state.clone()),
                ]);
                default_vars.extend([
                    ("XDG_CONFIG_HOME", config),
                    ("XDG_DATA_HOME", data),
                    ("XDG_CACHE_HOME", cache),
                    ("XDG_STATE_HOME", state),
                ]);
                if mac {
                    folders.extend([
                        (KnownFolder::UserLibrary, join(&home, "Library")),
                        (KnownFolder::SystemLibrary, "/Library".into()),
                        (KnownFolder::Applications, "/Applications".into()),
                        (KnownFolder::UserApplications, join(&home, "Applications")),
                    ]);
                }
            }
        }
        for (key, value) in default_vars {
            if self.var(key).is_none() {
                self.vars.insert(key.into(), value);
            }
        }
        for (folder, value) in overrides {
            folders.insert(*folder, value.into());
        }
        // Resolve after adding defaults: explicit known-folder overrides may use
        // %USERPROFILE%, $HOME or another captured variable.
        self.folders = folders
            .into_iter()
            .map(|(folder, path)| {
                let path = match path.to_str() {
                    Some(text) => self.expand_path(text)?,
                    None => self.resolve_path(Path::new(&path))?,
                };
                Ok((folder, path))
            })
            .collect::<io::Result<_>>()?;
        let raw = self.var("XDG_DATA_DIRS").unwrap_or_else(|| {
            if windows {
                OsString::new()
            } else {
                "/usr/local/share:/usr/share".into()
            }
        });
        // The fixture OS determines separators, not the OS running the test.
        self.xdg_data_dirs = raw
            .to_str()
            .unwrap_or_default()
            .split(if windows { ';' } else { ':' })
            .filter(|part| !part.is_empty() && (part.starts_with('/') || windows))
            .map(|part| self.expand_path(part))
            .collect::<io::Result<Vec<PathBuf>>>()?;
        if !windows && self.var("XDG_DATA_DIRS").is_none() {
            self.vars.insert("XDG_DATA_DIRS".into(), raw);
        }
        Ok(())
    }
}
