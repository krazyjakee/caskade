//! Magnet links and `.torrent` files from the OS (#159). Caskade installs
//! them like any other link; on request, the OS also sends it the ones the
//! browser and the file manager open. Linux makes Caskade their default
//! app; Windows only offers it in Default apps, where the user chooses.
//! Turning it off hands them back: on Linux, to the app that had them
//! before. macOS isn't supported yet.
//!
//! The OS starts `caskade <magnet link | .torrent file>`, or forwards that
//! to the running Caskade ([`input_from_args`]), and the link becomes an
//! install request like a deep link (`deep_link::queue`).

use serde::Serialize;
use specta::Type;
use std::path::{Path, PathBuf};

/// The magnet link or `.torrent` file the OS started Caskade with: its
/// one argument, a `magnet:` link, a path, or a `file://` or HTTP(S) URL
/// of a `.torrent`. Anything else, or more arguments, is a normal start.
pub fn input_from_args(args: &[String], cwd: &Path) -> Option<String> {
    let [_, arg] = args else {
        return None;
    };
    let starts = |prefix: &str| {
        arg.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    if starts("magnet:") {
        return Some(arg.clone());
    }
    let torrent = |path: &Path| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("torrent"))
    };
    // Linux's `%u` passes a local file as a path or as a `file://` URL, and
    // a remote one as its URL.
    if starts("http://") || starts("https://") {
        let url = url::Url::parse(arg).ok()?;
        return torrent(Path::new(url.path())).then(|| arg.clone());
    }
    let path = if starts("file:") {
        url::Url::parse(arg).ok()?.to_file_path().ok()?
    } else if arg.contains("://") {
        return None;
    } else {
        PathBuf::from(arg)
    };
    torrent(&path)
        .then(|| cwd.join(path).into_os_string().into_string().ok())
        .flatten()
}

/// How Caskade can take magnet links and `.torrent` files on this OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase", rename = "TorrentLinksSupport")]
pub enum Support {
    /// Caskade becomes their default app (Linux).
    Direct,
    /// Caskade is offered, and the user chooses it in the OS's settings
    /// (Windows).
    Chooser,
    /// Not on this OS (macOS).
    Unsupported,
}

/// Whether the OS opens magnet links and `.torrent` files with Caskade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentLinks {
    pub support: Support,
    /// Caskade told the OS it opens them.
    pub registered: bool,
    /// The OS opens magnet links with Caskade.
    pub magnet: bool,
    /// The OS opens `.torrent` files with Caskade.
    pub torrent_files: bool,
}

/// The program the OS should start: the AppImage when Caskade runs from
/// one, but not an AppImage it merely inherited (`caskade_core::program`).
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn program() -> Result<String, String> {
    let path = caskade_core::program::current().map_err(|error| error.to_string())?;
    path.into_os_string()
        .into_string()
        .map_err(|_| caskade_core::tr!("Caskade's path isn't valid Unicode"))
}

/// What the OS does with them now. Blocking: it may run `xdg-mime`.
pub fn status() -> Result<TorrentLinks, String> {
    #[cfg(target_os = "linux")]
    {
        linux::Handler::live()?.status()
    }
    #[cfg(windows)]
    {
        Ok(windows::status())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(TorrentLinks {
            support: Support::Unsupported,
            registered: false,
            magnet: false,
            torrent_files: false,
        })
    }
}

/// Asks the OS to open them with Caskade (`open`), or hands them back.
/// On Windows, opening also shows Default apps, where the user chooses.
pub fn set(open: bool) -> Result<TorrentLinks, String> {
    #[cfg(target_os = "linux")]
    {
        let handler = linux::Handler::live()?;
        let changed = if open {
            handler.register()
        } else {
            handler.unregister()
        };
        changed.map_err(|error| {
            caskade_core::tr!(
                "Caskade couldn't change the default apps: {error}",
                error = error
            )
        })?;
        handler.status()
    }
    #[cfg(windows)]
    {
        let changed = if open {
            windows::register(&program()?)
        } else {
            windows::unregister()
        };
        changed.map_err(|error| {
            caskade_core::tr!(
                "Caskade couldn't change the default apps: {error}",
                error = error
            )
        })?;
        if open {
            windows::open_chooser()?;
        }
        Ok(windows::status())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = open;
        Err(caskade_core::tr!(
            "This system doesn't let Caskade open magnet links and torrent files."
        ))
    }
}

/// Windows: opens Default apps at Caskade's page.
pub fn choose() -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::open_chooser()
    }
    #[cfg(not(windows))]
    {
        Err(caskade_core::tr!(
            "Only Windows asks you to choose the app in its settings."
        ))
    }
}

/// Points an earlier registration at this copy of Caskade, which may have
/// moved since. Changes nothing when it wasn't registered. Blocking.
pub fn refresh() {
    #[cfg(target_os = "linux")]
    let refreshed = linux::Handler::live()
        .and_then(|handler| handler.refresh().map_err(|error| error.to_string()));
    #[cfg(windows)]
    let refreshed = program().and_then(|program| windows::refresh(&program));
    #[cfg(any(windows, target_os = "linux"))]
    if let Err(error) = refreshed {
        tracing::warn!(%error, "couldn't refresh the magnet link handler");
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod linux {
    use super::{Support, TorrentLinks};
    use std::{
        collections::HashMap,
        fs, io,
        path::{Path, PathBuf},
        process::Command,
    };

    /// The desktop entry that takes them; `NoDisplay`, so menus don't list
    /// it next to Caskade's own.
    pub const FILE_NAME: &str = "caskade-torrent-links.desktop";
    /// Each MIME type, and the key that remembers the app it had before.
    pub const MIMES: [(&str, &str); 2] = [
        ("x-scheme-handler/magnet", "X-Caskade-Previous-Magnet"),
        ("application/x-bittorrent", "X-Caskade-Previous-Torrent"),
    ];

    /// The default apps: `xdg-mime` and `update-desktop-database` live.
    pub trait Mime {
        /// The desktop file that opens `mime` now.
        fn default_app(&self, mime: &str) -> Option<String>;
        fn set_default(&self, file: &str, mime: &str) -> io::Result<()>;
        /// Rebuilds the MIME cache of `applications`.
        fn refresh(&self, applications: &Path);
    }

    pub struct Xdg;

    /// A desktop tool, run outside Caskade's AppImage (#274).
    fn tool(program: &str) -> Command {
        let mut command = Command::new(program);
        caskade_core::managed::outside_own_appimage(&mut command);
        command
    }

    impl Mime for Xdg {
        fn default_app(&self, mime: &str) -> Option<String> {
            let output = tool("xdg-mime")
                .args(["query", "default", mime])
                .output()
                .ok()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let app = stdout.lines().next()?.trim();
            (output.status.success() && !app.is_empty()).then(|| app.to_owned())
        }

        fn set_default(&self, file: &str, mime: &str) -> io::Result<()> {
            let status = tool("xdg-mime").args(["default", file, mime]).status()?;
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!("xdg-mime failed: {status}")))
            }
        }

        fn refresh(&self, applications: &Path) {
            // Without it, xdg-mime still writes the defaults.
            if let Err(error) = tool("update-desktop-database").arg(applications).status() {
                tracing::debug!(%error, "update-desktop-database didn't run");
            }
        }
    }

    pub struct Handler<M> {
        /// `$XDG_DATA_HOME/applications`.
        pub applications: PathBuf,
        /// `$XDG_CONFIG_HOME/mimeapps.list`.
        pub mimeapps: PathBuf,
        pub program: String,
        pub mime: M,
    }

    /// `$<var>`, or `~/<fallback>`.
    pub(crate) fn xdg_dir(var: &str, fallback: &str) -> Result<PathBuf, String> {
        if let Some(dir) = std::env::var_os(var).map(PathBuf::from)
            && dir.is_absolute()
        {
            return Ok(dir);
        }
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(fallback))
            .ok_or_else(|| caskade_core::tr!("HOME isn't set"))
    }

    impl Handler<Xdg> {
        pub fn live() -> Result<Self, String> {
            Ok(Self {
                applications: xdg_dir("XDG_DATA_HOME", ".local/share")?.join("applications"),
                mimeapps: xdg_dir("XDG_CONFIG_HOME", ".config")?.join("mimeapps.list"),
                program: super::program()?,
                mime: Xdg,
            })
        }
    }

    impl<M: Mime> Handler<M> {
        fn entry(&self) -> PathBuf {
            self.applications.join(FILE_NAME)
        }

        pub fn status(&self) -> Result<TorrentLinks, String> {
            let ours = |mime| self.mime.default_app(mime).as_deref() == Some(FILE_NAME);
            Ok(TorrentLinks {
                support: Support::Direct,
                registered: self.entry().is_file(),
                magnet: ours(MIMES[0].0),
                torrent_files: ours(MIMES[1].0),
            })
        }

        /// Makes Caskade their default app, remembering the apps that had
        /// them.
        pub fn register(&self) -> io::Result<()> {
            let recorded = recorded(&self.entry());
            let previous = MIMES
                .iter()
                .filter_map(|&(mime, key)| {
                    let app = match self.mime.default_app(mime) {
                        Some(app) if app != FILE_NAME => app,
                        // Already ours: keep what it had before that.
                        _ => recorded.get(key)?.clone(),
                    };
                    Some((key, app))
                })
                .collect::<Vec<_>>();
            fs::create_dir_all(&self.applications)?;
            fs::write(self.entry(), desktop_entry(&self.program, &previous)?)?;
            self.mime.refresh(&self.applications);
            for (mime, _) in MIMES {
                self.mime.set_default(FILE_NAME, mime)?;
            }
            Ok(())
        }

        /// Hands them back to the apps that had them, and removes the entry.
        pub fn unregister(&self) -> io::Result<()> {
            let recorded = recorded(&self.entry());
            let ours = MIMES
                .iter()
                .filter(|(mime, _)| self.mime.default_app(mime).as_deref() == Some(FILE_NAME))
                .collect::<Vec<_>>();
            match fs::read_to_string(&self.mimeapps) {
                Ok(list) => {
                    if let Some(list) = without(&list, FILE_NAME) {
                        fs::write(&self.mimeapps, list)?;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            for (mime, key) in ours {
                if let Some(app) = recorded.get(*key) {
                    self.mime.set_default(app, mime)?;
                }
            }
            if let Err(error) = fs::remove_file(self.entry())
                && error.kind() != io::ErrorKind::NotFound
            {
                return Err(error);
            }
            self.mime.refresh(&self.applications);
            Ok(())
        }

        /// Rewrites the entry when Caskade moved, keeping the defaults.
        pub fn refresh(&self) -> io::Result<()> {
            let Ok(current) = fs::read_to_string(self.entry()) else {
                return Ok(());
            };
            let recorded = recorded(&self.entry());
            let previous = MIMES
                .iter()
                .filter_map(|&(_, key)| Some((key, recorded.get(key)?.clone())))
                .collect::<Vec<_>>();
            let wanted = desktop_entry(&self.program, &previous)?;
            if current != wanted {
                fs::write(self.entry(), wanted)?;
                self.mime.refresh(&self.applications);
            }
            Ok(())
        }
    }

    /// The entry's `X-Caskade-Previous-*` keys.
    fn recorded(entry: &Path) -> HashMap<String, String> {
        fs::read_to_string(entry)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(key, _)| key.starts_with("X-Caskade-Previous-"))
            .map(|(key, value)| (key.to_owned(), value.trim().to_owned()))
            .collect()
    }

    pub fn desktop_entry(program: &str, previous: &[(&str, String)]) -> io::Result<String> {
        // An `Exec` line can't carry one.
        if program.chars().any(char::is_control) {
            return Err(io::Error::other("Caskade's path has a control character"));
        }
        let mimes = MIMES.map(|(mime, _)| format!("{mime};")).concat();
        let mut entry = format!(
            "[Desktop Entry]\nType=Application\nName=Caskade\nComment=Install games from magnet links and torrent files\nExec={} %u\nTerminal=false\nNoDisplay=true\nMimeType={mimes}\n",
            caskade_core::login_item::exec_arg(program)
        );
        for (key, app) in previous {
            entry.push_str(&format!("{key}={app}\n"));
        }
        Ok(entry)
    }

    /// `list` (a `mimeapps.list`) without `file` in our MIME types' default
    /// and added apps; `None` when it had none. Other lines stay as they are.
    pub fn without(list: &str, file: &str) -> Option<String> {
        let mut group = "";
        let mut changed = false;
        let mut out = String::with_capacity(list.len());
        for line in list.split_inclusive('\n') {
            let trimmed = line.trim();
            if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                group = name;
            } else if matches!(group, "Default Applications" | "Added Associations")
                && let Some((key, value)) = trimmed.split_once('=')
                && MIMES.iter().any(|(mime, _)| *mime == key.trim_end())
                && value.split(';').any(|app| app.trim() == file)
            {
                changed = true;
                let apps = value
                    .split(';')
                    .map(str::trim)
                    .filter(|app| !app.is_empty() && *app != file)
                    .map(|app| format!("{app};"))
                    .collect::<String>();
                if !apps.is_empty() {
                    out.push_str(&format!("{}={apps}\n", key.trim_end()));
                }
                continue;
            }
            out.push_str(line);
        }
        changed.then_some(out)
    }
}

#[cfg(windows)]
mod windows {
    //! Windows lets only the user pick default apps, so Caskade registers
    //! as able to open them (`RegisteredApplications`, its capabilities and
    //! a ProgId for each) and opens Default apps for the user to choose.
    //! All of it is under HKEY_CURRENT_USER and Caskade's own names: other
    //! apps' `magnet` and `.torrent` keys stay as they are.
    use super::{Support, TorrentLinks};
    use windows_registry::{CURRENT_USER, Result};

    const APP: &str = "Caskade";
    const CAPABILITIES: &str = r"Software\dev.caskade.app\Capabilities";
    const MAGNET: &str = "Caskade.magnet";
    const TORRENT: &str = "Caskade.torrent";

    fn classes(prog_id: &str) -> String {
        format!(r"Software\Classes\{prog_id}")
    }

    fn command(program: &str) -> String {
        format!("\"{program}\" \"%1\"")
    }

    pub fn register(program: &str) -> Result<()> {
        for (prog_id, name) in [(MAGNET, "Magnet link"), (TORRENT, "BitTorrent file")] {
            CURRENT_USER
                .create(classes(prog_id))?
                .set_string("", name)?;
            CURRENT_USER
                .create(format!(r"{}\DefaultIcon", classes(prog_id)))?
                .set_string("", format!("{program},0"))?;
            CURRENT_USER
                .create(format!(r"{}\shell\open\command", classes(prog_id)))?
                .set_string("", command(program))?;
        }
        // "Open with" lists Caskade for torrent files.
        CURRENT_USER
            .create(r"Software\Classes\.torrent\OpenWithProgids")?
            .set_string(TORRENT, "")?;
        let capabilities = CURRENT_USER.create(CAPABILITIES)?;
        capabilities.set_string("ApplicationName", APP)?;
        capabilities.set_string(
            "ApplicationDescription",
            "Install games from magnet links and torrent files",
        )?;
        CURRENT_USER
            .create(format!(r"{CAPABILITIES}\URLAssociations"))?
            .set_string("magnet", MAGNET)?;
        CURRENT_USER
            .create(format!(r"{CAPABILITIES}\FileAssociations"))?
            .set_string(".torrent", TORRENT)?;
        CURRENT_USER
            .create(r"Software\RegisteredApplications")?
            .set_string(APP, CAPABILITIES)
    }

    pub fn unregister() -> Result<()> {
        if let Ok(key) = CURRENT_USER.open(r"Software\RegisteredApplications") {
            let _ = key.remove_value(APP);
        }
        if let Ok(key) = CURRENT_USER.open(r"Software\Classes\.torrent\OpenWithProgids") {
            let _ = key.remove_value(TORRENT);
        }
        for path in [CAPABILITIES.to_owned(), classes(MAGNET), classes(TORRENT)] {
            if CURRENT_USER.open(&path).is_ok() {
                CURRENT_USER.remove_tree(&path)?;
            }
        }
        Ok(())
    }

    fn registered_command() -> Option<String> {
        CURRENT_USER
            .open(format!(r"{}\shell\open\command", classes(MAGNET)))
            .and_then(|key| key.get_string(""))
            .ok()
    }

    pub fn refresh(program: &str) -> std::result::Result<(), String> {
        match registered_command() {
            Some(registered) if registered != command(program) => {
                register(program).map_err(|error| error.to_string())
            }
            _ => Ok(()),
        }
    }

    /// The ProgId the user chose in `path`'s `UserChoice` (or the newer
    /// `UserChoiceLatest`).
    fn chosen(path: &str) -> Option<String> {
        ["UserChoiceLatest", "UserChoice"]
            .iter()
            .find_map(|choice| {
                CURRENT_USER
                    .open(format!(r"{path}\{choice}"))
                    .and_then(|key| key.get_string("ProgId"))
                    .ok()
            })
    }

    pub fn status() -> TorrentLinks {
        TorrentLinks {
            support: Support::Chooser,
            registered: registered_command().is_some(),
            magnet: chosen(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\magnet")
                .as_deref()
                == Some(MAGNET),
            torrent_files: chosen(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.torrent",
            )
            .as_deref()
                == Some(TORRENT),
        }
    }

    pub fn open_chooser() -> std::result::Result<(), String> {
        std::process::Command::new("explorer.exe")
            .arg(format!("ms-settings:defaultapps?registeredAppUser={APP}"))
            .spawn()
            .map(drop)
            .map_err(|error| {
                caskade_core::tr!("Caskade couldn't open Default apps: {error}", error = error)
            })
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "torrent_links_tests.rs"]
mod tests;
