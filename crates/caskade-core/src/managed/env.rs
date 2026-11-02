//! A managed program started from Caskade must not inherit Caskade's own
//! AppImage runtime: an unpacked AppImage's `AppRun` takes `$APPDIR` as
//! its own folder when it is set, so Moonlight's would look for itself in
//! Caskade's mount. Library and plugin paths into that mount would load
//! Caskade's libraries into it too.

use std::ffi::OsString;

/// The AppImage runtime's own variables.
const RUNTIME: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// The variables of `vars` to drop: the runtime's, and any whose value
/// points into `$APPDIR`.
pub fn appimage_vars(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<OsString> {
    let vars: Vec<(OsString, OsString)> = vars.into_iter().collect();
    let appdir = vars
        .iter()
        .find(|(key, _)| key == "APPDIR")
        .map(|(_, value)| value.to_string_lossy().into_owned())
        .filter(|dir| dir.len() > 1);
    vars.into_iter()
        .filter(|(key, value)| {
            RUNTIME.iter().any(|name| key == name)
                || appdir
                    .as_deref()
                    .is_some_and(|dir| value.to_string_lossy().contains(dir))
        })
        .map(|(key, _)| key)
        .collect()
}

/// Removes Caskade's AppImage runtime from `command`'s environment.
pub fn outside_own_appimage(command: &mut std::process::Command) {
    for key in appimage_vars(std::env::vars_os()) {
        command.env_remove(key);
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn caskades_appimage_runtime_stays_behind() {
        let vars = [
            ("APPDIR", "/tmp/.mount_CaskadXYZ"),
            ("APPIMAGE", "/home/u/Caskade.AppImage"),
            ("ARGV0", "Caskade.AppImage"),
            ("OWD", "/home/u"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_CaskadXYZ/usr/lib:/usr/lib"),
            ("GTK_PATH", "/tmp/.mount_CaskadXYZ/usr/lib/gtk-3.0"),
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/u"),
        ]
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));
        let dropped = appimage_vars(vars.clone());
        assert_eq!(
            dropped,
            [
                "APPDIR",
                "APPIMAGE",
                "ARGV0",
                "OWD",
                "LD_LIBRARY_PATH",
                "GTK_PATH"
            ]
            .map(OsString::from)
        );
        // Not an AppImage: nothing but stray runtime names.
        let plain = vars
            .into_iter()
            .filter(|(key, _)| key == "PATH" || key == "HOME");
        assert!(appimage_vars(plain).is_empty());
    }
}
