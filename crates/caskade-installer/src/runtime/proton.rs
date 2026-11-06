//! Prefix runtimes (INSTALLER-PLAN §5): Proton through umu-launcher on Linux,
//! and a user-configured Wine build on macOS. Each game owns its prefix at
//! `<game>/.caskade/prefix` (or `WINEPREFIX` from the command), where
//! `dosdevices/g:` points at the game directory and `z:` (the whole host
//! filesystem) is removed before any installer or game code runs. Process
//! lifetime, timeouts and tree kills are the native runtime's.
use super::{NativeRuntime, ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

/// Prefix initialisation (wineboot + Proton's setup) can be slow on first use,
/// and umu may also download its runtime then.
const INIT_TIMEOUT: Duration = Duration::from_secs(1800);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launcher {
    /// `umu-run`, with `PROTONPATH` naming a Proton build (or `UMU-Proton`
    /// to let umu fetch and manage it).
    Umu {
        umu_run: PathBuf,
        proton_path: String,
        game_id: String,
    },
    /// A Wine binary (macOS: user-configured build).
    Wine { wine: PathBuf },
}

#[derive(Debug)]
pub struct PrefixRuntime {
    launcher: Launcher,
    native: NativeRuntime,
    /// Prefixes already initialised and isolated by this runtime.
    prepared: Mutex<Vec<PathBuf>>,
}

fn io_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Io {
        message: error.to_string(),
    }
}

impl PrefixRuntime {
    pub fn new(launcher: Launcher) -> Self {
        Self {
            launcher,
            native: NativeRuntime::new(),
            prepared: Mutex::new(vec![]),
        }
    }

    pub fn launcher(&self) -> &Launcher {
        &self.launcher
    }

    /// `WINEPREFIX` from the command, else `<cwd>/.caskade/prefix`.
    pub fn prefix_for(command: &ProcessCommand) -> PathBuf {
        command
            .env
            .get("WINEPREFIX")
            .map(PathBuf::from)
            .unwrap_or_else(|| command.cwd.join(".caskade/prefix"))
    }

    /// The host command that runs `command` inside `prefix`.
    pub fn wrap(&self, command: &ProcessCommand, prefix: &Path, game_dir: &Path) -> ProcessCommand {
        let mut env = BTreeMap::from([
            (
                "WINEPREFIX".to_owned(),
                prefix.to_string_lossy().into_owned(),
            ),
            ("WINEDEBUG".to_owned(), "-all".to_owned()),
        ]);
        let (program, mut args) = match &self.launcher {
            Launcher::Umu {
                umu_run,
                proton_path,
                game_id,
            } => {
                env.insert("PROTONPATH".into(), proton_path.clone());
                env.insert("GAMEID".into(), game_id.clone());
                // umu otherwise asks before updating its runtime.
                env.insert("UMU_RUNTIME_UPDATE".into(), "1".into());
                // Isolate the Steam Runtime container: a private home (umu's
                // protonfixes always maps x:/w:/u: to the container's home,
                // /mnt and /media), the game read-write, and only what Proton
                // needs read-only: umu's data, the Proton build and an outside
                // installer's folder. A command may override these in its env.
                env.insert("PRESSURE_VESSEL_SHARE_HOME".into(), "0".into());
                env.insert(
                    "PRESSURE_VESSEL_FILESYSTEMS_RW".into(),
                    game_dir.to_string_lossy().into_owned(),
                );
                let mut read_only: Vec<PathBuf> = vec![];
                if let Some(data) = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|home| PathBuf::from(home).join(".local/share"))
                    })
                {
                    read_only.push(data.join("umu"));
                    read_only.push(data.join("Steam/compatibilitytools.d"));
                }
                if Path::new(proton_path).is_absolute() {
                    read_only.push(proton_path.into());
                }
                if command.program.is_absolute()
                    && !command.program.starts_with(game_dir)
                    && let Some(parent) = command.program.parent()
                {
                    read_only.push(parent.into());
                }
                let read_only: Vec<String> = read_only
                    .iter()
                    .filter(|path| path.exists())
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                if !read_only.is_empty() {
                    env.insert("PRESSURE_VESSEL_FILESYSTEMS_RO".into(), read_only.join(":"));
                }
                (umu_run.clone(), vec![])
            }
            Launcher::Wine { wine } => (wine.clone(), vec![]),
        };
        env.extend(command.env.clone());
        args.push(windows_program(&command.program, game_dir));
        args.extend(command.args.iter().cloned());
        ProcessCommand {
            program,
            args,
            env,
            cwd: command.cwd.clone(),
        }
    }

    /// Initialise the prefix once, then map `g:` and neutralise `z:`.
    pub fn prepare(&self, prefix: &Path, game_dir: &Path) -> Result<(), RuntimeError> {
        let mut prepared = self.prepared.lock().expect("prefix runtime poisoned");
        if prepared.iter().any(|done| done == prefix) {
            return isolate(prefix, game_dir).map_err(io_error);
        }
        fs::create_dir_all(prefix).map_err(io_error)?;
        if !prefix.join("dosdevices").is_dir() {
            let init = ProcessCommand {
                program: "wineboot".into(),
                args: vec!["-u".into()],
                env: BTreeMap::new(),
                cwd: game_dir.into(),
            };
            let process = self.native.spawn(&self.wrap(&init, prefix, game_dir))?;
            let exit = self.native.wait(process, INIT_TIMEOUT);
            self.native.kill(process)?;
            let exit = exit?;
            if exit.code != 0 {
                return Err(io_error(format!(
                    "prefix initialisation failed with exit code {}",
                    exit.code
                )));
            }
        }
        isolate(prefix, game_dir).map_err(io_error)?;
        prepared.push(prefix.into());
        Ok(())
    }
}

/// Game-owned programs run by their `G:\Game` path, so Wine sees a normal
/// drive path and working directory (a host path would launch through
/// `/unix` without either); programs outside the game keep their host path.
pub fn windows_program(program: &Path, game_dir: &Path) -> String {
    match program.strip_prefix(game_dir) {
        Ok(inside) if program.has_root() && !inside.as_os_str().is_empty() => {
            let mut path = GAME_DIR.to_owned();
            for part in inside.components() {
                path.push('\\');
                path.push_str(&part.as_os_str().to_string_lossy());
            }
            path
        }
        _ => program.to_string_lossy().into_owned(),
    }
}

/// Starts a wrapped game program from `G:\\Game`. Wine names a process's
/// start folder from its Unix path, and with `z:` gone a game's real folder
/// is on no drive: Wine gives it the relative `unix\\tmp\\…`, and the game
/// can't open its own data by a relative path. Through the prefix's `g:`
/// link the folder is on the drive; `$PWD` keeps that name past `getcwd`,
/// which would resolve the link. Only for a prefix [`isolate`] has set up, at
/// the latest before `command` is spawned. Other folders stay as they are.
pub fn start_on_game_drive(command: &mut ProcessCommand, prefix: &Path, game_dir: &Path) {
    if let Ok(inside) = command.cwd.strip_prefix(game_dir) {
        let cwd = prefix.join("dosdevices/g:/Game").join(inside);
        command
            .env
            .insert("PWD".into(), cwd.to_string_lossy().into_owned());
        command.cwd = cwd;
    }
}

/// Windows path of the game directory inside a prefix. Installers refuse a
/// bare drive root by default (Inno `AllowRootDirectory`, NSIS
/// `AllowRootDirInstall`), so the game is the `Game` folder on drive `G:`.
pub const GAME_DIR: &str = r"G:\Game";

fn replace_link(link: &Path, target: &Path) -> io::Result<()> {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(link)?,
        Ok(_) => {
            return Err(io::Error::other(format!(
                "{} exists and is not a link",
                link.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
    #[cfg(not(unix))]
    {
        let _ = target;
        Err(io::Error::other("prefix links need a Unix host"))
    }
}

/// Map `g:` to `<prefix>/drive_g`, whose `Game` entry links to the game
/// directory (relative when the prefix lives inside the game, so the folder
/// stays movable; absolute for prefixes kept elsewhere, e.g. app data for
/// exFAT roots), and point `z:` at an empty directory. Proton recreates a
/// missing `z:` as `/`, so it must exist; umu's protonfixes still maps its
/// own `u:`/`w:`/`x:` drives inside the runtime container.
pub fn isolate(prefix: &Path, game_dir: &Path) -> io::Result<()> {
    let dosdevices = prefix.join("dosdevices");
    let drive = prefix.join("drive_g");
    let empty = prefix.join("caskade-no-host");
    for directory in [&dosdevices, &drive, &empty] {
        fs::create_dir_all(directory)?;
    }
    let target = match drive.strip_prefix(game_dir) {
        Ok(inside) => inside.components().map(|_| "..").collect::<PathBuf>(),
        Err(_) => game_dir.to_path_buf(),
    };
    replace_link(&drive.join("Game"), &target)?;
    replace_link(&dosdevices.join("g:"), Path::new("../drive_g"))?;
    replace_link(&dosdevices.join("z:"), Path::new("../caskade-no-host"))
}

impl Runtime for PrefixRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        let prefix = Self::prefix_for(command);
        // The game directory is the prefix's owner when it lives inside it.
        let game_dir = prefix
            .strip_prefix(&command.cwd)
            .ok()
            .filter(|rest| rest.starts_with(".caskade"))
            .map(|_| command.cwd.clone())
            .or_else(|| command.env.get("CASKADE_GAME_DIR").map(PathBuf::from))
            .unwrap_or_else(|| command.cwd.clone());
        self.prepare(&prefix, &game_dir)?;
        let mut wrapped = self.wrap(command, &prefix, &game_dir);
        start_on_game_drive(&mut wrapped, &prefix, &game_dir);
        self.native.spawn(&wrapped)
    }
    fn wait(&self, process: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        self.native.wait(process, timeout)
    }
    fn kill(&self, process: ProcessId) -> Result<(), RuntimeError> {
        self.native.kill(process)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn g_holds_the_game_relatively_and_z_is_empty() {
        let game = tempfile::tempdir().unwrap();
        let prefix = game.path().join(".caskade/prefix");
        fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("/", prefix.join("dosdevices/z:")).unwrap();
        fs::write(game.path().join("game.exe"), b"MZ").unwrap();
        isolate(&prefix, game.path()).unwrap();
        // Idempotent: a second run replaces its own links.
        isolate(&prefix, game.path()).unwrap();
        let z = prefix.join("dosdevices/z:");
        assert_eq!(fs::read_dir(&z).unwrap().count(), 0);
        assert_eq!(
            fs::read_link(prefix.join("drive_g/Game")).unwrap(),
            Path::new("../../..")
        );
        assert!(prefix.join("dosdevices/g:/Game/game.exe").is_file());
        // Elsewhere (app data), the link is absolute.
        let outside = tempfile::tempdir().unwrap();
        isolate(outside.path(), game.path()).unwrap();
        assert_eq!(
            fs::read_link(outside.path().join("drive_g/Game")).unwrap(),
            game.path()
        );
        // A real directory where a link belongs is never deleted.
        let blocked = tempfile::tempdir().unwrap();
        fs::create_dir_all(blocked.path().join("dosdevices/z:/keep")).unwrap();
        assert!(isolate(blocked.path(), game.path()).is_err());
        assert!(blocked.path().join("dosdevices/z:/keep").is_dir());
    }

    #[test]
    fn a_game_starts_on_its_drive_and_other_folders_stay() {
        let command = |cwd: &str| ProcessCommand {
            program: "/game/game.exe".into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: cwd.into(),
        };
        let prefix = Path::new("/game/.caskade/prefix");
        let mut game = command("/game/bin");
        start_on_game_drive(&mut game, prefix, Path::new("/game"));
        let drive = prefix.join("dosdevices/g:/Game").join("bin");
        assert_eq!(game.cwd, drive);
        assert_eq!(game.env["PWD"], drive.to_string_lossy());
        let mut installer = command("/downloads");
        start_on_game_drive(&mut installer, prefix, Path::new("/game"));
        assert_eq!(installer.cwd, Path::new("/downloads"));
        assert!(!installer.env.contains_key("PWD"));
    }

    #[test]
    fn umu_command_carries_prefix_proton_and_game_id() {
        let runtime = PrefixRuntime::new(Launcher::Umu {
            umu_run: "/opt/umu/umu-run".into(),
            proton_path: "/opt/GE-Proton".into(),
            game_id: "umu-default".into(),
        });
        let wrapped = runtime.wrap(
            &ProcessCommand {
                program: "/src/setup.exe".into(),
                args: vec!["/DIR=G:\\".into()],
                env: BTreeMap::new(),
                cwd: "/game".into(),
            },
            Path::new("/game/.caskade/prefix"),
            Path::new("/game"),
        );
        assert_eq!(wrapped.program, Path::new("/opt/umu/umu-run"));
        assert_eq!(wrapped.args, ["/src/setup.exe", "/DIR=G:\\"]);
        assert_eq!(wrapped.env["WINEPREFIX"], "/game/.caskade/prefix");
        assert_eq!(wrapped.env["PROTONPATH"], "/opt/GE-Proton");
        assert_eq!(wrapped.env["GAMEID"], "umu-default");
        assert_eq!(wrapped.env["PRESSURE_VESSEL_FILESYSTEMS_RW"], "/game");
        assert_eq!(wrapped.env["PRESSURE_VESSEL_SHARE_HOME"], "0");
        // Game-owned programs use their G:\Game path; outside ones stay host paths.
        assert_eq!(
            windows_program(Path::new("/game/bin/unins000.exe"), Path::new("/game")),
            r"G:\Game\bin\unins000.exe"
        );
        assert_eq!(
            windows_program(Path::new("/src/setup.exe"), Path::new("/game")),
            "/src/setup.exe"
        );
        assert_eq!(
            windows_program(Path::new("regedit"), Path::new("/game")),
            "regedit"
        );
    }
}
