use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Arguments stay separate all the way to tokio::process; no shell evaluation.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, specta::Type,
)]
pub struct LaunchCommand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulation: Option<EmulationLaunch>,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}
impl LaunchCommand {
    pub fn executable(program: impl Into<PathBuf>) -> Self {
        Self {
            emulation: None,
            program: program.into(),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
        }
    }
    pub fn validate(&self) -> Result<(), super::LaunchError> {
        if self.program.as_os_str().is_empty()
            || self.args.iter().any(|v| v.contains('\0'))
            || self.env.iter().any(|(key, value)| {
                key.is_empty() || key.contains(['=', '\0']) || value.contains('\0')
            })
        {
            return Err(super::LaunchError::Invalid(crate::tr!(
                "invalid executable arguments or environment"
            )));
        }
        Ok(())
    }
}

/// What an emulator launch resolved to (EMULATION-PLAN §8). `launch.before`
/// interceptors may rewrite it with the argv; the frontend never receives it.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub struct EmulationLaunch {
    pub emulator_id: String,
    pub profile_id: String,
    pub platform_id: String,
    /// The ROM `{RomPath}` named: the chosen disc or the playlist.
    pub rom: PathBuf,
    /// The profile's commands, variables expanded. A non-zero `before` exit
    /// cancels the launch; `after_exit` runs once the session has ended.
    pub before: Option<Vec<String>>,
    pub after_exit: Option<Vec<String>>,
}

/// The game launchers' URI schemes a launch may use. Everything else is
/// refused: the OS dispatchers (`rundll32 url.dll` on Windows above all) also
/// reach handlers such as `search-ms:`, `ms-msdt:` or `ms-word:` that open
/// remote content or run programs (CSK-09). These are the schemes Caskade's
/// discovery sources, start-menu shortcuts and installer hand-offs produce.
const LAUNCHER_SCHEMES: [&str; 15] = [
    "steam",
    "com.epicgames.launcher",
    "heroic",
    "lutris",
    "battlenet",
    "uplay",
    "goggalaxy",
    "itch",
    "itchio",
    "amazon-games",
    "origin",
    "origin2",
    "link2ea",
    // The installer's Flatpak hand-off.
    "flatpak",
    // Only as `shell:AppsFolder\<family>!<app>`, for packaged (Xbox) apps.
    "shell",
];

/// Whether `rest`, after `shell:`, names a packaged app and nothing else.
fn packaged_app(rest: &str) -> bool {
    let Some(aumid) = rest
        .get(..11)
        .filter(|prefix| prefix.eq_ignore_ascii_case(r"AppsFolder\"))
        .map(|_| &rest[11..])
    else {
        return false;
    };
    let part = |text: &str| {
        text.bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    aumid
        .split_once('!')
        .is_some_and(|(family, app)| part(family) && part(app))
}

/// Whether `app_id` can go to `flatpak run` as an app id: reverse-DNS, and
/// never an option.
pub(super) fn flatpak_id(app_id: &str) -> bool {
    !app_id.starts_with('-')
        && app_id.split('.').count() >= 3
        && app_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Uses the platform's URI dispatcher, with one URI argument and no shell.
pub fn uri_command(uri: &str) -> Result<LaunchCommand, super::LaunchError> {
    let (scheme, rest) = uri
        .split_once(':')
        .ok_or_else(|| super::LaunchError::Invalid(crate::tr!("launch URI has no scheme")))?;
    let scheme = scheme.to_ascii_lowercase();
    if !LAUNCHER_SCHEMES.contains(&scheme.as_str())
        || (scheme == "shell" && !packaged_app(rest))
        || uri.chars().any(char::is_control)
    {
        return Err(super::LaunchError::Invalid(crate::tr!(
            "not a game launcher URI"
        )));
    }
    let mut command = LaunchCommand::executable(if cfg!(windows) {
        "rundll32.exe"
    } else if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else {
        "xdg-open"
    });
    if cfg!(windows) {
        command.args.push("url.dll,FileProtocolHandler".into());
    }
    command.args.push(uri.into());
    Ok(command)
}

/// Resolve managed payload paths through the stable alias while checking the
/// real target. A stored path never becomes an ambient filesystem capability.
pub fn managed_path(
    real: &Path,
    alias: Option<&Path>,
    relative: &Path,
    directory: bool,
) -> Result<PathBuf, super::LaunchError> {
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
    {
        return Err(super::LaunchError::Invalid(crate::tr!(
            "managed launch path must be relative and confined"
        )));
    }
    let root = std::fs::canonicalize(real)?;
    let resolved = std::fs::canonicalize(root.join(relative))?;
    if !resolved.starts_with(&root)
        || (directory && !resolved.is_dir())
        || (!directory && !resolved.is_file())
    {
        return Err(super::LaunchError::Invalid(crate::tr!(
            "managed launch target escapes game directory or has wrong type"
        )));
    }
    if let Some(alias) = alias {
        if std::fs::canonicalize(alias)? != root {
            return Err(super::LaunchError::Invalid(crate::tr!(
                "game alias points at a different directory"
            )));
        }
        Ok(alias.join(relative))
    } else {
        Ok(real.join(relative))
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
