//! A program started from inside an AppImage must not inherit the AppImage's
//! runtime (#274). The runtime exports `APPDIR`, `APPIMAGE`, `ARGV0` and
//! `OWD`, and its hooks point library, plugin, data and program paths into
//! the mounted image (`LD_LIBRARY_PATH`, `GTK_PATH`, `XDG_DATA_DIRS`,
//! `PATH`, …). A game, emulator or system tool that inherits them loads the
//! host's bundled libraries instead of the system's and can crash; an
//! unpacked AppImage's `AppRun` would even take the host's `$APPDIR` for its
//! own folder.
//!
//! [`outside_appimage`] gives a child the environment the host was started
//! with: the runtime's own variables go, and in every other variable only
//! the entries that point into `$APPDIR` go, so what the user set (their own
//! `LD_LIBRARY_PATH` entries, the rest of `PATH`) is kept. A variable left
//! with nothing is removed. Outside an AppImage nothing changes. A variable
//! the caller set on the command itself is never touched.
//!
//! A hook that replaced a variable outright (rather than prepending to it)
//! leaves no trace of the user's value; such a variable is removed, and the
//! child falls back to the system default.

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// The AppImage runtime's own variables.
const RUNTIME: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// What to do to one inherited variable.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EnvChange {
    Remove(OsString),
    Set(OsString, OsString),
}

/// The changes that take `vars` (a host environment) outside its AppImage:
/// empty unless `APPDIR` is set to a folder.
fn appimage_changes(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<EnvChange> {
    let vars: Vec<(OsString, OsString)> = vars.into_iter().collect();
    let Some(appdir) = vars
        .iter()
        .find(|(key, _)| key == "APPDIR")
        .map(|(_, value)| value.to_string_lossy().trim_end_matches('/').to_owned())
        .filter(|dir| dir.len() > 1)
    else {
        return Vec::new();
    };
    let inside = |entry: &str| entry.contains(&appdir);
    let mut changes = Vec::new();
    for (key, value) in vars {
        if RUNTIME.iter().any(|name| key == *name) {
            changes.push(EnvChange::Remove(key));
            continue;
        }
        let text = value.to_string_lossy();
        if !inside(&text) {
            continue;
        }
        let kept: Vec<&str> = text
            .split(':')
            .filter(|entry| !entry.is_empty() && !inside(entry))
            .collect();
        changes.push(if kept.is_empty() {
            EnvChange::Remove(key)
        } else {
            EnvChange::Set(key, kept.join(":").into())
        });
    }
    changes
}

/// Applies `changes` to `command`, leaving what it set itself.
fn apply_changes(command: &mut Command, changes: &[EnvChange]) {
    let own: Vec<OsString> = command
        .get_envs()
        .map(|(key, _)| key.to_os_string())
        .collect();
    let mine = |key: &OsStr| own.iter().any(|set| set == key);
    for change in changes {
        match change {
            EnvChange::Remove(key) if !mine(key) => {
                command.env_remove(key);
            }
            EnvChange::Set(key, value) if !mine(key) => {
                command.env(key, value);
            }
            _ => {}
        }
    }
}

/// Starts `command` outside this process's AppImage, if it runs in one.
pub fn outside_appimage(command: &mut Command) {
    apply_changes(command, &appimage_changes(std::env::vars_os()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
    }

    const MOUNT: &str = "/tmp/.mount_CaskadXYZ";

    #[test]
    fn the_runtime_goes_and_the_users_own_entries_stay() {
        let host = vars(&[
            ("APPDIR", MOUNT),
            ("APPIMAGE", "/home/u/Caskade.AppImage"),
            ("ARGV0", "Caskade.AppImage"),
            ("OWD", "/home/u"),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_CaskadXYZ/usr/lib:/opt/mine/lib",
            ),
            ("GTK_PATH", "/tmp/.mount_CaskadXYZ/usr/lib/gtk-3.0"),
            ("PATH", "/tmp/.mount_CaskadXYZ/usr/bin:/usr/bin:/bin"),
            ("XDG_DATA_DIRS", "/tmp/.mount_CaskadXYZ/usr/share:"),
            ("HOME", "/home/u"),
            ("MY_SETTING", "on"),
        ]);
        let changes = appimage_changes(host);
        let set = |key: &str, value: &str| EnvChange::Set(key.into(), value.into());
        let remove = |key: &str| EnvChange::Remove(key.into());
        assert_eq!(
            changes,
            [
                remove("APPDIR"),
                remove("APPIMAGE"),
                remove("ARGV0"),
                remove("OWD"),
                set("LD_LIBRARY_PATH", "/opt/mine/lib"),
                remove("GTK_PATH"),
                set("PATH", "/usr/bin:/bin"),
                remove("XDG_DATA_DIRS"),
            ]
        );
    }

    #[test]
    fn outside_an_appimage_nothing_changes() {
        let host = vars(&[
            ("LD_LIBRARY_PATH", "/opt/mine/lib"),
            ("PATH", "/usr/bin"),
            ("OWD", "/home/u"),
        ]);
        assert!(appimage_changes(host).is_empty());
        assert!(appimage_changes(vars(&[("APPDIR", "/")])).is_empty());
    }

    #[test]
    fn what_the_caller_set_on_the_command_is_kept() {
        let changes = appimage_changes(vars(&[
            ("APPDIR", MOUNT),
            ("LD_LIBRARY_PATH", "/tmp/.mount_CaskadXYZ/usr/lib"),
            ("GTK_PATH", "/tmp/.mount_CaskadXYZ/usr/lib/gtk-3.0"),
        ]));
        let mut command = Command::new("true");
        command.env("LD_LIBRARY_PATH", "/game/libs");
        apply_changes(&mut command, &changes);
        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(
            envs,
            [
                ("APPDIR".to_owned(), None),
                ("GTK_PATH".to_owned(), None),
                ("LD_LIBRARY_PATH".to_owned(), Some("/game/libs".to_owned())),
            ]
        );
    }
}
