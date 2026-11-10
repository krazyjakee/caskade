//! Caskade's own program, as the OS should start it later: the login item
//! (`login_item`) and the magnet and `.torrent` handler (the adapter's
//! `torrent_links`) write it down, and so do the streaming host's app
//! entries (`caskade-streaming`'s `publish`) (#114, #115, #116).
//!
//! From an AppImage that's the AppImage file, since the executable inside
//! it is in a mount that moves on every start. `APPIMAGE` names it only
//! when the executable runs inside `APPDIR`, the AppImage's mount: the
//! AppImage runtime exports both to every child, so a Caskade started from
//! another AppImage (a launcher, an editor, an agent host) inherits that
//! one's, and would name it as Caskade.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

/// The program the OS should start for this Caskade.
pub fn current() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if !cfg!(target_os = "linux") {
        return Ok(exe);
    }
    Ok(from(exe, |name| std::env::var_os(name)))
}

/// [`current`] for executable `exe`, with the environment read through
/// `var` (tests pass their own).
pub fn from(exe: PathBuf, var: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    resolve(exe, var("APPIMAGE"), var("APPDIR"))
}

/// `appimage` when `exe` is inside `appdir`, else `exe`.
pub fn resolve(exe: PathBuf, appimage: Option<OsString>, appdir: Option<OsString>) -> PathBuf {
    match (appimage, appdir) {
        (Some(appimage), Some(appdir)) if !appdir.is_empty() && exe.starts_with(&appdir) => {
            PathBuf::from(appimage)
        }
        _ => exe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_real_appimage_is_started_as_itself() {
        assert_eq!(
            resolve(
                PathBuf::from("/tmp/.mount_CaskadXyz/usr/bin/caskade"),
                Some("/home/p/Caskade.AppImage".into()),
                Some("/tmp/.mount_CaskadXyz".into())
            ),
            Path::new("/home/p/Caskade.AppImage")
        );
    }

    #[test]
    fn an_inherited_appimage_is_ignored() {
        // A deb install started from another AppImage (a launcher).
        assert_eq!(
            resolve(
                PathBuf::from("/usr/bin/caskade"),
                Some("/home/p/Launcher.AppImage".into()),
                Some("/tmp/.mount_LauncherAbc".into())
            ),
            Path::new("/usr/bin/caskade")
        );
        // `APPIMAGE` alone, or an empty `APPDIR`, proves nothing.
        assert_eq!(
            resolve(
                PathBuf::from("/usr/bin/caskade"),
                Some("/home/p/Launcher.AppImage".into()),
                None
            ),
            Path::new("/usr/bin/caskade")
        );
        assert_eq!(
            resolve(
                PathBuf::from("/usr/bin/caskade"),
                Some("/home/p/Launcher.AppImage".into()),
                Some("".into())
            ),
            Path::new("/usr/bin/caskade")
        );
    }

    #[test]
    fn a_mount_named_like_the_appdir_is_not_inside_it() {
        // `starts_with` compares whole components, not text.
        assert_eq!(
            resolve(
                PathBuf::from("/tmp/.mount_CaskadXyz2/usr/bin/caskade"),
                Some("/home/p/Other.AppImage".into()),
                Some("/tmp/.mount_CaskadXyz".into())
            ),
            Path::new("/tmp/.mount_CaskadXyz2/usr/bin/caskade")
        );
    }

    #[test]
    fn without_appimage_variables_it_is_the_executable() {
        assert_eq!(
            resolve(PathBuf::from("/opt/caskade/caskade"), None, None),
            Path::new("/opt/caskade/caskade")
        );
    }
}
