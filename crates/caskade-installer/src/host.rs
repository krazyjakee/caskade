//! Injected host capabilities. Only `HostCaps::detect` inspects the real host;
//! analysis accepts a supplied profile and never calls detection itself.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::OperatingSystem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct FileSystemCaps {
    pub writable: bool,
    pub exec_bits: bool,
    pub symlinks: bool,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Native,
    Proton,
    Wine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RunnerInfo {
    pub kind: RunnerKind,
    pub program: PathBuf,
}

/// Injected package inventory and host tools. Probe/plan never query the OS.
/// `root` is set explicitly by a trusted boundary (e.g. a container test);
/// ordinary delegated installs use pkexec for the OS authentication prompt.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct PackageCapabilities {
    pub installed: BTreeSet<String>,
    pub managers: BTreeMap<String, PathBuf>,
    pub root: bool,
}
impl PackageCapabilities {
    pub fn is_empty(&self) -> bool {
        self.installed.is_empty() && self.managers.is_empty() && !self.root
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct HostCaps {
    pub os: OperatingSystem,
    pub arch: String,
    pub runners: Vec<RunnerInfo>,
    pub seven_zip: Option<PathBuf>,
    pub proton_builds: Vec<PathBuf>,
    pub filesystem: FileSystemCaps,
    #[serde(default, skip_serializing_if = "PackageCapabilities::is_empty")]
    pub packages: PackageCapabilities,
}

impl HostCaps {
    /// Best-effort detection at the CLI/app boundary. Filesystem flags are
    /// conservative OS defaults, not a capability test of an installation root;
    /// replace them with that root's measured flags before planning a job.
    pub fn detect() -> Self {
        let os = if cfg!(target_os = "windows") {
            OperatingSystem::Windows
        } else if cfg!(target_os = "macos") {
            OperatingSystem::Macos
        } else {
            OperatingSystem::Linux
        };
        let mut runners = vec![RunnerInfo {
            kind: RunnerKind::Native,
            program: PathBuf::new(),
        }];
        if os == OperatingSystem::Linux
            && let Some(program) = find_tool("umu-run").or_else(|| {
                crate::runtime::umu::UmuBootstrap::for_host().and_then(|umu| umu.installed())
            })
        {
            runners.push(RunnerInfo {
                kind: RunnerKind::Proton,
                program,
            });
        }
        if os != OperatingSystem::Windows
            && let Some(program) = find_tool("wine")
        {
            runners.push(RunnerInfo {
                kind: RunnerKind::Wine,
                program,
            });
        }
        let proton_builds = if os == OperatingSystem::Linux {
            proton_builds()
        } else {
            vec![]
        };
        Self {
            os,
            arch: std::env::consts::ARCH.into(),
            runners,
            seven_zip: find_tool("7zz").or_else(|| find_tool("7z")),
            proton_builds,
            filesystem: FileSystemCaps {
                writable: false,
                exec_bits: cfg!(unix),
                symlinks: cfg!(unix),
                case_sensitive: os == OperatingSystem::Linux,
            },
            packages: PackageCapabilities {
                managers: if os == OperatingSystem::Linux {
                    ["apt", "dnf", "zypper", "dpkg-query", "rpm", "pkexec", "flatpak"]
                        .into_iter()
                        .filter_map(|name| find_tool(name).map(|path| (name.into(), path)))
                        .collect()
                } else if os == OperatingSystem::Macos {
                    // /usr/sbin is often absent from a GUI app's PATH.
                    // osascript provides the OS authentication prompt for
                    // non-root package delegates.
                    ["installer", "pkgutil", "osascript", "hdiutil"]
                        .into_iter()
                        .filter_map(|name| {
                            let path = ["/usr/sbin", "/usr/bin"]
                                .into_iter()
                                .map(|directory| Path::new(directory).join(name))
                                .find(|path| path.is_file())
                                .or_else(|| find_tool(name));
                            path.map(|path| (name.into(), path))
                        })
                        .collect()
                } else {
                    BTreeMap::new()
                },
                ..PackageCapabilities::default()
            },
        }
    }

    pub fn has_runner(&self, kind: RunnerKind) -> bool {
        self.runners.iter().any(|runner| runner.kind == kind)
    }
}

/// `PROTONPATH`, then Proton builds in Steam's compatibility-tool folders
/// (GE-Proton, UMU-Proton; native, flatpak and system Steam) and Steam's
/// own `steamapps/common/Proton*`. A build is a folder with a `proton` script.
fn proton_builds() -> Vec<PathBuf> {
    let mut builds: Vec<PathBuf> = std::env::var_os("PROTONPATH")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .into_iter()
        .collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut tool_dirs = vec![PathBuf::from("/usr/share/steam/compatibilitytools.d")];
    let mut common = vec![];
    if let Some(home) = &home {
        for steam in [
            home.join(".local/share/Steam"),
            home.join(".steam/root"),
            home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        ] {
            tool_dirs.push(steam.join("compatibilitytools.d"));
            common.push(steam.join("steamapps/common"));
        }
    }
    let mut found = vec![];
    for directory in tool_dirs.iter().chain(&common) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if (tool_dirs.contains(directory) || name.starts_with("proton"))
                && path.join("proton").is_file()
            {
                found.push(path.canonicalize().unwrap_or(path));
            }
        }
    }
    // umu-managed UMU-Proton first (umu's default and best tested), then
    // newest-looking names; PROTONPATH above always wins.
    found.sort_by_key(|path| {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (!name.starts_with("umu-proton"), std::cmp::Reverse(name))
    });
    for build in found {
        if !builds.contains(&build) {
            builds.push(build);
        }
    }
    builds
}

fn find_tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = directory.join(format!("{name}.exe"));
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
