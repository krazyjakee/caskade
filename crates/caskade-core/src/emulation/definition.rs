//! Emulation data (EMULATION-PLAN §3): platforms, regions and emulator
//! definitions, read from TOML. Read-only to the user; plugins contribute
//! more in the same format.

use super::Tracking;
use caskade_installer::OperatingSystem;
use serde::{Deserialize, Serialize, de};
use std::collections::BTreeMap;

/// A system games run on, e.g. `nintendo_snes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub id: String,
    pub name: String,
    pub short: String,
    /// Lowercase, without the dot. Identifies a stray ROM with no scanner.
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub ids: PlatformIds,
    #[serde(default)]
    pub bios: Vec<Bios>,
    /// Emulators to install for the platform, best first: `<emulator>` or
    /// `<emulator>:<profile>` (EMULATOR-SETUP-PLAN §2.4).
    #[serde(default)]
    pub recommended: Vec<String>,
}

/// The platform in other databases and tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformIds {
    pub igdb: Option<u32>,
    /// Canonical and historical RomM slugs. Several can name the same system.
    #[serde(default)]
    pub romm: Vec<String>,
    pub libretro: Option<String>,
    pub playnite: Option<String>,
    pub esde: Option<String>,
    /// The platform's Hasheous data dump, without `.zip` (`crate::hasheous`).
    pub hasheous: Option<String>,
}

/// A BIOS or firmware file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bios {
    pub file: String,
    pub md5: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// A region and the codes found for it in ROM names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub codes: Vec<String>,
}

/// One emulator as Caskade knows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmulatorDefinition {
    pub id: String,
    pub name: String,
    pub website: Option<String>,
    #[serde(default)]
    pub detect: Detect,
    #[serde(default)]
    pub vars: Vars,
    /// Save files and save states, as absolute path/glob templates. Folders
    /// are scanned recursively; ROM variables keep per-game saves separate.
    #[serde(default)]
    pub saves: DefinitionSaves,
    /// Configuration that travels between the user's devices while
    /// Emulator configuration syncs (#275): controller mappings, core
    /// options, shader presets. Path/glob templates like `saves`'s.
    #[serde(default)]
    pub config: DefinitionConfig,
    /// Profiles in recommended order.
    #[serde(default, rename = "profile")]
    pub profiles: Vec<DefinitionProfile>,
    /// An importer for games that aren't plain files (e.g. `rpcs3`).
    pub import: Option<String>,
    /// The default tracking mode when the install kind's isn't right.
    pub tracking: Option<Tracking>,
    /// Where Caskade downloads it from, per OS (EMULATOR-SETUP-PLAN §2.1).
    #[serde(default)]
    pub install: Install,
    /// Flags for fullscreen and windowed play (§2.2).
    #[serde(default)]
    pub display: Display,
    /// What an install writes so the first launch needs no setup (§2.3).
    #[serde(default)]
    pub setup: Vec<SetupStep>,
    /// Where a profile's missing `requires` files (RetroArch cores)
    /// download from, per OS.
    #[serde(default)]
    pub requires_from: RequiresFrom,
}

impl EmulatorDefinition {
    pub fn profile(&self, name: &str) -> Option<&DefinitionProfile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }

    /// Expand save patterns, escaping device/ROM values as glob literals.
    /// Missing values fail rather than scanning an unintended parent folder.
    pub fn resolve_saves(
        &self,
        os: OperatingSystem,
        kind: InstallKind,
        context: &super::VarContext,
    ) -> Result<DefinitionSaves, String> {
        let expander = super::vars::Expander {
            vars: &self.vars,
            os,
            kind,
            context,
        };
        let expand = |patterns: &[String]| -> Result<Vec<String>, String> {
            patterns
                .iter()
                .map(|pattern| expander.expand_glob(pattern))
                .collect()
        };
        Ok(DefinitionSaves {
            files: expand(&self.saves.files)?,
            states: expand(&self.saves.states)?,
        })
    }

    /// Expand the `[config]` patterns for this install, as `resolve_saves`
    /// does the save patterns.
    pub fn resolve_config(
        &self,
        os: OperatingSystem,
        kind: InstallKind,
        context: &super::VarContext,
    ) -> Result<Vec<String>, String> {
        let expander = super::vars::Expander {
            vars: &self.vars,
            os,
            kind,
            context,
        };
        self.config
            .files
            .iter()
            .map(|pattern| expander.expand_glob(pattern))
            .collect()
    }

    /// The folders saves may sit under for this install, by name: the
    /// definition's variables (`{save_data}`) and the built-in folders
    /// (`{RomDir}`, `{EmulatorDir}`, …), each with its path here. Save
    /// versions are stored relative to them (#239), so they restore under
    /// the same names on another device. Values that aren't absolute paths
    /// are left out.
    pub fn save_roots(
        &self,
        os: OperatingSystem,
        kind: InstallKind,
        context: &super::VarContext,
    ) -> Vec<(String, String)> {
        let expander = super::vars::Expander {
            vars: &self.vars,
            os,
            kind,
            context,
        };
        let builtins = [
            "RomDir",
            "EmulatorDir",
            "UserHome",
            "XdgConfig",
            "AppData",
            "AppSupport",
        ];
        self.vars
            .names(os, kind)
            .into_iter()
            .chain(builtins)
            .filter_map(|name| {
                let template = format!("{{{name}}}");
                let path = expander.expand(&template).ok()?.replace('\\', "/");
                let absolute = path.starts_with('/')
                    || (path.as_bytes().get(1) == Some(&b':') && path.len() > 2);
                absolute.then_some((template, path))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionSaves {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub states: Vec<String>,
}

/// Files of an emulator's own configuration that sync (#275), as absolute
/// path/glob templates; a trailing `**` takes every file under a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionConfig {
    #[serde(default)]
    pub files: Vec<String>,
}

/// Where to look for an emulator on each OS.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detect {
    pub windows: Option<DetectRules>,
    pub linux: Option<DetectRules>,
    pub macos: Option<DetectRules>,
}

impl Detect {
    pub fn on(&self, os: OperatingSystem) -> Option<&DetectRules> {
        match os {
            OperatingSystem::Windows => self.windows.as_ref(),
            OperatingSystem::Linux => self.linux.as_ref(),
            OperatingSystem::Macos => self.macos.as_ref(),
        }
    }
}

/// Where Caskade installs an emulator from on each OS.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub windows: Option<InstallSource>,
    pub linux: Option<InstallSource>,
    pub macos: Option<InstallSource>,
}

impl Install {
    pub fn on(&self, os: OperatingSystem) -> Option<&InstallSource> {
        match os {
            OperatingSystem::Windows => self.windows.as_ref(),
            OperatingSystem::Linux => self.linux.as_ref(),
            OperatingSystem::Macos => self.macos.as_ref(),
        }
    }
}

/// One download source (EMULATOR-SETUP-PLAN §2.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstallSource {
    /// A GitHub repository's latest release: the asset matching `asset`,
    /// or `url` with `{version}` (the tag without a leading `v`) or `{tag}`.
    Github {
        /// `owner/repo`.
        repo: String,
        asset: Option<String>,
        url: Option<String>,
        /// The tag is reused (`latest`, `continuous`) or a commit hash, so
        /// releases are told apart by when they were published.
        #[serde(default)]
        rolling: bool,
    },
    /// Dolphin's update endpoint, which names a file per `artifact`
    /// (`Windows x64`, `macOS (ARM/Intel Universal)`, …).
    Dolphin { artifact: String },
    /// A fixed file; nothing tells its version, so it isn't updated.
    Url { url: String },
    /// A Flathub app, installed and updated by Flatpak.
    Flatpak { app: String },
}

impl InstallSource {
    /// `github`, `dolphin`, `url` or `flatpak`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Github { .. } => "github",
            Self::Dolphin { .. } => "dolphin",
            Self::Url { .. } => "url",
            Self::Flatpak { .. } => "flatpak",
        }
    }

    /// Whether Caskade finds newer versions of it.
    pub fn tracked(&self) -> bool {
        matches!(self, Self::Github { .. } | Self::Dolphin { .. })
    }
}

/// URL templates for a profile's `requires` files, per OS: `{file}` is
/// the file's name (`snes9x_libretro.so`), `{arch}` the machine's
/// (`x86_64` or `arm64`). A `.zip` answer is unpacked and the file taken
/// from it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiresFrom {
    pub windows: Option<String>,
    pub linux: Option<String>,
    pub macos: Option<String>,
}

impl RequiresFrom {
    pub fn on(&self, os: OperatingSystem) -> Option<&str> {
        match os {
            OperatingSystem::Windows => self.windows.as_deref(),
            OperatingSystem::Linux => self.linux.as_deref(),
            OperatingSystem::Macos => self.macos.as_deref(),
        }
    }
}

/// The flags that make a launch fullscreen or windowed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Display {
    #[serde(default)]
    pub fullscreen: Vec<String>,
    #[serde(default)]
    pub windowed: Vec<String>,
}

/// A file an install writes: a key in an INI file (`ini`, `section`,
/// `key`, `value`), or a whole file written only when missing (`file`,
/// `text`). Paths and INI values take the definition's variables; `text`
/// is written as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupStep {
    pub ini: Option<String>,
    pub section: Option<String>,
    pub key: Option<String>,
    pub value: Option<String>,
    pub file: Option<String>,
    pub text: Option<String>,
    /// The OSes it applies on ([`os_key`]); every OS when empty.
    #[serde(default)]
    pub os: Vec<String>,
}

/// What a [`SetupStep`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAction<'a> {
    Ini {
        path: &'a str,
        section: &'a str,
        key: &'a str,
        value: &'a str,
    },
    File {
        path: &'a str,
        text: &'a str,
    },
}

impl SetupStep {
    /// The step's action; `None` when its fields don't make one.
    pub fn action(&self) -> Option<SetupAction<'_>> {
        match self {
            Self {
                ini: Some(path),
                section: Some(section),
                key: Some(key),
                value: Some(value),
                file: None,
                text: None,
                ..
            } => Some(SetupAction::Ini {
                path,
                section,
                key,
                value,
            }),
            Self {
                file: Some(path),
                ini: None,
                section: None,
                key: None,
                value: None,
                text,
                ..
            } => Some(SetupAction::File {
                path,
                text: text.as_deref().unwrap_or_default(),
            }),
            _ => None,
        }
    }

    pub fn applies(&self, os: OperatingSystem) -> bool {
        self.os.is_empty() || self.os.iter().any(|key| key == os_key(os))
    }
}

/// Any rule that matches makes an install.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectRules {
    /// Regex matched against file names in `locations` and user folders.
    pub executable: Option<String>,
    /// Folders to look in; variables allowed.
    #[serde(default)]
    pub locations: Vec<String>,
    pub flatpak: Option<String>,
    /// Binary names looked up in `$PATH`.
    #[serde(default)]
    pub path: Vec<String>,
    /// Regex matched against AppImage file names.
    pub appimage: Option<String>,
    /// macOS bundle id.
    pub bundle: Option<String>,
}

/// One kind of ROM an emulator runs, as the definition describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionProfile {
    pub name: String,
    pub platforms: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    /// argv tokens; variables are expanded inside each token.
    #[serde(default)]
    pub args: Vec<String>,
    /// The profile is available only when these paths exist.
    #[serde(default)]
    pub requires: Vec<String>,
    /// BIOS beyond the platform's list.
    #[serde(default)]
    pub bios: Vec<Bios>,
    pub working_dir: Option<String>,
    pub tracking: Option<Tracking>,
}

/// How an emulator is installed, which also picks its `vars`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    Folder,
    Path,
    Flatpak,
    AppImage,
    AppBundle,
}

impl InstallKind {
    pub const ALL: [Self; 5] = [
        Self::Folder,
        Self::Path,
        Self::Flatpak,
        Self::AppImage,
        Self::AppBundle,
    ];

    /// The key used in `[vars.<os>.<kind>]` and `[detect.<os>]`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Path => "path",
            Self::Flatpak => "flatpak",
            Self::AppImage => "appimage",
            Self::AppBundle => "bundle",
        }
    }

    /// The kinds an emulator can have on `os`.
    pub fn on(os: OperatingSystem) -> &'static [Self] {
        match os {
            OperatingSystem::Windows => &[Self::Folder, Self::Path],
            OperatingSystem::Linux => &[Self::Folder, Self::Path, Self::Flatpak, Self::AppImage],
            OperatingSystem::Macos => &[Self::Folder, Self::Path, Self::AppBundle],
        }
    }
}

pub const OSES: [OperatingSystem; 3] = [
    OperatingSystem::Windows,
    OperatingSystem::Linux,
    OperatingSystem::Macos,
];

pub fn os_key(os: OperatingSystem) -> &'static str {
    match os {
        OperatingSystem::Windows => "windows",
        OperatingSystem::Linux => "linux",
        OperatingSystem::Macos => "macos",
    }
}

/// Variables a definition's profiles can use. Per-OS values win over the
/// shared ones, and per-install-kind values win over both:
///
/// ```toml
/// [vars]
/// cores = "{EmulatorDir}/cores"
/// [vars.linux]
/// cores = "{XdgConfig}/retroarch/cores"
/// [vars.linux.flatpak]
/// cores = "{UserHome}/.var/app/org.libretro.RetroArch/config/retroarch/cores"
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vars {
    pub shared: BTreeMap<String, String>,
    /// Keyed by [`os_key`].
    pub os: BTreeMap<String, OsVars>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsVars {
    pub values: BTreeMap<String, String>,
    /// Keyed by [`InstallKind::key`].
    pub kinds: BTreeMap<String, BTreeMap<String, String>>,
}

impl Vars {
    /// No variables of its own: custom profiles and per-game arguments know
    /// only the built-in ones.
    pub const NONE: &Self = &Self {
        shared: BTreeMap::new(),
        os: BTreeMap::new(),
    };

    /// The template of `name` for an install of `kind` on `os`.
    pub fn get(&self, name: &str, os: OperatingSystem, kind: InstallKind) -> Option<&str> {
        let os = self.os.get(os_key(os));
        os.and_then(|os| os.kinds.get(kind.key()))
            .and_then(|values| values.get(name))
            .or_else(|| os.and_then(|os| os.values.get(name)))
            .or_else(|| self.shared.get(name))
            .map(String::as_str)
    }

    /// Every variable name visible to an install of `kind` on `os`.
    pub fn names(&self, os: OperatingSystem, kind: InstallKind) -> Vec<&str> {
        let os = self.os.get(os_key(os));
        let mut names: Vec<&str> = self.shared.keys().map(String::as_str).collect();
        if let Some(os) = os {
            names.extend(os.values.keys().map(String::as_str));
            if let Some(values) = os.kinds.get(kind.key()) {
                names.extend(values.keys().map(String::as_str));
            }
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Every template with its key path, for validation.
    pub fn templates(&self) -> Vec<(String, &str)> {
        let mut all: Vec<(String, &str)> = self
            .shared
            .iter()
            .map(|(name, value)| (format!("vars.{name}"), value.as_str()))
            .collect();
        for (os, values) in &self.os {
            for (name, value) in &values.values {
                all.push((format!("vars.{os}.{name}"), value));
            }
            for (kind, values) in &values.kinds {
                for (name, value) in values {
                    all.push((format!("vars.{os}.{kind}.{name}"), value));
                }
            }
        }
        all
    }
}

impl<'de> Deserialize<'de> for Vars {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use toml::Value;
        let table = BTreeMap::<String, Value>::deserialize(deserializer)?;
        let strings = |table: BTreeMap<String, Value>, at: &str| {
            table
                .into_iter()
                .map(|(name, value)| match value {
                    Value::String(text) => Ok((name, text)),
                    _ => Err(de::Error::custom(format!("{at}.{name} must be a string"))),
                })
                .collect::<Result<BTreeMap<_, _>, D::Error>>()
        };
        let mut vars = Vars::default();
        for (key, value) in table {
            match value {
                Value::String(text) => {
                    vars.shared.insert(key, text);
                }
                Value::Table(table) if OSES.iter().any(|os| os_key(*os) == key) => {
                    let mut os = OsVars::default();
                    for (name, value) in table {
                        match value {
                            Value::String(text) => {
                                os.values.insert(name, text);
                            }
                            Value::Table(table)
                                if InstallKind::ALL.iter().any(|kind| kind.key() == name) =>
                            {
                                let at = format!("vars.{key}.{name}");
                                let values = strings(table.into_iter().collect(), &at)?;
                                os.kinds.insert(name, values);
                            }
                            _ => {
                                return Err(de::Error::custom(format!(
                                    "vars.{key}.{name} must be a string or an install kind table"
                                )));
                            }
                        }
                    }
                    vars.os.insert(key, os);
                }
                _ => {
                    return Err(de::Error::custom(format!(
                        "vars.{key} must be a string or an OS table"
                    )));
                }
            }
        }
        Ok(vars)
    }
}

impl Serialize for Vars {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde_json::{Map, Value};
        let mut out = Map::new();
        for (name, value) in &self.shared {
            out.insert(name.clone(), Value::String(value.clone()));
        }
        for (os, values) in &self.os {
            let mut table = Map::new();
            for (name, value) in &values.values {
                table.insert(name.clone(), Value::String(value.clone()));
            }
            for (kind, values) in &values.kinds {
                let values = values
                    .iter()
                    .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                    .collect();
                table.insert(kind.clone(), Value::Object(values));
            }
            out.insert(os.clone(), Value::Object(table));
        }
        out.serialize(serializer)
    }
}
