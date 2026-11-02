//! Tool definitions (`crates/caskade-sources/tools/*.toml`): where a helper
//! program installs from on each OS and machine, where Caskade keeps it,
//! and how to find it. Their own schema: emulator definitions are
//! `deny_unknown_fields` and carry profiles a tool has no use for.

use crate::{
    Error, Result,
    emulation::{Detect, InstallSource, OperatingSystem},
};
use serde::Deserialize;
use std::path::{Component, Path};

/// A helper program Caskade manages.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    /// `sunshine`, `moonlight`.
    pub id: String,
    /// The upstream program's name, for logs. Never shown to people
    /// ([`super::tool_label`] is).
    pub name: String,
    /// Where it goes under Caskade's data folder (`streaming/host`).
    pub dir: String,
    /// How to find it, on each OS: in its own folder after an install.
    #[serde(default)]
    pub detect: Detect,
    /// An AppImage release is unpacked and its program run directly: no
    /// FUSE, and none of the AppImage's own start-up script.
    #[serde(default)]
    pub extract: bool,
    #[serde(default)]
    pub install: ToolInstall,
}

/// Where a tool installs from, per OS: the first source whose conditions
/// the machine meets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstall {
    #[serde(default)]
    pub windows: Vec<ToolSource>,
    #[serde(default)]
    pub linux: Vec<ToolSource>,
    #[serde(default)]
    pub macos: Vec<ToolSource>,
}

/// One source, for the machines it suits. Unknown keys reach the source,
/// whose schema refuses them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ToolSource {
    /// Any one of these makes the source this machine's; none: every one.
    #[serde(default)]
    pub when: Vec<Condition>,
    #[serde(flatten)]
    pub source: InstallSource,
}

/// Something about the machine a source can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Condition {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
    /// A Wayland session (Linux).
    #[serde(rename = "wayland")]
    Wayland,
    /// An X11 session (Linux, not Wayland).
    #[serde(rename = "x11")]
    X11,
    /// gamescope: Steam Deck's game mode, a gamescope session.
    #[serde(rename = "gamescope")]
    Gamescope,
}

/// The machine's processor family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Other,
}

/// The machine sources are chosen for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub os: OperatingSystem,
    pub arch: Arch,
    pub wayland: bool,
    pub gamescope: bool,
}

impl Host {
    /// This machine, from its build target and session.
    pub fn current() -> Self {
        let var = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let os = crate::emulation::host_os();
        let linux = os == OperatingSystem::Linux;
        Self {
            os,
            arch: match std::env::consts::ARCH {
                "x86_64" => Arch::X86_64,
                "aarch64" => Arch::Aarch64,
                _ => Arch::Other,
            },
            wayland: linux
                && (var("WAYLAND_DISPLAY")
                    || std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind == "wayland")),
            gamescope: linux
                && (var("GAMESCOPE_WAYLAND_DISPLAY")
                    || desktop
                        .split(':')
                        .any(|part| part.eq_ignore_ascii_case("gamescope"))),
        }
    }

    pub fn meets(&self, condition: Condition) -> bool {
        let linux = self.os == OperatingSystem::Linux;
        match condition {
            Condition::X86_64 => self.arch == Arch::X86_64,
            Condition::Aarch64 => self.arch == Arch::Aarch64,
            Condition::Wayland => linux && self.wayland,
            Condition::X11 => linux && !self.wayland && !self.gamescope,
            Condition::Gamescope => linux && self.gamescope,
        }
    }
}

impl ToolDefinition {
    /// Parses and checks one definition.
    pub fn parse(text: &str) -> Result<Self> {
        let definition: Self =
            toml::from_str(text).map_err(|error| Error::Invalid(error.to_string()))?;
        definition.validate()?;
        Ok(definition)
    }

    /// The source for `host`: its OS's first whose conditions it meets.
    pub fn source(&self, host: &Host) -> Option<&InstallSource> {
        let sources = match host.os {
            OperatingSystem::Windows => &self.install.windows,
            OperatingSystem::Linux => &self.install.linux,
            OperatingSystem::Macos => &self.install.macos,
        };
        sources
            .iter()
            .find(|source| {
                source.when.is_empty() || source.when.iter().any(|&when| host.meets(when))
            })
            .map(|source| &source.source)
    }

    fn validate(&self) -> Result<()> {
        let invalid = |why: String| Err(Error::Invalid(format!("tool {}: {why}", self.id)));
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return invalid("the id is lowercase letters, digits and -".into());
        }
        // A folder inside the data folder, never beside or above it.
        let dir = Path::new(&self.dir);
        if self.dir.is_empty()
            || self.dir.contains('\\')
            || !dir
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return invalid(format!("dir {} isn't a relative folder", self.dir));
        }
        for (os, rules) in [
            ("windows", &self.detect.windows),
            ("linux", &self.detect.linux),
            ("macos", &self.detect.macos),
        ] {
            let Some(rules) = rules else { continue };
            for pattern in [&rules.executable, &rules.appimage].into_iter().flatten() {
                if let Err(error) = regex::Regex::new(pattern) {
                    return invalid(format!("detect.{os}: {error}"));
                }
            }
        }
        for (os, sources) in [
            ("windows", &self.install.windows),
            ("linux", &self.install.linux),
            ("macos", &self.install.macos),
        ] {
            for source in sources {
                if let InstallSource::Github {
                    repo, asset, url, ..
                } = &source.source
                {
                    if repo.split('/').filter(|part| !part.is_empty()).count() != 2 {
                        return invalid(format!("install.{os}: {repo} isn't owner/name"));
                    }
                    match (asset, url) {
                        (Some(pattern), None) => {
                            if let Err(error) = regex::Regex::new(pattern) {
                                return invalid(format!("install.{os}: {error}"));
                            }
                        }
                        (None, Some(_)) => {}
                        _ => return invalid(format!("install.{os}: name one asset or url")),
                    }
                }
            }
        }
        Ok(())
    }
}

/// The tools Caskade knows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCatalog {
    tools: Vec<ToolDefinition>,
}

impl ToolCatalog {
    /// Fails on a repeated id or folder.
    pub fn new(tools: Vec<ToolDefinition>) -> Result<Self> {
        for (at, tool) in tools.iter().enumerate() {
            if tools[..at]
                .iter()
                .any(|other| other.id == tool.id || other.dir == tool.dir)
            {
                return Err(Error::Invalid(format!(
                    "tool {} repeats another's id or folder",
                    tool.id
                )));
            }
        }
        Ok(Self { tools })
    }

    pub fn tool(&self, id: &str) -> Option<&ToolDefinition> {
        self.tools.iter().find(|tool| tool.id == id)
    }

    pub fn tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.iter()
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    fn host(os: OperatingSystem, arch: Arch, wayland: bool, gamescope: bool) -> Host {
        Host {
            os,
            arch,
            wayland,
            gamescope,
        }
    }

    const TOOL: &str = r#"
id = "viewer"
name = "Viewer"
dir = "streaming/client"

[[install.linux]]
when = ["wayland", "gamescope", "aarch64"]
source = "flatpak"
app = "org.example.Viewer"

[[install.linux]]
when = ["x86_64"]
source = "github"
repo = "example/viewer"
asset = '^Viewer-[\d.]+-x86_64\.AppImage$'
"#;

    #[test]
    fn the_first_source_the_machine_meets_is_its_own() {
        let tool = ToolDefinition::parse(TOOL).unwrap();
        let kind = |host: Host| tool.source(&host).map(InstallSource::kind);
        let linux = OperatingSystem::Linux;
        assert_eq!(
            kind(host(linux, Arch::X86_64, false, false)),
            Some("github")
        );
        assert_eq!(
            kind(host(linux, Arch::X86_64, true, false)),
            Some("flatpak")
        );
        assert_eq!(
            kind(host(linux, Arch::X86_64, false, true)),
            Some("flatpak")
        );
        assert_eq!(
            kind(host(linux, Arch::Aarch64, false, false)),
            Some("flatpak")
        );
        assert_eq!(kind(host(linux, Arch::Other, false, false)), None);
        assert_eq!(
            kind(host(OperatingSystem::Windows, Arch::X86_64, false, false)),
            None
        );
    }

    #[test]
    fn definitions_are_checked() {
        for (wrong, why) in [
            (TOOL.replace("streaming/client", "../client"), "dir"),
            (TOOL.replace("streaming/client", "/client"), "dir"),
            (TOOL.replace("id = \"viewer\"", "id = \"Viewer\""), "id"),
            (TOOL.replace("example/viewer", "viewer"), "repo"),
            (TOOL.replace("[\\d.]+", "[\\d.+"), "asset"),
            (TOOL.replace("app = ", "apps = "), "an unknown key"),
            (TOOL.replace("\"aarch64\"]", "\"riscv\"]"), "a condition"),
            (
                format!("{TOOL}\nprofiles = []\n"),
                "an unknown top-level key",
            ),
        ] {
            assert!(ToolDefinition::parse(&wrong).is_err(), "{why}");
        }
        let tool = ToolDefinition::parse(TOOL).unwrap();
        assert!(ToolCatalog::new(vec![tool.clone(), tool]).is_err());
    }
}
