//! The streaming host (#257): Sunshine, installed,
//! configured and run by Caskade without being shown.
//!
//! - **On** ([`StreamingHost::enable`]): download it (managed tools),
//!   write its configuration ([`config`]), make its web credentials (OS
//!   keychain, then `--creds`), run the administrator step once
//!   ([`crate::elevation::Plan::StreamingHost`]), switch
//!   `core.streaming.hosting` on and start it. A cancelled or refused
//!   prompt leaves hosting off.
//! - **Running** ([`process`]): only while Caskade runs. It starts with
//!   Caskade when hosting is on and stops when Caskade quits
//!   ([`StreamingHost::shutdown`]); a window closed to the tray keeps it.
//!   A crash restarts it, up to five times in ten minutes; then, like a
//!   failed start or a refused sign-in, the status says "Streaming isn't
//!   working on this device".
//! - **Off** ([`StreamingHost::disable`]): stop it, undo the administrator
//!   step (one prompt) and remove the program. Its configuration,
//!   credentials and paired devices stay, so turning it on again needs no
//!   new pairing (§7).
//!
//! On Windows the program and configuration run from the administrator-only
//! copy the administrator step makes (§8.4); Caskade's own copy in `<data>`
//! is only the staging. macOS hosts are not in this milestone.

mod checks;
pub mod config;
pub mod process;
#[cfg(feature = "net")]
mod remote;
mod watch;

#[cfg(all(test, unix, feature = "net"))]
mod tests;

pub use checks::HostChecks;

use super::secrets::{Credentials, HostSecrets};
use crate::elevation::{Elevation, Outcome, Plan};
use caskade_core::{
    Error, Result,
    emulation::{EmulatorInstall, OperatingSystem},
    managed::{CancellationToken, ManagedTools},
    settings::{SettingsService, StreamingSettings},
};
use config::{HostConfig, HostFiles};
use process::{HostLaunch, HostRunner};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// The managed tool that hosts.
pub const TOOL: &str = "sunshine";
/// The port the host answers `/serverinfo` on.
pub const HTTP_PORT: u16 = 47989;
/// The host's HTTPS port.
pub const HTTPS_PORT: u16 = 47984;
/// The streaming settings' namespace.
pub(crate) const SETTINGS: &str = "core.streaming";

/// Where hosting is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HostState {
    Off,
    /// Downloading and setting up.
    SettingUp,
    /// Started, not answering yet.
    Starting,
    Ready,
    /// It crashed too often, didn't start or refused Caskade's sign-in.
    NeedsAttention,
    /// This system can't host (macOS in this milestone).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub state: HostState,
    /// The host program's version, once it answered.
    pub version: Option<String>,
    /// Why it needs attention, for people.
    pub problem: Option<String>,
    /// What a ready host can't do (§4.4–4.5): no screen to show, or pads
    /// that won't work. Streaming works otherwise.
    #[serde(default)]
    pub warning: Option<String>,
    /// Running, but other devices can't reach it, and the warning says why.
    /// It stays ready, so it works as soon as they can.
    #[serde(default)]
    pub unreachable: bool,
}

/// How switching hosting on or off ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HostOutcome {
    On,
    Off,
    /// The administrator prompt was dismissed: nothing changed.
    Cancelled,
    /// The OS refused the prompt: nothing changed.
    Refused,
    /// Someone already set streaming up on this device (#258): nothing
    /// was installed. Use it with `use_existing`, or leave hosting off.
    ExistingFound,
}

/// How often things are looked at (shorter in tests).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long a started host gets to answer.
    pub ready_wait: Duration,
    /// How often a running host is looked at.
    pub watch_every: Duration,
    /// How often this machine is looked at again while a host runs
    /// ([`HostChecks`]). Long, as on Windows that starts programs, a game
    /// may be streaming, and a network seldom changes.
    pub look_every: Duration,
    /// The first wait before a restart; doubled after each crash.
    pub restart_after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ready_wait: Duration::from_secs(45),
            watch_every: Duration::from_secs(2),
            look_every: Duration::from_secs(60),
            restart_after: Duration::from_secs(1),
        }
    }
}

struct Inner {
    tools: ManagedTools,
    elevation: Elevation,
    settings: SettingsService,
    secrets: HostSecrets,
    runner: Arc<dyn HostRunner>,
    /// Finds a user's own host ([`crate::existing`]).
    existing: Arc<dyn Fn() -> Option<crate::existing::Existing> + Send + Sync>,
    os: OperatingSystem,
    timing: Timing,
    /// Where the host's API answers.
    #[cfg(feature = "net")]
    api_base: url::Url,
    /// Where the host answers `/serverinfo`: whether it streams now.
    http_port: u16,
    checks: Arc<dyn Fn(OperatingSystem) -> HostChecks + Send + Sync>,
    name: Mutex<String>,
    status: Mutex<HostStatus>,
    watch: Mutex<Option<tokio::task::JoinHandle<()>>>,
    #[cfg(feature = "net")]
    api: Mutex<remote::KeptApi>,
    /// One switch at a time.
    switching: tokio::sync::Mutex<()>,
}

/// Caskade's streaming host; cheap to clone.
#[derive(Clone)]
pub struct StreamingHost {
    inner: Arc<Inner>,
}

fn not_working() -> String {
    caskade_core::tr!("Streaming isn't working on this device")
}

impl StreamingHost {
    /// This system's host: its OS, the keychain, and the child process
    /// (Linux) or the service (Windows).
    pub fn new(
        tools: ManagedTools,
        elevation: Elevation,
        settings: SettingsService,
        paths: &caskade_core::AppPaths,
    ) -> Self {
        let os = caskade_core::emulation::host_os();
        let runner: Arc<dyn HostRunner> = match os {
            OperatingSystem::Windows => Arc::new(process::ServiceRunner),
            _ => Arc::new(process::ChildRunner::new(
                paths.data.join("streaming").join("run.json"),
            )),
        };
        Self::with(
            tools,
            elevation,
            settings,
            Parts {
                os,
                runner,
                existing: Arc::new(crate::existing::detect_here),
                secrets: HostSecrets::keychain(),
                timing: Timing::default(),
                #[cfg(feature = "net")]
                api_base: url::Url::parse(crate::api::LOCAL).expect("valid URL"),
                http_port: HTTP_PORT,
                checks: Arc::new(HostChecks::here),
            },
        )
    }

    /// With other parts (tests).
    pub fn with(
        tools: ManagedTools,
        elevation: Elevation,
        settings: SettingsService,
        parts: Parts,
    ) -> Self {
        let name = sysinfo::System::host_name().unwrap_or_else(|| "Caskade".to_owned());
        Self {
            inner: Arc::new(Inner {
                tools,
                elevation,
                settings,
                secrets: parts.secrets,
                runner: parts.runner,
                existing: parts.existing,
                os: parts.os,
                timing: parts.timing,
                #[cfg(feature = "net")]
                api_base: parts.api_base,
                http_port: parts.http_port,
                checks: parts.checks,
                name: Mutex::new(name),
                status: Mutex::new(HostStatus {
                    state: HostState::Off,
                    version: None,
                    problem: None,
                    warning: None,
                    unreachable: false,
                }),
                watch: Mutex::default(),
                #[cfg(feature = "net")]
                api: Mutex::default(),
                switching: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// The device's name in Caskade, which clients see; used from the next
    /// start.
    pub fn set_device_name(&self, name: &str) {
        *caskade_core::lock(&self.inner.name) = name.to_owned();
    }

    pub fn status(&self) -> HostStatus {
        caskade_core::lock(&self.inner.status).clone()
    }

    fn set_status(&self, state: HostState, version: Option<String>, problem: Option<String>) {
        *caskade_core::lock(&self.inner.status) = HostStatus {
            state,
            version,
            problem,
            warning: None,
            unreachable: false,
        };
    }

    /// Ready, with what this machine can't do (§4.5): looked at now.
    async fn set_ready(&self, version: Option<String>) {
        let looked = self.look().await.ok();
        self.set_status(HostState::Ready, version, None);
        if let Some(checks) = looked {
            self.show_checks(checks);
        }
    }

    /// What a look at this machine found, on a host that is still ready.
    fn show_checks(&self, checks: HostChecks) {
        let mut status = caskade_core::lock(&self.inner.status);
        if status.state == HostState::Ready {
            status.warning = checks.warning(self.inner.os);
            status.unreachable = !checks.reachable;
        }
    }

    /// Looks at this machine on a thread of its own.
    fn look(&self) -> tokio::task::JoinHandle<HostChecks> {
        let (checks, os) = (self.inner.checks.clone(), self.inner.os);
        tokio::task::spawn_blocking(move || checks(os))
    }

    /// Windows: downloads the gamepad driver's pinned installer when the
    /// driver isn't there, for the administrator step to install (§4.5).
    /// Hosting goes on without it.
    async fn fetch_gamepad_driver(&self) {
        #[cfg(feature = "net")]
        if self.inner.os == OperatingSystem::Windows
            && !self
                .inner
                .elevation
                .present(crate::elevation::Change::GamepadDriver)
            && let Ok(staging) = self.staging()
            && let Err(error) = super::gamepads::fetch(&staging).await
        {
            tracing::warn!(%error, "the gamepad driver wasn't downloaded");
        }
    }

    fn supported(&self) -> bool {
        matches!(
            self.inner.os,
            OperatingSystem::Linux | OperatingSystem::Windows
        )
    }

    /// Whether hosting is switched on.
    pub async fn hosting(&self) -> bool {
        self.inner
            .settings
            .read_or_default::<StreamingSettings>(SETTINGS)
            .await
            .hosting
    }

    /// A user's own host ([`crate::existing`]), looked for on a blocking
    /// thread: it runs programs.
    async fn existing(&self) -> Option<crate::existing::Existing> {
        let existing = self.inner.existing.clone();
        tokio::task::spawn_blocking(move || existing())
            .await
            .ok()
            .flatten()
    }

    /// The host runs in a Flatpak sandbox (a user's own Flatpak Sunshine),
    /// so its apps' commands leave it with `flatpak-spawn --host`.
    pub async fn host_is_flatpak(&self) -> bool {
        self.existing()
            .await
            .is_some_and(|existing| existing.found == [crate::existing::Found::Flatpak])
    }

    /// Forgets the API client [`Self::api`] keeps.
    pub(crate) fn forget_api(&self) {
        #[cfg(feature = "net")]
        {
            let mut kept = caskade_core::lock(&self.inner.api);
            kept.api = None;
            kept.generation += 1;
        }
    }

    /// Whether hosting goes through a user's own host (#258).
    #[cfg(feature = "net")]
    async fn existing_mode(&self) -> bool {
        self.inner
            .settings
            .read_or_default::<StreamingSettings>(SETTINGS)
            .await
            .existing
    }

    async fn set_hosting(&self, hosting: bool) -> Result<()> {
        self.set_mode(hosting, false).await
    }

    async fn set_mode(&self, hosting: bool, existing: bool) -> Result<()> {
        let mut settings: StreamingSettings = self.inner.settings.read_or_default(SETTINGS).await;
        settings.hosting = hosting;
        settings.existing = existing;
        self.inner
            .settings
            .set(SETTINGS, serde_json::to_value(settings)?, None)
            .await
            .map_err(|error| Error::Invalid(error.to_string()))?;
        self.forget_api();
        Ok(())
    }

    /// Caskade's staging copy of the host: `<data>/streaming/host`.
    fn staging(&self) -> Result<PathBuf> {
        self.inner.tools.dir(TOOL)
    }

    /// Where the configuration is written before the first start: the
    /// staging copy (Windows copies it on), or the run folder (Linux).
    fn staged_files(&self) -> Result<HostFiles> {
        let staging = self.staging()?;
        Ok(match self.inner.os {
            OperatingSystem::Windows => HostFiles::windows(&staging),
            _ => HostFiles::linux(&staging.join("config")),
        })
    }

    /// Where the running host keeps its files.
    #[cfg(feature = "net")]
    fn running_files(&self) -> Result<HostFiles> {
        match self.inner.os {
            OperatingSystem::Windows => crate::elevation::windows::target()
                .map(|target| HostFiles::windows(&target))
                .ok_or_else(|| Error::Invalid(not_working())),
            _ => self.staged_files(),
        }
    }

    /// The installed host program, downloaded first when it isn't there.
    async fn program(&self, download: bool) -> Result<PathBuf> {
        let mut found = self.inner.tools.find(TOOL).await?;
        if found.is_none() && download {
            self.inner
                .tools
                .install(TOOL, &CancellationToken::new(), |_, _| {})
                .await?;
            found = self.inner.tools.find(TOOL).await?;
        }
        match found {
            Some(EmulatorInstall::Folder { dir, executable }) => Ok(dir.join(executable)),
            Some(_) => Err(Error::Invalid(caskade_core::tr!(
                "this system's streaming host can't be run by Caskade yet"
            ))),
            None => Err(Error::Invalid(not_working())),
        }
    }

    /// How the host starts here (Linux; Windows starts the service).
    fn launch(&self, program: &Path) -> Result<HostLaunch> {
        let staging = self.staging()?;
        Ok(HostLaunch {
            program: program.to_path_buf(),
            args: Vec::new(),
            env: vec![("CONFIGURATION_DIRECTORY".into(), staging.join("config"))],
            cwd: staging,
        })
    }

    /// Writes the configuration and, the first time, the web credentials.
    /// `force` sets the credentials again (a repair): the host's copy may
    /// have drifted from the keychain's, or the keychain lost them.
    /// The API client is made again afterwards: the credentials or the
    /// certificate may be new.
    async fn configure(&self, program: &Path, force: bool) -> Result<Credentials> {
        self.forget_api();
        let files = self.staged_files()?;
        let name = caskade_core::lock(&self.inner.name).clone();
        config::write(
            &files,
            &HostConfig {
                name,
                windows: self.inner.os == OperatingSystem::Windows,
            },
        )?;
        let (credentials, new) = match self.inner.secrets.get().await? {
            Some(credentials) => (credentials, false),
            None => {
                let credentials = Credentials::random();
                self.inner.secrets.set(&credentials).await?;
                (credentials, true)
            }
        };
        if new || force || !files.state().is_file() {
            let mut command = tokio::process::Command::new(program);
            caskade_core::managed::outside_own_appimage(command.as_std_mut());
            command
                .args(["--creds", &credentials.user, &credentials.password])
                .current_dir(self.staging()?)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true);
            if self.inner.os != OperatingSystem::Windows {
                command.env("CONFIGURATION_DIRECTORY", self.staging()?.join("config"));
            }
            let status = tokio::time::timeout(Duration::from_secs(60), command.status())
                .await
                .map_err(|_| Error::Invalid(not_working()))??;
            if !status.success() || !files.state().is_file() {
                return Err(Error::Invalid(caskade_core::tr!(
                    "the streaming host's sign-in couldn't be set"
                )));
            }
        }
        self.forget_api();
        Ok(credentials)
    }

    /// Switches hosting on: one administrator prompt (none when it is
    /// already set up). Cancelling it leaves hosting off.
    pub async fn enable(&self) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        if !self.supported() {
            self.set_status(HostState::Unsupported, None, None);
            return Err(Error::Invalid(caskade_core::tr!(
                "this device can't host game streams"
            )));
        }
        // A user's own host: offer it instead of installing a second.
        if self.inner.tools.managed(TOOL).await?.is_none() && self.existing().await.is_some() {
            return Ok(HostOutcome::ExistingFound);
        }
        self.set_status(HostState::SettingUp, None, None);
        let prepared = async {
            // Downloads and setup show in Activity (#265).
            let _activity = caskade_jobs::global()
                .activity(caskade_core::tr!("Setting up streaming from this device"));
            let program = self.program(true).await?;
            self.configure(&program, false).await?;
            Ok::<_, Error>(program)
        }
        .await;
        let program = match prepared {
            Ok(program) => program,
            Err(error) => {
                self.set_status(HostState::Off, None, None);
                return Err(error);
            }
        };
        self.fetch_gamepad_driver().await;
        match self.inner.elevation.apply(Plan::StreamingHost).await {
            Ok(Outcome::Cancelled) => {
                self.set_status(HostState::Off, None, None);
                return Ok(HostOutcome::Cancelled);
            }
            Ok(Outcome::Refused) => {
                self.set_status(HostState::Off, None, None);
                return Ok(HostOutcome::Refused);
            }
            Ok(Outcome::NothingToDo | Outcome::Done(_)) => {}
            Err(error) => {
                self.set_status(HostState::Off, None, None);
                return Err(error);
            }
        }
        self.set_hosting(true).await?;
        self.run(&program).await;
        Ok(HostOutcome::On)
    }

    /// Switches hosting off: the host stops, the administrator step is
    /// undone (one prompt) and the program removed; its configuration,
    /// credentials and pairings stay. A cancelled prompt leaves it on.
    pub async fn disable(&self) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        #[cfg(feature = "net")]
        if self.existing_mode().await {
            // Only Caskade's apps go; the host and its other apps stay.
            let api = self.api().await?;
            let caskade = crate::publish::Caskade::own(self.host_is_flatpak().await)?;
            self.checked(crate::publish::withdraw(&api, &caskade).await)?;
            self.set_mode(false, false).await?;
            self.set_status(HostState::Off, None, None);
            return Ok(HostOutcome::Off);
        }
        self.halt().await;
        let outcome = match self.inner.elevation.undo(Plan::StreamingHost).await {
            Ok(outcome) => outcome,
            Err(error) => {
                self.resume().await;
                return Err(error);
            }
        };
        match outcome {
            Outcome::Cancelled | Outcome::Refused => {
                self.resume().await;
                return Ok(if outcome == Outcome::Cancelled {
                    HostOutcome::Cancelled
                } else {
                    HostOutcome::Refused
                });
            }
            Outcome::NothingToDo | Outcome::Done(_) => {}
        }
        if self.inner.tools.managed(TOOL).await?.is_some() {
            self.inner.tools.remove(TOOL).await?;
        }
        self.set_hosting(false).await?;
        self.set_status(HostState::Off, None, None);
        Ok(HostOutcome::Off)
    }

    /// Whether a device is streaming from this one now.
    async fn streaming_now(&self) -> bool {
        #[cfg(feature = "net")]
        {
            crate::hosts::ask_server_info("127.0.0.1", self.inner.http_port)
                .await
                .is_some_and(|info| info.busy)
        }
        #[cfg(not(feature = "net"))]
        {
            // Nothing can ask without the network client.
            let _ = self.inner.http_port;
            false
        }
    }

    /// Installs the newest host, never during a stream:
    /// it stops, the files are swapped (its configuration, state, paired
    /// devices and published games stay), Caskade's configuration is
    /// written again, the administrator step re-applies what the new copy
    /// needs (on Windows, copying it into the administrator-only folder:
    /// one prompt), and it starts again.
    pub async fn update(&self) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        #[cfg(feature = "net")]
        if self.existing_mode().await {
            return Err(Error::Invalid(caskade_core::tr!(
                "The streaming set up on this device before Caskade is updated the way it was installed."
            )));
        }
        if self.streaming_now().await {
            return Err(Error::Invalid(caskade_core::tr!(
                "A device is streaming from this one. Update once the game ends."
            )));
        }
        let hosting = self.hosting().await;
        let _activity = caskade_jobs::global()
            .activity(caskade_core::tr!("Updating streaming from this device"));
        if hosting {
            self.halt().await;
        }
        let updated = async {
            self.inner
                .tools
                .update(TOOL, &CancellationToken::new(), |_, _| {})
                .await?;
            let program = self.program(false).await?;
            self.configure(&program, false).await?;
            Ok::<_, Error>(program)
        }
        .await;
        let program = match updated {
            Ok(program) => program,
            Err(error) => {
                if hosting {
                    self.resume().await;
                }
                return Err(error);
            }
        };
        if !hosting {
            return Ok(HostOutcome::Off);
        }
        let outcome = self.reapply().await;
        self.run(&program).await;
        outcome
    }

    /// Re-runs setup (§7): the program's missing files, the configuration
    /// and the web credentials (new ones when the keychain lost them), and
    /// whatever of the administrator step went (rules, the service, the
    /// firewall, the administrator-only copy), behind one prompt. Paired
    /// devices stay.
    pub async fn repair(&self) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        if !self.supported() {
            self.set_status(HostState::Unsupported, None, None);
            return Err(Error::Invalid(caskade_core::tr!(
                "this device can't host game streams"
            )));
        }
        #[cfg(feature = "net")]
        if self.existing_mode().await {
            // Not Caskade's to set up: only whether it answers.
            return match self.until_ready().await {
                Ok(version) => {
                    self.set_status(HostState::Ready, version, None);
                    Ok(HostOutcome::On)
                }
                Err(error) => {
                    self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                    Err(error)
                }
            };
        }
        self.halt().await;
        self.set_status(HostState::SettingUp, None, None);
        let prepared = async {
            let _activity = caskade_jobs::global()
                .activity(caskade_core::tr!("Repairing streaming from this device"));
            if !self.inner.tools.intact(TOOL).await? {
                self.inner
                    .tools
                    .install(TOOL, &CancellationToken::new(), |_, _| {})
                    .await?;
            }
            let program = self.program(true).await?;
            self.configure(&program, true).await?;
            Ok::<_, Error>(program)
        }
        .await;
        let program = match prepared {
            Ok(program) => program,
            Err(error) => {
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return Err(error);
            }
        };
        let outcome = self.reapply().await?;
        if outcome == HostOutcome::On {
            self.set_hosting(true).await?;
            self.run(&program).await;
        } else {
            self.set_status(HostState::NeedsAttention, None, Some(not_working()));
        }
        Ok(outcome)
    }

    /// The administrator step for what is missing or stale, as an outcome.
    async fn reapply(&self) -> Result<HostOutcome> {
        self.fetch_gamepad_driver().await;
        Ok(
            match self.inner.elevation.apply(Plan::StreamingHost).await? {
                Outcome::Cancelled => HostOutcome::Cancelled,
                Outcome::Refused => HostOutcome::Refused,
                Outcome::NothingToDo | Outcome::Done(_) => HostOutcome::On,
            },
        )
    }

    /// At Caskade's start: runs the host when hosting is on.
    pub async fn start(&self) {
        if !self.supported() {
            self.set_status(HostState::Unsupported, None, None);
            return;
        }
        if !self.hosting().await {
            return;
        }
        #[cfg(feature = "net")]
        if self.existing_mode().await {
            // Not Caskade's to run: only whether it answers.
            match self.until_ready().await {
                Ok(version) => self.set_status(HostState::Ready, version, None),
                Err(_) => self.set_status(HostState::NeedsAttention, None, Some(not_working())),
            }
            return;
        }
        self.resume().await;
    }

    /// Hosts through the user's own host (#258) with its web credentials,
    /// once they work against its API (pinned to its own certificate).
    /// Nothing is installed and its settings and apps stay as they are.
    #[cfg(feature = "net")]
    pub async fn use_existing(&self, credentials: Credentials) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        let cert = self
            .existing()
            .await
            .and_then(|existing| existing.cert)
            .ok_or_else(|| {
                Error::Invalid(caskade_core::tr!(
                    "Caskade can't find the certificate of the streaming already set up here"
                ))
            })?;
        let api = crate::api::HostApi::with_cert_file(
            self.inner.api_base.clone(),
            credentials.clone(),
            &cert,
        )?;
        let version = match api.version().await {
            Ok(version) => version,
            Err(Error::Forbidden(_)) => {
                return Err(Error::Forbidden(caskade_core::tr!(
                    "That user name and password didn't work"
                )));
            }
            Err(error) => return Err(error),
        };
        self.inner.secrets.set_existing(&credentials).await?;
        self.set_mode(true, true).await?;
        self.set_status(HostState::Ready, Some(version), None);
        Ok(HostOutcome::On)
    }
}

/// The parts [`StreamingHost::with`] takes.
pub struct Parts {
    pub os: OperatingSystem,
    pub runner: Arc<dyn HostRunner>,
    pub existing: Arc<dyn Fn() -> Option<crate::existing::Existing> + Send + Sync>,
    pub secrets: HostSecrets,
    pub timing: Timing,
    #[cfg(feature = "net")]
    pub api_base: url::Url,
    /// The host's `/serverinfo` port ([`HTTP_PORT`]; another in tests).
    pub http_port: u16,
    /// What this machine lets a host do ([`HostChecks::here`]).
    pub checks: Arc<dyn Fn(OperatingSystem) -> HostChecks + Send + Sync>,
}
