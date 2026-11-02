//! Variables in `args`, `requires`, `vars`, working directories and tracking
//! paths (EMULATION-PLAN §3.3). `{Name}` is replaced inside each argv token,
//! so a value with spaces stays one argument and nothing needs quoting.

use super::{InstallKind, Vars};
use caskade_installer::OperatingSystem;
use std::path::PathBuf;

/// Built-in variables, with the Playnite names kept as aliases.
pub const BUILTINS: [&str; 15] = [
    "RomPath",
    "RomName",
    "RomNameNoExt",
    "RomDir",
    "EmulatorDir",
    "Name",
    "Platform",
    "GameId",
    "CaskadeDir",
    "StorageRoot",
    "UserHome",
    "XdgConfig",
    "AppData",
    "ProgramFiles",
    "AppSupport",
];

const ALIASES: [(&str, &str); 4] = [
    ("ImagePath", "RomPath"),
    ("ImageName", "RomName"),
    ("ImageNameNoExt", "RomNameNoExt"),
    ("ImageDir", "RomDir"),
];

/// The Playnite names of a built-in.
pub(super) fn aliases_of(builtin: &str) -> impl Iterator<Item = &'static str> + '_ {
    ALIASES
        .iter()
        .filter(move |(_, target)| *target == builtin)
        .map(|(alias, _)| *alias)
}

/// The built-in a name refers to, if it is one.
pub fn builtin(name: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .copied()
        .find(|builtin| *builtin == name)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(alias, _)| *alias == name)
                .map(|(_, builtin)| *builtin)
        })
}

/// A template piece.
#[derive(Debug, PartialEq, Eq)]
enum Piece<'a> {
    Text(&'a str),
    Var(&'a str),
}

/// Splits a template into text and `{Name}` variables.
fn parse(template: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut pieces = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find(['{', '}']) {
        if rest.as_bytes()[open] == b'}' {
            return Err(format!("unmatched `}}` in {template:?}"));
        }
        if open > 0 {
            pieces.push(Piece::Text(&rest[..open]));
        }
        let after = &rest[open + 1..];
        let close = after
            .find(['{', '}'])
            .filter(|at| after.as_bytes()[*at] == b'}')
            .ok_or_else(|| format!("unclosed `{{` in {template:?}"))?;
        let name = &after[..close];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("invalid variable `{{{name}}}` in {template:?}"));
        }
        pieces.push(Piece::Var(name));
        rest = &after[close + 1..];
    }
    if !rest.is_empty() {
        pieces.push(Piece::Text(rest));
    }
    Ok(pieces)
}

/// Whether `template` starts with the ROM's file name (`{RomName}`,
/// `{RomNameNoExt}` or their aliases), unless a definition variable of that
/// name shadows the built-in. A ROM named `-option.zip` would then be read
/// as an option by an emulator such as MAME, which has no `--` (CSK-10).
pub(super) fn leads_with_rom_name(
    vars: &Vars,
    template: &str,
    os: OperatingSystem,
    kind: InstallKind,
) -> bool {
    match parse(template)
        .ok()
        .and_then(|pieces| pieces.into_iter().next())
    {
        Some(Piece::Var(name)) => {
            vars.get(name, os, kind).is_none()
                && matches!(builtin(name), Some("RomName" | "RomNameNoExt"))
        }
        _ => false,
    }
}

/// The variable names a template uses.
pub fn names(template: &str) -> Result<Vec<&str>, String> {
    Ok(parse(template)?
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Var(name) => Some(name),
            Piece::Text(_) => None,
        })
        .collect())
}

/// OS folders; empty on OSes that don't have them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsFolders {
    pub user_home: Option<PathBuf>,
    pub xdg_config: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub app_support: Option<PathBuf>,
}

impl OsFolders {
    /// This machine's folders.
    pub fn host() -> Self {
        let dirs = directories::BaseDirs::new();
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            user_home: dirs.as_ref().map(|dirs| dirs.home_dir().to_path_buf()),
            xdg_config: cfg!(target_os = "linux")
                .then(|| dirs.as_ref().map(|dirs| dirs.config_dir().to_path_buf()))
                .flatten(),
            app_data: cfg!(windows).then(|| env("APPDATA")).flatten(),
            program_files: cfg!(windows).then(|| env("ProgramFiles")).flatten(),
            app_support: cfg!(target_os = "macos")
                .then(|| dirs.as_ref().map(|dirs| dirs.data_dir().to_path_buf()))
                .flatten(),
        }
    }
}

/// The values of the built-in variables for one launch (or one check).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VarContext {
    pub rom: Option<PathBuf>,
    /// Install folder; for a Flatpak, the app's data dir.
    pub emulator_dir: Option<PathBuf>,
    pub name: Option<String>,
    pub platform: Option<String>,
    pub game_id: Option<String>,
    pub caskade_dir: Option<PathBuf>,
    pub storage_root: Option<PathBuf>,
    pub folders: OsFolders,
}

/// Splits a path the way `os` does, so resolving for another OS (tests,
/// definition checks) matches resolving on it.
fn split(path: &str, os: OperatingSystem) -> (&str, &str) {
    let separators: &[char] = match os {
        OperatingSystem::Windows => &['\\', '/'],
        OperatingSystem::Linux | OperatingSystem::Macos => &['/'],
    };
    match path.rfind(separators) {
        Some(at) => (&path[..at], &path[at + 1..]),
        None => ("", path),
    }
}

impl VarContext {
    fn value(&self, builtin: &str, os: OperatingSystem) -> String {
        let path = |path: &Option<PathBuf>| {
            path.as_deref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let rom = path(&self.rom);
        let (rom_dir, rom_name) = split(&rom, os);
        match builtin {
            "RomPath" => rom.clone(),
            "RomName" => rom_name.to_owned(),
            "RomNameNoExt" => match rom_name.rfind('.') {
                Some(at) if at > 0 => rom_name[..at].to_owned(),
                _ => rom_name.to_owned(),
            },
            "RomDir" => rom_dir.to_owned(),
            "EmulatorDir" => path(&self.emulator_dir),
            "Name" => self.name.clone().unwrap_or_default(),
            "Platform" => self.platform.clone().unwrap_or_default(),
            "GameId" => self.game_id.clone().unwrap_or_default(),
            "CaskadeDir" => path(&self.caskade_dir),
            "StorageRoot" => path(&self.storage_root),
            "UserHome" => path(&self.folders.user_home),
            "XdgConfig" => path(&self.folders.xdg_config),
            "AppData" => path(&self.folders.app_data),
            "ProgramFiles" => path(&self.folders.program_files),
            "AppSupport" => path(&self.folders.app_support),
            _ => unreachable!("not a built-in: {builtin}"),
        }
    }
}

/// Expands templates for one install of one definition.
pub struct Expander<'a> {
    pub vars: &'a Vars,
    pub os: OperatingSystem,
    pub kind: InstallKind,
    pub context: &'a VarContext,
}

/// Definition variables may use each other; deeper chains are a cycle.
const MAX_DEPTH: usize = 16;

impl Expander<'_> {
    pub fn expand(&self, template: &str) -> Result<String, String> {
        self.expand_at(template, &mut Vec::new(), false)
    }

    pub(super) fn expand_glob(&self, template: &str) -> Result<String, String> {
        self.expand_at(template, &mut Vec::new(), true)
    }

    fn expand_at<'t>(
        &'t self,
        template: &'t str,
        stack: &mut Vec<&'t str>,
        glob: bool,
    ) -> Result<String, String> {
        let mut out = String::new();
        for piece in parse(template)? {
            match piece {
                Piece::Text(text) => out.push_str(text),
                Piece::Var(name) => {
                    // A definition variable may shadow a built-in (`cores`
                    // never collides, but plugins may define `RomDir`).
                    if let Some(value) = self.vars.get(name, self.os, self.kind) {
                        if stack.contains(&name) || stack.len() >= MAX_DEPTH {
                            return Err(format!("variable `{{{name}}}` refers to itself"));
                        }
                        stack.push(name);
                        out.push_str(&self.expand_at(value, stack, glob)?);
                        stack.pop();
                    } else if let Some(builtin) = builtin(name) {
                        let value = self.context.value(builtin, self.os);
                        if glob {
                            if value.is_empty() {
                                return Err(crate::tr!(
                                    "Save variable {variable} is unavailable for this release.",
                                    variable = format!("{{{builtin}}}")
                                ));
                            }
                            out.push_str(&globset::escape(&value.replace('\\', "/")));
                        } else {
                            out.push_str(&value);
                        }
                    } else {
                        return Err(format!("unknown variable `{{{name}}}`"));
                    }
                }
            }
        }
        Ok(out)
    }
}
