//! Documented Windows installer arguments. These are logical argv entries;
//! runtimes own OS command-line serialization and must preserve NSIS's raw tail.
//! Package-specific language/task/property names must come from a recipe.

use std::{collections::BTreeMap, sync::OnceLock};

use serde::Deserialize;

use crate::{Format, exec::INSTALL_DIR};

use super::PresetArgs;

/// Families with a documented generic command-line contract or an explicit
/// interactive-only fallback. Detection of InstallShield alone is insufficient
/// to select one of its incompatible silent invocation variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallerFamily {
    Inno,
    Nsis,
    Msi,
    Burn,
    InstallShield,
    Wise,
    SetupFactory,
    Custom,
}

impl From<Format> for InstallerFamily {
    fn from(format: Format) -> Self {
        match format {
            Format::Inno => Self::Inno,
            Format::Nsis => Self::Nsis,
            Format::Msi => Self::Msi,
            Format::Burn => Self::Burn,
            Format::InstallShield => Self::InstallShield,
            Format::Wise => Self::Wise,
            Format::SetupFactory => Self::SetupFactory,
            _ => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerMode {
    Silent,
    Interactive,
}

/// Settings consumed by format handlers. Keep the directory placeholder until
/// execution so retries and collision suffixes receive the correct final path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchSettings {
    pub install_dir: String,
    pub language: Option<String>,
    pub let_installers_create_shortcuts: bool,
    pub no_launch_at_finish: bool,
}

impl Default for SwitchSettings {
    fn default() -> Self {
        Self {
            install_dir: INSTALL_DIR.into(),
            language: None,
            let_installers_create_shortcuts: false,
            no_launch_at_finish: true,
        }
    }
}

impl From<&PresetArgs> for SwitchSettings {
    fn from(args: &PresetArgs) -> Self {
        Self {
            language: args.language.clone(),
            let_installers_create_shortcuts: !args.no_shortcuts,
            ..Self::default()
        }
    }
}

/// The embedded, reviewable source table. Empty capability lists indicate that
/// a generic preset is unavailable, not that the setting was silently applied.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilySwitches {
    /// Used only after a handler has identified the Basic MSI bootstrapper variant.
    #[serde(default)]
    pub identified_msi_silent_args: Option<Vec<String>>,
    pub silent_args: Vec<String>,
    pub interactive_args: Vec<String>,
    pub dir_args: Vec<String>,
    pub language_args: Vec<String>,
    pub no_shortcuts_args: Vec<String>,
    pub no_launch_args: Vec<String>,
    pub dir_last: bool,
    pub silent_supported: bool,
    /// Installer versions actually checked by a person on a native machine.
    pub checked_versions: Vec<String>,
    pub notes: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltArgs {
    pub args: Vec<String>,
    /// Unsupported preferences and the table's version/scope limitations.
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PresetError {
    #[error("this installer family needs a package-specific silent recipe")]
    SilentUnsupported,
    #[error("this installer family has no generic uninstall argument contract")]
    UninstallUnsupported,
    #[error("MSI uninstall requires a braced product GUID")]
    InvalidProductCode,
    #[error("{0} must be nonempty and contain no quotes or control characters")]
    InvalidValue(&'static str),
}

pub fn switch_catalog() -> &'static BTreeMap<InstallerFamily, FamilySwitches> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Catalog {
        families: BTreeMap<InstallerFamily, FamilySwitches>,
    }
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    &CATALOG
        .get_or_init(|| {
            toml::from_str(include_str!("../../data/silent-switches.toml"))
                .expect("embedded installer switch table is valid")
        })
        .families
}

/// Build presets only. MSI callers prepend `/i` and the package filename;
/// executable installers use the returned vector directly. Do not append more
/// arguments after NSIS's `/D=` entry.
pub fn build_args(
    family: InstallerFamily,
    mode: InstallerMode,
    settings: &SwitchSettings,
) -> Result<BuiltArgs, PresetError> {
    fn validate(value: &str, label: &'static str) -> Result<(), PresetError> {
        if value.is_empty() || value.chars().any(|c| c == '"' || c.is_control()) {
            Err(PresetError::InvalidValue(label))
        } else {
            Ok(())
        }
    }
    validate(&settings.install_dir, "install directory")?;
    if let Some(language) = &settings.language {
        validate(language, "installer language")?;
    }
    let entry = &switch_catalog()[&family];
    if mode == InstallerMode::Silent && !entry.silent_supported {
        return Err(PresetError::SilentUnsupported);
    }
    let expand = |templates: &[String]| -> Vec<String> {
        templates
            .iter()
            .map(|template| {
                template
                    .replace("{install_dir}", &settings.install_dir)
                    .replace(
                        "{language}",
                        settings.language.as_deref().unwrap_or_default(),
                    )
            })
            .collect()
    };
    let mut args = match mode {
        InstallerMode::Silent => entry.silent_args.clone(),
        InstallerMode::Interactive => entry.interactive_args.clone(),
    };
    let mut warnings = vec![entry.notes.clone()];
    if entry.checked_versions.is_empty() {
        warnings.push(
            "Presets have not yet been hand-checked against a native installer version.".into(),
        );
    }
    if entry.dir_args.is_empty() {
        warnings
            .push("Install directory cannot be preset without a package-specific recipe.".into());
    }
    if !entry.dir_last {
        args.extend(expand(&entry.dir_args));
    }
    if settings.language.is_some() {
        if entry.language_args.is_empty() {
            warnings.push(
                "Installer language cannot be preset without a package-specific recipe.".into(),
            );
        } else {
            args.extend(expand(&entry.language_args));
        }
    }
    if !settings.let_installers_create_shortcuts {
        if entry.no_shortcuts_args.is_empty() {
            warnings.push("Shortcut suppression needs a package-specific recipe.".into());
        } else {
            args.extend(expand(&entry.no_shortcuts_args));
        }
    }
    if settings.no_launch_at_finish {
        if entry.no_launch_args.is_empty() {
            warnings.push("Launch-at-finish suppression needs a package-specific recipe.".into());
        } else {
            args.extend(expand(&entry.no_launch_args));
        }
    }
    if entry.dir_last {
        args.extend(expand(&entry.dir_args));
    }
    Ok(BuiltArgs { args, warnings })
}

/// Logical argv for a validated, family-specific native uninstaller. This does
/// not parse registry command lines or authorize a program path. Keep the NSIS
/// directory placeholder in its final unquoted entry until execution.
pub fn build_uninstall_args(
    family: InstallerFamily,
    product_code: Option<&str>,
) -> Result<Vec<String>, PresetError> {
    match family {
        InstallerFamily::Inno => Ok(vec![
            "/VERYSILENT".into(),
            "/SUPPRESSMSGBOXES".into(),
            "/NORESTART".into(),
        ]),
        InstallerFamily::Nsis => Ok(vec!["/S".into(), format!("_?={INSTALL_DIR}")]),
        InstallerFamily::Msi => {
            let code = product_code.ok_or(PresetError::InvalidProductCode)?;
            let bytes = code.as_bytes();
            if bytes.len() != 38
                || bytes[0] != b'{'
                || bytes[37] != b'}'
                || !(1..37).all(|index| {
                    if [9, 14, 19, 24].contains(&index) {
                        bytes[index] == b'-'
                    } else {
                        bytes[index].is_ascii_hexdigit()
                    }
                })
            {
                return Err(PresetError::InvalidProductCode);
            }
            Ok(vec![
                "/x".into(),
                code.into(),
                "/qn".into(),
                "/norestart".into(),
            ])
        }
        _ => Err(PresetError::UninstallUnsupported),
    }
}
