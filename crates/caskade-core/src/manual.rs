//! Games added by hand (#174): a title-only record (a boxed or console game
//! Caskade can't start), or a game already on disk or behind a URI that
//! Caskade starts where it is. Nothing is copied, moved or run when one is
//! added: the record is a `manual` entry ([`MANUAL_SOURCE`]) whose built-in
//! launch is the target, the same model named actions build on (#177).
//! Shortcuts keep their arguments, folder and environment; executables are
//! started directly, never through a shell. Removing such a game from the
//! library never touches its files.

use crate::{Error, Result, install::dropped, launch::validate_target, model::*};
use caskade_discovery::{GameRuntime, Prefix};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// What a game added by hand starts, worked out from what the user chose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualTarget {
    /// The title the target suggests: a shortcut's name, a program's file
    /// name; empty for a URI.
    pub title: String,
    pub launch: LaunchSpec,
    /// The folder the game is in, for Show in folder; none for a URI.
    pub folder: Option<PathBuf>,
    /// The program is a Windows one, started through a compatibility tool
    /// off Windows.
    pub windows: bool,
}

/// A game already in the library that may be the same one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGame {
    pub game_id: String,
    pub title: String,
    pub reason: DuplicateReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateReason {
    /// One of its releases starts the same program or URI.
    SameTarget,
    /// It has the same title (or one of its other names is).
    SameTitle,
}

/// A game to add by hand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualGame {
    pub title: String,
    /// What it starts; none for a title-only record.
    pub target: Option<ManualTarget>,
    /// Adds the target as a release of this game instead of a new game.
    pub link_to: Option<String>,
    /// A new game the user said isn't one of the duplicates: IGDB's
    /// same-game merging leaves it apart.
    #[serde(default)]
    pub keep_apart: bool,
}

const SHORTCUTS: [&str; 3] = ["lnk", "url", "desktop"];
const WINDOWS_PROGRAMS: [&str; 4] = ["exe", "bat", "cmd", "com"];

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|known| e.eq_ignore_ascii_case(known)))
}

/// Whether `input` is a URI to open (`steam://…`, `https://…`), rather than
/// a path: a scheme of two letters or more, so `C:\…` is a path.
fn is_uri(input: &str) -> bool {
    input.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && scheme != "file"
    })
}

/// The entry `target` would be, for validation and for adding it.
pub(crate) fn entry_for(game_id: &str, target: &ManualTarget) -> GameEntry {
    let mut entry = GameEntry::new(
        game_id,
        caskade_discovery::SourceId::new(MANUAL_SOURCE),
        new_id(),
    );
    entry.state = InstallState::Installed;
    entry.location = target
        .folder
        .clone()
        .map(|path| InstallLocation::External { path });
    entry.launch = Some(target.launch.clone());
    if target.windows {
        let prefix = match &target.launch {
            LaunchSpec::Executable { env, .. } => env.get("WINEPREFIX").map(|path| Prefix {
                path: PathBuf::from(path),
                arch: None,
                owner: None,
            }),
            _ => None,
        };
        entry.runtime = GameRuntime::Windows {
            prefix,
            runner: None,
        };
    }
    entry
}

/// What the path or URI the user chose starts: a shortcut's target with
/// its arguments, folder and environment, a program as it is, or a URI.
/// Nothing is run or copied; a folder is refused (choose its program).
pub fn resolve_target(input: &str) -> Result<ManualTarget> {
    let input = input.trim();
    let target = if is_uri(input) {
        ManualTarget {
            title: String::new(),
            launch: LaunchSpec::Uri {
                uri: input.to_owned(),
            },
            folder: None,
            windows: false,
        }
    } else {
        let path = Path::new(input);
        if !crate::model::validation::absolute(path) {
            return Err(Error::Invalid(crate::tr!(
                "choose the game's program, shortcut or URI"
            )));
        }
        let metadata = std::fs::metadata(path)?;
        if metadata.is_dir() {
            return Err(Error::Invalid(crate::tr!(
                "{path} is a folder: choose the game's program or a shortcut in it",
                path = path.display()
            )));
        }
        if has_extension(path, &SHORTCUTS) {
            let shortcut = dropped::shortcut_target(path)?;
            let (folder, windows) = match &shortcut.launch {
                LaunchSpec::Executable { path, cwd, env, .. } => (
                    cwd.clone().or_else(|| path.parent().map(Path::to_path_buf)),
                    has_extension(path, &WINDOWS_PROGRAMS) || env.contains_key("WINEPREFIX"),
                ),
                _ => (None, false),
            };
            ManualTarget {
                title: shortcut.title,
                launch: shortcut.launch,
                folder,
                windows,
            }
        } else {
            ManualTarget {
                title: path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                launch: LaunchSpec::Executable {
                    path: path.to_path_buf(),
                    args: vec![],
                    cwd: None,
                    env: Default::default(),
                },
                folder: path.parent().map(Path::to_path_buf),
                windows: has_extension(path, &WINDOWS_PROGRAMS),
            }
        }
    };
    validate_target(&entry_for("check", &target), &target.launch)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Ok(target)
}

#[cfg(test)]
#[path = "manual_tests.rs"]
mod tests;
