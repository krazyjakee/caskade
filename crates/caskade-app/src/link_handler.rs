//! `caskade://` links opened by the OS (#88): which app the OS starts for
//! them. Installers register the scheme for the app they install; a copy
//! that wasn't installed (the native app before its own installer, a
//! development build) registers itself only when no app handles the
//! scheme yet, so it never takes it from an installed Caskade, or when
//! asked (`--register-links`).
//!
//! - Linux: a desktop entry for `x-scheme-handler/caskade`, made the
//!   default through `xdg-mime`, as torrent links are (`torrent_links`).
//! - Windows: the `caskade` URL protocol under HKEY_CURRENT_USER.
//! - macOS: the app bundle's `CFBundleURLTypes`; nothing to do at run time.

/// The argument that registers this copy for `caskade://` links.
pub const REGISTER_ARG: &str = "--register-links";

/// Whether this copy should take `caskade://` links: asked to, or no app
/// has them. Blocking: it may run `xdg-mime` or read the registry.
pub fn wanted(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == REGISTER_ARG) || platform::handler().is_none()
}

/// Makes this copy the app the OS starts for `caskade://` links.
pub fn register() -> Result<(), String> {
    platform::register()
}

/// The app that takes `caskade://` links now, as the OS names it.
pub fn handler() -> Option<String> {
    platform::handler()
}

#[cfg(target_os = "linux")]
mod platform {
    use crate::torrent_links::linux::{Mime, Xdg, xdg_dir};

    /// The desktop entry; `NoDisplay`, so menus don't list it.
    pub const FILE_NAME: &str = "caskade-links.desktop";
    const MIME: &str = "x-scheme-handler/caskade";

    pub fn entry(program: &str) -> String {
        let quoted = program.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "[Desktop Entry]\nType=Application\nName=Caskade\nNoDisplay=true\nExec=\"{quoted}\" %u\nMimeType={MIME};\n"
        )
    }

    pub fn handler() -> Option<String> {
        Xdg.default_app(MIME)
    }

    pub fn register() -> Result<(), String> {
        let program = crate::torrent_links::program()?;
        let applications = xdg_dir("XDG_DATA_HOME", ".local/share")?.join("applications");
        std::fs::create_dir_all(&applications).map_err(|error| error.to_string())?;
        std::fs::write(applications.join(FILE_NAME), entry(&program))
            .map_err(|error| error.to_string())?;
        Xdg.refresh(&applications);
        Xdg.set_default(FILE_NAME, MIME)
            .map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
mod platform {
    use windows_registry::CURRENT_USER;

    const KEY: &str = r"Software\Classes\caskade";

    pub fn handler() -> Option<String> {
        CURRENT_USER
            .open(format!(r"{KEY}\shell\open\command"))
            .ok()?
            .get_string("")
            .ok()
    }

    pub fn register() -> Result<(), String> {
        let program = crate::torrent_links::program()?;
        let set = || -> windows_registry::Result<()> {
            let key = CURRENT_USER.create(KEY)?;
            key.set_string("", "URL:Caskade")?;
            key.set_string("URL Protocol", "")?;
            CURRENT_USER
                .create(format!(r"{KEY}\DefaultIcon"))?
                .set_string("", format!("{program},0"))?;
            CURRENT_USER
                .create(format!(r"{KEY}\shell\open\command"))?
                .set_string("", format!("\"{program}\" \"%1\""))
        };
        set().map_err(|error| error.to_string())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    /// The bundle declares the scheme; Launch Services knows its app.
    pub fn handler() -> Option<String> {
        Some("the app bundle".to_owned())
    }

    pub fn register() -> Result<(), String> {
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn the_entry_starts_this_program_with_the_link() {
        let entry = super::platform::entry("/opt/My Games/caskade-native");
        assert!(
            entry.contains("Exec=\"/opt/My Games/caskade-native\" %u\n"),
            "{entry}"
        );
        assert!(entry.contains("MimeType=x-scheme-handler/caskade;\n"));
        assert!(entry.contains("NoDisplay=true\n"));
    }
}
