//! The streaming client (STREAMING-PLAN §5, #261): Moonlight, installed the
//! first time someone streams and run with state of its own, so a user's
//! own Moonlight never sees Caskade's hosts or settings, nor Caskade theirs.
//!
//! Moonlight runs in portable mode: a `portable.dat` in its working
//! directory makes it keep settings, paired hosts, certificates and caches
//! in `<cwd>/Moonlight Game Streaming Project/Moonlight.ini` and
//! `<cwd>/cache/` (checked against moonlight-qt 6.2.0). Caskade's working
//! directory is `<data>/streaming/client/state/`, or for the Flatpak a
//! folder inside the app's own `~/.var/app/…` (the sandbox can't reach
//! Caskade's), and it writes the settings that keep Moonlight quiet first.

use crate::{
    Error, Result,
    emulation::EmulatorInstall,
    managed::{CancellationToken, ManagedTools},
};
#[cfg(feature = "net")]
use std::time::Duration;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The managed tool that streams.
pub const TOOL: &str = "moonlight";
/// Where Moonlight keeps its settings, relative to its working directory.
pub const SETTINGS_DIR: &str = "Moonlight Game Streaming Project";
pub const SETTINGS_FILE: &str = "Moonlight.ini";
/// Makes Moonlight portable when it is in the working directory.
pub const PORTABLE: &str = "portable.dat";
/// Caskade's folder inside the Flatpak's own data.
const FLATPAK_STATE: &str = "caskade";

/// Settings written before every start (§5.2): no warning toasts or
/// overlays, no Discord presence naming it, its own window full screen
/// (dark under Caskade's curtain), and hosts found on the network.
pub const QUIET: [(&str, &str); 6] = [
    ("confwarnings", "false"),
    ("connwarnings", "false"),
    ("detectnetblocking", "false"),
    ("richpresence", "false"),
    ("uidisplaymode", "2"),
    ("mdns", "true"),
];

/// The client as Caskade runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub install: EmulatorInstall,
    /// Its working directory, holding `portable.dat` and its state.
    pub state: PathBuf,
}

/// Sets `prefs` in the `[General]` section of a Qt settings file, keeping
/// every other line (Moonlight's certificate, key, id and paired hosts).
pub fn set_general(text: &str, prefs: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let general = lines.iter().position(|line| line.trim() == "[General]");
    let start = match general {
        Some(at) => at + 1,
        None => {
            lines.insert(0, "[General]".to_owned());
            1
        }
    };
    let end = lines[start..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |at| start + at);
    let mut missing = Vec::new();
    for (key, value) in prefs {
        let line = format!("{key}={value}");
        match lines[start..end].iter().position(|existing| {
            existing
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == *key)
        }) {
            Some(at) => lines[start + at] = line,
            None => missing.push(line),
        }
    }
    let at = end.min(lines.len());
    for (offset, line) in missing.into_iter().enumerate() {
        lines.insert(at + offset, line);
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Makes `dir` the client's working directory: `portable.dat`, and the
/// quiet settings in its settings file.
pub fn prepare_state(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir.join(SETTINGS_DIR))?;
    let portable = dir.join(PORTABLE);
    if !portable.exists() {
        std::fs::write(&portable, "")?;
    }
    let settings = dir.join(SETTINGS_DIR).join(SETTINGS_FILE);
    let text = match std::fs::read_to_string(&settings) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let updated = set_general(&text, &QUIET);
    if updated != text {
        let mut part = settings.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        std::fs::write(&part, updated)?;
        std::fs::rename(&part, &settings)?;
    }
    Ok(())
}

/// Installs, prepares and runs the client.
#[derive(Clone)]
pub struct StreamingClient {
    tools: ManagedTools,
    home: Option<PathBuf>,
    flatpak: PathBuf,
    /// One Moonlight at a time: it writes its state file when it quits,
    /// over any change made meanwhile.
    #[cfg(feature = "net")]
    pub(crate) busy: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl StreamingClient {
    pub fn new(tools: ManagedTools) -> Self {
        Self {
            tools,
            home: std::env::var_os("HOME").map(PathBuf::from),
            flatpak: "flatpak".into(),
            #[cfg(feature = "net")]
            busy: std::sync::Arc::default(),
        }
    }

    /// Another home and `flatpak` program (tests).
    pub fn with_home(self, home: PathBuf, flatpak: PathBuf) -> Self {
        Self {
            home: Some(home),
            flatpak,
            ..self
        }
    }

    /// The installed client, if any.
    pub async fn installed(&self) -> Result<Option<EmulatorInstall>> {
        self.tools.find(TOOL).await
    }

    /// The working directory for `install`.
    pub fn state_dir(&self, install: &EmulatorInstall) -> Result<PathBuf> {
        match install {
            EmulatorInstall::Flatpak { app_id } => self
                .home
                .as_ref()
                .map(|home| {
                    home.join(".var")
                        .join("app")
                        .join(app_id)
                        .join(FLATPAK_STATE)
                })
                .ok_or_else(|| Error::Invalid(crate::tr!("there is no home folder"))),
            _ => Ok(self.tools.dir(TOOL)?.join("state")),
        }
    }

    /// The client, installed first when it isn't there (an Activity job,
    /// "Getting streaming ready"), with its state prepared.
    pub async fn ensure(&self, cancel: &CancellationToken) -> Result<Client> {
        let install = match self.tools.find(TOOL).await? {
            Some(install) => install,
            None => {
                let _activity =
                    caskade_jobs::global().activity(crate::tr!("Getting streaming ready"));
                self.tools.install(TOOL, cancel, |_, _| {}).await?;
                self.tools
                    .find(TOOL)
                    .await?
                    .ok_or_else(|| Error::Invalid(crate::tr!("streaming couldn't be set up")))?
            }
        };
        let state = self.state_dir(&install)?;
        prepare_state(&state)?;
        Ok(Client { install, state })
    }

    /// Moonlight with `args`, in its own working directory. An unpacked
    /// AppImage runs through its `AppRun`, which picks a fallback VA-API
    /// when the system's doesn't work and then runs Moonlight itself.
    pub fn command(&self, client: &Client, args: &[OsString]) -> Result<std::process::Command> {
        let mut command = match &client.install {
            EmulatorInstall::Folder { dir, executable } => {
                let root = self.tools.dir(TOOL)?;
                let app_run = root.join("AppRun");
                if dir.starts_with(&root) && app_run.is_file() {
                    std::process::Command::new(app_run)
                } else {
                    std::process::Command::new(dir.join(executable))
                }
            }
            EmulatorInstall::AppImage { file } => std::process::Command::new(file),
            EmulatorInstall::AppBundle { bundle } => {
                std::process::Command::new(bundle.join("Contents").join("MacOS").join("Moonlight"))
            }
            EmulatorInstall::Flatpak { app_id } => {
                let mut command = std::process::Command::new(&self.flatpak);
                command
                    .arg("run")
                    .arg(format!("--cwd={}", client.state.display()))
                    .arg(app_id);
                command
            }
            EmulatorInstall::Path { binary } => std::process::Command::new(binary),
        };
        command.args(args).current_dir(&client.state);
        crate::managed::outside_own_appimage(&mut command);
        Ok(command)
    }
}

#[cfg(feature = "net")]
impl StreamingClient {
    fn settings_file(state: &Path) -> PathBuf {
        state.join(SETTINGS_DIR).join(SETTINGS_FILE)
    }

    /// Moonlight's identity in Caskade's state, made the first time by a
    /// run that opens no window (`list` of an address nobody answers,
    /// asked to quit, so it saves).
    pub async fn identity(&self, client: &Client) -> Result<super::pairing::Identity> {
        let read = || {
            std::fs::read_to_string(Self::settings_file(&client.state))
                .ok()
                .and_then(|text| super::pairing::Identity::from_settings(&text))
        };
        if let Some(identity) = read() {
            return Ok(identity);
        }
        // TEST-NET-1: never a host.
        self.run_and_quit(
            client,
            &["list".into(), "192.0.2.1".into(), "--csv".into()],
            Duration::from_secs(4),
        )
        .await?;
        read().ok_or_else(|| Error::Invalid(crate::tr!("streaming couldn't be set up")))
    }

    /// Runs Moonlight with `args`, asks it to quit after `after` (so it
    /// saves its state) and waits for it.
    pub(crate) async fn run_and_quit(
        &self,
        client: &Client,
        args: &[OsString],
        after: Duration,
    ) -> Result<()> {
        let mut command = tokio::process::Command::from(self.command(client, args)?);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        if tokio::time::timeout(after, child.wait()).await.is_err() {
            if let Some(pid) = child.id() {
                let _ = tokio::process::Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status()
                    .await;
            }
            if tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_err()
            {
                let _ = child.kill().await;
            }
        }
        Ok(())
    }

    /// Pairs with the host at `address` using `pin`, which someone enters
    /// on the host: no window of Moonlight's opens. A paired host is
    /// recorded in Caskade's client state as Moonlight records it.
    pub async fn pair(
        &self,
        address: &str,
        pin: &str,
        wait: Duration,
        cancel: &CancellationToken,
    ) -> Result<super::pairing::Paired> {
        let _busy = self.busy.lock().await;
        let client = self.ensure(cancel).await?;
        let identity = self.identity(&client).await?;
        let info = super::hosts::ask_server_info(address, super::hosts::HTTP_PORT)
            .await
            .ok_or_else(|| {
                Error::Invalid(crate::tr!("{address} isn't answering", address = address))
            })?;
        let endpoint = super::pairing::Endpoint::new(address)?;
        let paired = super::pairing::Pairing {
            endpoint: &endpoint,
            identity: &identity,
            pin,
            wait,
        }
        .run()
        .await?;
        if let super::pairing::Paired::Paired { server_cert_pem } = &paired {
            let file = Self::settings_file(&client.state);
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let updated = super::pairing::add_host(
                &text,
                &super::pairing::HostRecord {
                    uuid: info.id,
                    name: info.name,
                    address: address.to_owned(),
                    server_cert_pem: server_cert_pem.clone(),
                },
            );
            write_atomically(&file, &updated)?;
        }
        Ok(paired)
    }

    /// Installs the newest client (STREAMING-PLAN §7), never during a
    /// stream: a stream holds the client until it ends. Its state (this
    /// device's identity, paired devices, settings) is a folder the
    /// release doesn't ship, so it stays.
    #[cfg(feature = "net")]
    pub async fn update(&self, cancel: &CancellationToken) -> Result<()> {
        let Ok(_busy) = self.busy.clone().try_lock_owned() else {
            return Err(Error::Invalid(crate::tr!(
                "This device is streaming. Update once the game ends."
            )));
        };
        let _activity =
            caskade_jobs::global().activity(crate::tr!("Updating streaming to this device"));
        self.tools.update(TOOL, cancel, |_, _| {}).await?;
        if let Some(install) = self.installed().await? {
            prepare_state(&self.state_dir(&install)?)?;
        }
        Ok(())
    }

    /// Forgets a paired host on this device.
    pub async fn forget(&self, host_id: &str) -> Result<()> {
        let _busy = self.busy.lock().await;
        let Some(install) = self.installed().await? else {
            return Ok(());
        };
        let file = Self::settings_file(&self.state_dir(&install)?);
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Ok(());
        };
        write_atomically(&file, &super::pairing::forget_host(&text, host_id))
    }
}

#[cfg(feature = "net")]
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, text)?;
    std::fs::rename(&part, path)?;
    Ok(())
}

#[cfg(all(test, unix, feature = "net"))]
pub(super) mod tests;
