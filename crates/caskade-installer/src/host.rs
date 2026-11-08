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
                    [
                        "apt",
                        "dnf",
                        "zypper",
                        "dpkg-query",
                        "rpm",
                        "pkexec",
                        "flatpak",
                    ]
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
                found.push(dunce::canonicalize(&path).unwrap_or(path));
            }
        }
    }
    // Best first (see `proton_rank`); PROTONPATH above always wins.
    found.sort_by_cached_key(|path| proton_rank(path));
    for build in found {
        if !builds.contains(&build) {
            builds.push(build);
        }
    }
    builds
}

/// A Proton build's family, in the order an automatic choice prefers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProtonFamily {
    /// umu's own build: its default and best tested with it.
    Umu,
    GeProton,
    /// Valve's numbered releases (`Proton 10.0`).
    SteamStable,
    SteamExperimental,
    /// Other compatibility tools (Proton-tkg, CachyOS and the like).
    Other,
    Beta,
    /// Valve's builds for the particular games it assigns them to.
    Hotfix,
}

fn proton_family(name: &str) -> ProtonFamily {
    let name = name.to_ascii_lowercase();
    if name.contains("hotfix") {
        ProtonFamily::Hotfix
    } else if name.starts_with("umu-proton") {
        ProtonFamily::Umu
    } else if name.starts_with("ge-proton")
        || name.starts_with("ge_proton")
        || name.starts_with("proton-ge")
    {
        ProtonFamily::GeProton
    } else if name.contains("beta") {
        ProtonFamily::Beta
    } else if name.contains("experimental") {
        ProtonFamily::SteamExperimental
    } else if name
        .strip_prefix("proton ")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    {
        ProtonFamily::SteamStable
    } else {
        ProtonFamily::Other
    }
}

/// The numbers in a build's name, so `GE-Proton10-34` is newer than
/// `GE-Proton9-27` (a text comparison would say otherwise).
fn proton_version(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|digits| digits.parse().ok())
        .collect()
}

/// Sorts Proton builds best first: by family, then newest version.
fn proton_rank(path: &Path) -> (ProtonFamily, std::cmp::Reverse<Vec<u32>>, String) {
    let name = proton_build_name(path);
    (
        proton_family(&name),
        std::cmp::Reverse(proton_version(&name)),
        name,
    )
}

/// A Proton build's name: its folder's, as Steam and umu show it.
pub fn proton_build_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The build an automatic choice runs from `builds`, which are best first
/// (as `HostCaps::proton_builds`): the first that isn't a Hotfix, which is
/// only for the games Valve assigns it to.
pub fn automatic_proton_build(builds: &[PathBuf]) -> Option<&PathBuf> {
    builds
        .iter()
        .find(|path| proton_family(&proton_build_name(path)) != ProtonFamily::Hotfix)
}

/// `PROTONPATH` for an automatic choice from `builds`: the
/// [automatic build](automatic_proton_build), except that an umu-managed
/// UMU-Proton is named `UMU-Proton` so umu keeps it current. `None` when
/// none is installed.
pub fn automatic_proton(builds: &[PathBuf]) -> Option<String> {
    let build = automatic_proton_build(builds)?;
    Some(
        if proton_family(&proton_build_name(build)) == ProtonFamily::Umu {
            UMU_PROTON.into()
        } else {
            build.to_string_lossy().into_owned()
        },
    )
}

/// `PROTONPATH` that has umu download UMU-Proton and keep it current.
pub const UMU_PROTON: &str = "UMU-Proton";

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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(mut builds: Vec<PathBuf>) -> Vec<String> {
        builds.sort_by_cached_key(|path| proton_rank(path));
        builds.iter().map(|path| proton_build_name(path)).collect()
    }

    #[test]
    fn proton_builds_rank_by_family_then_newest_version() {
        let common = Path::new("/steam/steamapps/common");
        let tools = Path::new("/steam/compatibilitytools.d");
        let builds = vec![
            common.join("Proton Hotfix"),
            common.join("Proton 9.0 (Beta)"),
            common.join("Proton 10.0"),
            common.join("Proton 8.0"),
            common.join("Proton - Experimental"),
            tools.join("GE-Proton9-27"),
            tools.join("GE-Proton10-34"),
            tools.join("proton-cachyos"),
            tools.join("UMU-Proton-10.0-4"),
            tools.join("UMU-Proton-9.0-3"),
        ];
        assert_eq!(
            names(builds),
            [
                "UMU-Proton-10.0-4",
                "UMU-Proton-9.0-3",
                "GE-Proton10-34",
                "GE-Proton9-27",
                "Proton 10.0",
                "Proton 8.0",
                "Proton - Experimental",
                "proton-cachyos",
                "Proton 9.0 (Beta)",
                "Proton Hotfix",
            ]
        );
    }

    #[test]
    fn automatic_proton_skips_hotfix_and_lets_umu_keep_its_own_current() {
        let common = Path::new("/steam/steamapps/common");
        let tools = Path::new("/steam/compatibilitytools.d");
        assert_eq!(automatic_proton(&[]), None);
        assert_eq!(automatic_proton(&[common.join("Proton Hotfix")]), None);
        assert_eq!(
            automatic_proton(&[common.join("Proton Hotfix"), common.join("Proton 10.0")]),
            // The path as joined here: a backslash on Windows.
            Some(common.join("Proton 10.0").to_string_lossy().into_owned())
        );
        assert_eq!(
            automatic_proton(&[
                tools.join("UMU-Proton-10.0-4"),
                tools.join("GE-Proton10-34")
            ]),
            Some(UMU_PROTON.into())
        );
        assert_eq!(
            automatic_proton_build(&[tools.join("UMU-Proton-10.0-4")]),
            Some(&tools.join("UMU-Proton-10.0-4"))
        );
    }
}
