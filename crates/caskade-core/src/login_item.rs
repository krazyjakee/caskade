//! Starting Caskade when the user logs in (#114): the login item that
//! Settings → General → Startup's Launch on system startup
//! (`core.general.launchOnLogin`) asks for, and what a start through it
//! does with the window.
//!
//! The item starts Caskade with [`LOGIN_ARG`], so a login start can stay
//! in the tray when Start minimized is on too ([`starts_hidden`]); a start
//! by the user always shows the window.
//!
//! - Linux: an XDG autostart entry, `$XDG_CONFIG_HOME/autostart/caskade.desktop`
//!   (`~/.config/autostart`), which X11 and Wayland sessions alike start.
//!   From an AppImage it starts the AppImage, not the executable mounted
//!   inside it. In a Flatpak the sandbox can't write there: the Background
//!   portal writes it for the app ([`portal_request`]).
//! - Windows: the current user's `Run` key, value `Caskade`, written with
//!   `reg.exe` (the core links no registry crate).
//! - macOS: a launch agent, `~/Library/LaunchAgents/dev.caskade.app.login.plist`,
//!   which System Settings → Login Items lists under Allow in the Background.
//!   It opens the app bundle through `open`, so Launch Services starts it as
//!   any other.
//!
//! [`follow`] keeps the item in step with the setting: at start, when the
//! setting is on, it rewrites an item that names another path (a moved or
//! updated install), and it registers or removes the item when the setting
//! changes. Nothing is written for anyone who never switched it on, and the
//! portal and `reg.exe` are asked only on a change or while it is on.

use crate::settings::{GeneralSettings, SettingsService};
use std::io;
use std::path::{Path, PathBuf};

/// The argument the login item starts Caskade with.
pub const LOGIN_ARG: &str = "--login";

/// The launch agent's label and file name (macOS).
pub const AGENT_LABEL: &str = "dev.caskade.app.login";

/// The `Run` key's value name (Windows).
pub const RUN_VALUE: &str = "Caskade";

/// The current user's `Run` key (Windows).
pub const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Whether this process was started by the login item.
pub fn started_at_login(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == LOGIN_ARG)
}

/// Whether a start leaves the window hidden in the tray: started by the
/// login item, with Launch on system startup and Start minimized both on.
/// The caller still shows the window when there's no tray to bring it
/// back from.
pub fn starts_hidden(args: &[String], general: &GeneralSettings) -> bool {
    started_at_login(args) && general.launch_on_login && general.start_minimized
}

/// Where the login item lives, and how it's written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginItem {
    /// An XDG autostart entry at `path` that runs `program`.
    Autostart { path: PathBuf, program: PathBuf },
    /// The Background portal, from a Flatpak; `command` is the command
    /// inside the sandbox.
    Portal { command: String },
    /// The current user's `Run` key (Windows).
    RunKey { program: PathBuf },
    /// A launch agent at `path` (macOS). `program` is the app bundle, or the
    /// bare executable outside one.
    LaunchAgent { path: PathBuf, program: PathBuf },
}

impl LoginItem {
    /// This system's login item for the running Caskade, if it has one.
    pub fn live() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let var = |name: &str| std::env::var_os(name);
        let dirs = directories::BaseDirs::new()?;
        if cfg!(windows) {
            return Some(Self::RunKey { program: exe });
        }
        if cfg!(target_os = "macos") {
            return Some(Self::LaunchAgent {
                path: dirs
                    .home_dir()
                    .join("Library/LaunchAgents")
                    .join(format!("{AGENT_LABEL}.plist")),
                program: app_bundle(&exe).unwrap_or(exe),
            });
        }
        if cfg!(target_os = "linux") {
            // Only the sandbox has it; `FLATPAK_ID` can leak to a program a
            // Flatpak starts on the host.
            if Path::new("/.flatpak-info").exists() {
                let command = exe.file_name()?.to_str()?.to_owned();
                return Some(Self::Portal { command });
            }
            return Some(Self::Autostart {
                path: autostart_path(dirs.config_dir()),
                program: program(exe, var("APPIMAGE"), var("APPDIR")),
            });
        }
        None
    }

    /// Whether the item can be checked without starting a program: only
    /// these are settled at start while the setting is off.
    fn is_file(&self) -> bool {
        matches!(self, Self::Autostart { .. } | Self::LaunchAgent { .. })
    }

    /// Registers the item (`on`) or removes it. Returns whether anything
    /// changed. Blocking: it writes files or runs a program.
    pub fn apply(&self, on: bool) -> io::Result<bool> {
        match self {
            Self::Autostart { path, program } => {
                write_or_remove(path, on.then(|| desktop_entry(program)))
            }
            Self::LaunchAgent { path, program } => {
                write_or_remove(path, on.then(|| launch_agent(program)))
            }
            Self::RunKey { program } => run_key(on, &run_command(program)),
            Self::Portal { command } => {
                let args = portal_request(on, command);
                let status = crate::quiet::command("gdbus")
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .status()?;
                if status.success() {
                    Ok(true)
                } else {
                    Err(io::Error::other(format!("gdbus failed: {status}")))
                }
            }
        }
    }

    /// What a start does: with the setting on, rewrite an item that differs;
    /// with it off, remove a leftover file. A portal or the registry is left
    /// alone while it's off, so nothing runs for anyone who never switched
    /// it on.
    pub fn settle_at_start(&self, on: bool) -> io::Result<bool> {
        if on || self.is_file() {
            self.apply(on)
        } else {
            Ok(false)
        }
    }
}

/// Keeps `item` in step with `core.general.launchOnLogin`: settled once at
/// start, then on every change of the setting. Runs until the settings
/// service goes away.
pub async fn follow(settings: SettingsService, item: LoginItem) {
    use tokio::sync::broadcast::error::RecvError;
    let mut changes = settings.subscribe();
    let mut on = general(&settings).await.launch_on_login;
    settle(&item, on, true).await;
    loop {
        match changes.recv().await {
            Ok(changed) if changed.namespace == "core.general" => {}
            Err(RecvError::Lagged(_)) => {}
            Ok(_) => continue,
            Err(RecvError::Closed) => break,
        }
        let now = general(&settings).await.launch_on_login;
        if now != on {
            on = now;
            settle(&item, on, false).await;
        }
    }
}

async fn general(settings: &SettingsService) -> GeneralSettings {
    settings.read_or_default("core.general").await
}

async fn settle(item: &LoginItem, on: bool, start: bool) {
    let blocking = item.clone();
    let done = tokio::task::spawn_blocking(move || {
        if start {
            blocking.settle_at_start(on)
        } else {
            blocking.apply(on)
        }
    })
    .await
    .unwrap_or_else(|error| Err(io::Error::other(error)));
    match done {
        Ok(true) => tracing::info!(on, ?item, "login item updated"),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, on, ?item, "couldn't update the login item"),
    }
}

/// What the autostart entry runs: the AppImage when Caskade runs from one,
/// since the executable inside it moves on every start. `APPIMAGE` counts
/// only when the executable is inside `APPDIR`, the AppImage's mount: a
/// program started from another AppImage (a launcher, a terminal) inherits
/// that one's variables.
pub fn program(
    exe: PathBuf,
    appimage: Option<std::ffi::OsString>,
    appdir: Option<std::ffi::OsString>,
) -> PathBuf {
    match (appimage, appdir) {
        (Some(appimage), Some(appdir)) if exe.starts_with(&appdir) => PathBuf::from(appimage),
        _ => exe,
    }
}

/// `<config>/autostart/caskade.desktop`.
pub fn autostart_path(config: &Path) -> PathBuf {
    config.join("autostart").join("caskade.desktop")
}

/// The autostart entry that starts `program` with [`LOGIN_ARG`].
pub fn desktop_entry(program: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Caskade\n\
         Comment={comment}\n\
         Exec={exec} {LOGIN_ARG}\n\
         Icon=caskade\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        comment = single_line(&crate::tr!("Start Caskade when you log in")),
        exec = exec_arg(&program.to_string_lossy()),
    )
}

/// A desktop entry's string value stays on its line.
fn single_line(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// One `Exec` argument: quoted when it has a reserved character, with `"`,
/// `` ` ``, `$` and `\` escaped inside the quotes, then the value's own
/// string escape (`\` doubled) and `%` doubled, as the Desktop Entry
/// specification has it.
pub fn exec_arg(arg: &str) -> String {
    const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";
    let quoted = if arg.is_empty() || arg.chars().any(|c| RESERVED.contains(c)) {
        let mut inner = String::with_capacity(arg.len() + 2);
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                inner.push('\\');
            }
            inner.push(c);
        }
        format!("\"{inner}\"")
    } else {
        arg.to_owned()
    };
    quoted.replace('\\', "\\\\").replace('%', "%%")
}

/// The `.app` folder `exe` runs from, if it's inside one.
pub fn app_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|dir| dir.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

/// The launch agent: `open -a <bundle> --args --login`, or the executable
/// itself outside a bundle.
pub fn launch_agent(program: &Path) -> String {
    let program = program.to_string_lossy();
    let arguments: Vec<&str> = if program.ends_with(".app") {
        vec!["/usr/bin/open", "-a", &program, "--args", LOGIN_ARG]
    } else {
        vec![&program, LOGIN_ARG]
    };
    let arguments: String = arguments
        .iter()
        .map(|arg| format!("\t\t<string>{}</string>\n", xml_escape(arg)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>{AGENT_LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         {arguments}\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         \t<key>ProcessType</key>\n\
         \t<string>Interactive</string>\n\
         </dict>\n\
         </plist>\n"
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The `Run` value: the quoted program and [`LOGIN_ARG`].
pub fn run_command(program: &Path) -> String {
    format!("\"{}\" {LOGIN_ARG}", program.display())
}

/// The `gdbus` arguments that ask the Background portal to start Caskade
/// at login (`on`) or not.
pub fn portal_request(on: bool, command: &str) -> Vec<String> {
    let reason = gvariant_string(&crate::tr!("Start Caskade when you log in"));
    let options = format!(
        "{{'reason': <{reason}>, 'autostart': <{on}>, 'commandline': <[{command}, {login}]>, 'dbus-activatable': <false>}}",
        command = gvariant_string(command),
        login = gvariant_string(LOGIN_ARG),
    );
    [
        "call",
        "--session",
        "--dest",
        "org.freedesktop.portal.Desktop",
        "--object-path",
        "/org/freedesktop/portal/desktop",
        "--method",
        "org.freedesktop.portal.Background.RequestBackground",
        "",
        &options,
    ]
    .map(String::from)
    .to_vec()
}

/// A GVariant text-format string.
fn gvariant_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        match c {
            '\'' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Writes `contents` to `path` unless it's already there, or removes the
/// file when `contents` is `None`. Returns whether anything changed.
fn write_or_remove(path: &Path, contents: Option<String>) -> io::Result<bool> {
    match contents {
        Some(contents) => {
            if std::fs::read_to_string(path).is_ok_and(|now| now == contents) {
                return Ok(false);
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let staged = path.with_extension("tmp");
            std::fs::write(&staged, contents)?;
            std::fs::rename(&staged, path)?;
            Ok(true)
        }
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        },
    }
}

/// The `Run` value `reg.exe query` printed, if any.
pub fn run_value(query: &str) -> Option<String> {
    query.lines().find_map(|line| {
        let line = line.trim_start();
        let rest = line.strip_prefix(RUN_VALUE)?;
        let rest = rest.trim_start().strip_prefix("REG_SZ")?;
        Some(rest.trim().to_owned())
    })
}

/// Sets or deletes the `Run` value, through `reg.exe`.
fn run_key(on: bool, command: &str) -> io::Result<bool> {
    use std::process::Stdio;
    let query = crate::quiet::command("reg.exe")
        .args(["query", RUN_KEY, "/v", RUN_VALUE])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    let now = run_value(&String::from_utf8_lossy(&query.stdout));
    let args: Vec<&str> = match (on, now.as_deref()) {
        (true, Some(now)) if now == command => return Ok(false),
        (true, _) => vec![
            "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", command, "/f",
        ],
        (false, None) => return Ok(false),
        (false, Some(_)) => vec!["delete", RUN_KEY, "/v", RUN_VALUE, "/f"],
    };
    let status = crate::quiet::command("reg.exe")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(true)
    } else {
        Err(io::Error::other(format!("reg.exe failed: {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn only_the_login_item_start_counts_as_a_login_start() {
        assert!(started_at_login(&args(&["caskade", "--login"])));
        assert!(!started_at_login(&args(&["caskade"])));
        // The program's own name isn't an argument.
        assert!(!started_at_login(&args(&["--login"])));
    }

    #[test]
    fn a_login_start_stays_hidden_only_with_both_settings_on() {
        let login = args(&["caskade", "--login"]);
        let mut general = GeneralSettings::default();
        assert!(!starts_hidden(&login, &general), "defaults show the window");
        general.start_minimized = true;
        assert!(!starts_hidden(&login, &general), "needs launch on login");
        general.launch_on_login = true;
        assert!(starts_hidden(&login, &general));
        assert!(
            !starts_hidden(&args(&["caskade"]), &general),
            "a start by the user always shows the window"
        );
    }

    #[test]
    fn an_appimage_is_started_as_itself_but_not_a_parents() {
        let inside = PathBuf::from("/tmp/.mount_CaskadXyz/usr/bin/caskade");
        assert_eq!(
            program(
                inside.clone(),
                Some("/home/p/Caskade.AppImage".into()),
                Some("/tmp/.mount_CaskadXyz".into())
            ),
            Path::new("/home/p/Caskade.AppImage")
        );
        // A deb install started from another AppImage (a launcher).
        assert_eq!(
            program(
                PathBuf::from("/usr/bin/caskade"),
                Some("/home/p/Launcher.AppImage".into()),
                Some("/tmp/.mount_LauncherAbc".into())
            ),
            Path::new("/usr/bin/caskade")
        );
        assert_eq!(program(inside.clone(), None, None), inside);
    }

    #[test]
    fn the_autostart_entry_lives_in_the_config_folder() {
        assert_eq!(
            autostart_path(Path::new("/home/player/.config")),
            Path::new("/home/player/.config/autostart/caskade.desktop")
        );
    }

    #[test]
    fn the_autostart_entry_starts_the_program_with_the_login_argument() {
        let entry = desktop_entry(Path::new("/usr/bin/caskade"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("\nType=Application\n"));
        assert!(entry.contains("\nExec=/usr/bin/caskade --login\n"));
        assert!(entry.contains("\nX-GNOME-Autostart-enabled=true\n"));
        assert!(!entry.contains("Hidden="));
    }

    #[test]
    fn exec_arguments_are_quoted_and_escaped() {
        assert_eq!(exec_arg("/opt/caskade"), "/opt/caskade");
        assert_eq!(
            exec_arg("/home/p/My Games/Caskade.AppImage"),
            "\"/home/p/My Games/Caskade.AppImage\""
        );
        // `$` escaped inside the quotes, then that backslash doubled.
        assert_eq!(exec_arg("/a$b"), "\"/a\\\\$b\"");
        assert_eq!(exec_arg("/100%"), "/100%%");
        assert_eq!(exec_arg(""), "\"\"");
    }

    #[test]
    fn writing_and_removing_the_entry_reports_changes() {
        let dir = tempfile::tempdir().unwrap();
        let item = LoginItem::Autostart {
            path: autostart_path(dir.path()),
            program: PathBuf::from("/usr/bin/caskade"),
        };
        assert!(!item.settle_at_start(false).unwrap(), "nothing to remove");
        assert!(!dir.path().join("autostart").exists(), "nothing written");
        assert!(item.apply(true).unwrap());
        assert!(!item.apply(true).unwrap(), "already there");
        let moved = LoginItem::Autostart {
            path: autostart_path(dir.path()),
            program: PathBuf::from("/home/p/Caskade.AppImage"),
        };
        assert!(moved.settle_at_start(true).unwrap(), "a moved install");
        let written = std::fs::read_to_string(autostart_path(dir.path())).unwrap();
        assert!(written.contains("Exec=/home/p/Caskade.AppImage --login"));
        assert!(
            moved.settle_at_start(false).unwrap(),
            "a leftover is removed"
        );
        assert!(!autostart_path(dir.path()).exists());
        assert!(!moved.apply(false).unwrap());
    }

    #[tokio::test]
    async fn the_item_follows_the_setting() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::db::LibraryStore::in_memory().await.unwrap();
        let settings = SettingsService::new(store);
        settings
            .register_core(&crate::AppPaths::under(temp.path()))
            .unwrap();
        let path = autostart_path(&temp.path().join("config"));
        let item = LoginItem::Autostart {
            path: path.clone(),
            program: PathBuf::from("/usr/bin/caskade"),
        };
        let following = tokio::spawn(follow(settings.clone(), item));
        let set = |on: bool| {
            let settings = settings.clone();
            async move {
                let now = settings.get("core.general").await.unwrap();
                let mut value = now.value;
                value["launchOnLogin"] = serde_json::Value::Bool(on);
                settings
                    .set("core.general", value, Some(now.revision))
                    .await
                    .unwrap();
            }
        };
        let until = |present: bool| {
            let path = path.clone();
            async move {
                for _ in 0..200 {
                    if path.exists() == present {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                panic!("the entry's presence never became {present}");
            }
        };
        set(true).await;
        until(true).await;
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("Exec=/usr/bin/caskade --login")
        );
        set(false).await;
        until(false).await;
        following.abort();
    }

    #[test]
    fn the_launch_agent_opens_the_bundle_at_load() {
        let exe = Path::new("/Applications/Caskade.app/Contents/MacOS/caskade");
        let bundle = app_bundle(exe).unwrap();
        assert_eq!(bundle, Path::new("/Applications/Caskade.app"));
        let plist = launch_agent(&bundle);
        assert!(plist.contains("<string>dev.caskade.app.login</string>"));
        assert!(plist.contains(
            "<string>/usr/bin/open</string>\n\t\t<string>-a</string>\n\t\t<string>/Applications/Caskade.app</string>\n\t\t<string>--args</string>\n\t\t<string>--login</string>"
        ));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        let bare = launch_agent(Path::new("/tmp/R&D/caskade"));
        assert!(
            bare.contains("<string>/tmp/R&amp;D/caskade</string>\n\t\t<string>--login</string>")
        );
        assert!(app_bundle(Path::new("/usr/local/bin/caskade")).is_none());
    }

    #[test]
    fn the_run_value_quotes_the_program() {
        assert_eq!(
            run_command(Path::new(r"C:\Users\p\AppData\Local\Caskade\caskade.exe")),
            r#""C:\Users\p\AppData\Local\Caskade\caskade.exe" --login"#
        );
    }

    #[test]
    fn reg_query_output_gives_the_run_value() {
        let output = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    Caskade    REG_SZ    \"C:\\Program Files\\Caskade\\caskade.exe\" --login\r\n\r\n";
        assert_eq!(
            run_value(output).as_deref(),
            Some(r#""C:\Program Files\Caskade\caskade.exe" --login"#)
        );
        assert_eq!(
            run_value(
                "ERROR: The system was unable to find the specified registry key or value.\r\n"
            ),
            None
        );
        // Another program's value that starts with the same name.
        assert_eq!(run_value("    CaskadeHelper    REG_SZ    x\r\n"), None);
    }

    #[test]
    fn the_portal_request_names_the_command_and_the_choice() {
        let on = portal_request(true, "caskade");
        assert_eq!(on[7], "org.freedesktop.portal.Background.RequestBackground");
        assert_eq!(on[8], "", "no parent window");
        assert!(on[9].contains("'autostart': <true>"));
        assert!(on[9].contains("'commandline': <['caskade', '--login']>"));
        assert!(portal_request(false, "caskade")[9].contains("'autostart': <false>"));
        assert_eq!(gvariant_string("it's"), r"'it\'s'");
    }
}
