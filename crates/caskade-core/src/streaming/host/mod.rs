//! The streaming host (STREAMING-PLAN §4, #257): Sunshine, installed,
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

pub mod config;
pub mod process;

#[cfg(all(test, unix, feature = "net"))]
mod tests;

use super::secrets::{Credentials, HostSecrets};
use crate::{
    Error, Result,
    elevation::{Elevation, Outcome, Plan},
    emulation::{EmulatorInstall, OperatingSystem},
    managed::{CancellationToken, ManagedTools},
    settings::{SettingsService, StreamingSettings},
};
use config::{HostConfig, HostFiles};
use process::{HostLaunch, HostRunner};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The managed tool that hosts.
pub const TOOL: &str = "sunshine";
/// The port the host answers `/serverinfo` on.
pub const HTTP_PORT: u16 = 47989;
const SETTINGS: &str = "core.streaming";

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
    /// The first wait before a restart; doubled after each crash.
    pub restart_after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ready_wait: Duration::from_secs(45),
            watch_every: Duration::from_secs(2),
            restart_after: Duration::from_secs(1),
        }
    }
}

/// Restarts after this many crashes within [`CRASH_WINDOW`] stop.
const CRASHES: usize = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(10 * 60);

struct Inner {
    tools: ManagedTools,
    elevation: Elevation,
    settings: SettingsService,
    secrets: HostSecrets,
    runner: Arc<dyn HostRunner>,
    /// Finds a user's own host ([`crate::streaming::existing`]).
    existing: Arc<dyn Fn() -> Option<crate::streaming::existing::Existing> + Send + Sync>,
    os: OperatingSystem,
    timing: Timing,
    /// Where the host's API answers.
    #[cfg(feature = "net")]
    api_base: url::Url,
    /// Where the host answers `/serverinfo`: whether it streams now.
    http_port: u16,
    name: Mutex<String>,
    status: Mutex<HostStatus>,
    watch: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// One switch at a time.
    switching: tokio::sync::Mutex<()>,
}

/// Caskade's streaming host; cheap to clone.
#[derive(Clone)]
pub struct StreamingHost {
    inner: Arc<Inner>,
}

fn not_working() -> String {
    crate::tr!("Streaming isn't working on this device")
}

impl StreamingHost {
    /// This system's host: its OS, the keychain, and the child process
    /// (Linux) or the service (Windows).
    pub fn new(
        tools: ManagedTools,
        elevation: Elevation,
        settings: SettingsService,
        paths: &crate::AppPaths,
    ) -> Self {
        let os = crate::emulation::host_os();
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
                existing: Arc::new(crate::streaming::existing::detect_here),
                secrets: HostSecrets::keychain(),
                timing: Timing::default(),
                #[cfg(feature = "net")]
                api_base: url::Url::parse(crate::streaming::api::LOCAL).expect("valid URL"),
                http_port: HTTP_PORT,
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
                name: Mutex::new(name),
                status: Mutex::new(HostStatus {
                    state: HostState::Off,
                    version: None,
                    problem: None,
                }),
                watch: Mutex::default(),
                switching: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// The device's name in Caskade, which clients see; used from the next
    /// start.
    pub fn set_device_name(&self, name: &str) {
        *crate::lock(&self.inner.name) = name.to_owned();
    }

    pub fn status(&self) -> HostStatus {
        crate::lock(&self.inner.status).clone()
    }

    fn set_status(&self, state: HostState, version: Option<String>, problem: Option<String>) {
        *crate::lock(&self.inner.status) = HostStatus {
            state,
            version,
            problem,
        };
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

    /// The host runs in a Flatpak sandbox (a user's own Flatpak Sunshine),
    /// so its apps' commands leave it with `flatpak-spawn --host`.
    pub fn host_is_flatpak(&self) -> bool {
        (self.inner.existing)()
            .is_some_and(|existing| existing.found == [crate::streaming::existing::Found::Flatpak])
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
            Some(_) => Err(Error::Invalid(crate::tr!(
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
    async fn configure(&self, program: &Path, force: bool) -> Result<Credentials> {
        let files = self.staged_files()?;
        let name = crate::lock(&self.inner.name).clone();
        config::write(&files, &HostConfig { name })?;
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
            crate::managed::outside_own_appimage(command.as_std_mut());
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
                return Err(Error::Invalid(crate::tr!(
                    "the streaming host's sign-in couldn't be set"
                )));
            }
        }
        Ok(credentials)
    }

    /// Switches hosting on: one administrator prompt (none when it is
    /// already set up). Cancelling it leaves hosting off.
    pub async fn enable(&self) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        if !self.supported() {
            self.set_status(HostState::Unsupported, None, None);
            return Err(Error::Invalid(crate::tr!(
                "this device can't host game streams"
            )));
        }
        // A user's own host: offer it instead of installing a second.
        if self.inner.tools.managed(TOOL).await?.is_none() && (self.inner.existing)().is_some() {
            return Ok(HostOutcome::ExistingFound);
        }
        self.set_status(HostState::SettingUp, None, None);
        let prepared = async {
            // Downloads and setup show in Activity (#265).
            let _activity = caskade_jobs::global()
                .activity(crate::tr!("Setting up streaming from this device"));
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
            let caskade = crate::streaming::publish::Caskade {
                program: std::env::current_exe()?,
                flatpak_host: self.host_is_flatpak(),
            };
            crate::streaming::publish::withdraw(&api, &caskade).await?;
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
            crate::streaming::hosts::ask_server_info("127.0.0.1", self.inner.http_port)
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

    /// Installs the newest host (STREAMING-PLAN §7), never during a stream:
    /// it stops, the files are swapped (its configuration, state, paired
    /// devices and published games stay), Caskade's configuration is
    /// written again, the administrator step re-applies what the new copy
    /// needs (on Windows, copying it into the administrator-only folder:
    /// one prompt), and it starts again.
    pub async fn update(&self) -> Result<HostOutcome> {
        let _switching = self.inner.switching.lock().await;
        #[cfg(feature = "net")]
        if self.existing_mode().await {
            return Err(Error::Invalid(crate::tr!(
                "The streaming set up on this device before Caskade is updated the way it was installed."
            )));
        }
        if self.streaming_now().await {
            return Err(Error::Invalid(crate::tr!(
                "A device is streaming from this one. Update once the game ends."
            )));
        }
        let hosting = self.hosting().await;
        let _activity =
            caskade_jobs::global().activity(crate::tr!("Updating streaming from this device"));
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
            return Err(Error::Invalid(crate::tr!(
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
            let _activity =
                caskade_jobs::global().activity(crate::tr!("Repairing streaming from this device"));
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
        let cert = (self.inner.existing)()
            .and_then(|existing| existing.cert)
            .ok_or_else(|| {
                Error::Invalid(crate::tr!(
                    "Caskade can't find the certificate of the streaming already set up here"
                ))
            })?;
        let api = crate::streaming::api::HostApi::with_cert_file(
            self.inner.api_base.clone(),
            credentials.clone(),
            &cert,
        )?;
        let version = match api.version().await {
            Ok(version) => version,
            Err(Error::Forbidden(_)) => {
                return Err(Error::Forbidden(crate::tr!(
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

    /// Starts it again after a pause, with its configuration rewritten.
    async fn resume(&self) {
        let prepared = async {
            let program = self.program(false).await?;
            if self.inner.os != OperatingSystem::Windows {
                self.configure(&program, false).await?;
            }
            Ok::<_, Error>(program)
        }
        .await;
        match prepared {
            Ok(program) => self.run(&program).await,
            Err(error) => {
                tracing::warn!(%error, "streaming host not started");
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
            }
        }
    }

    /// Starts the host and watches it.
    async fn run(&self, program: &Path) {
        let launch = match self.launch(program) {
            Ok(launch) => launch,
            Err(error) => {
                tracing::warn!(%error, "streaming host not started");
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return;
            }
        };
        let this = self.clone();
        let watch = tokio::spawn(async move { this.watch(launch).await });
        if let Some(old) = crate::lock(&self.inner.watch).replace(watch) {
            old.abort();
        }
    }

    /// Starts, waits for it to answer, and restarts it after a crash.
    async fn watch(&self, launch: HostLaunch) {
        let mut crashes: VecDeque<Instant> = VecDeque::new();
        loop {
            self.set_status(HostState::Starting, None, None);
            let runner = self.inner.runner.clone();
            let started = {
                let launch = launch.clone();
                tokio::task::spawn_blocking(move || runner.start(&launch)).await
            };
            if !matches!(started, Ok(Ok(()))) {
                tracing::warn!("streaming host didn't start");
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return;
            }
            match self.until_ready().await {
                Ok(version) => self.set_status(HostState::Ready, version, None),
                Err(error) => {
                    tracing::warn!(%error, "streaming host isn't answering");
                    self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                    let runner = self.inner.runner.clone();
                    let _ = tokio::task::spawn_blocking(move || runner.stop()).await;
                    return;
                }
            }
            while self.inner.runner.running() {
                tokio::time::sleep(self.inner.timing.watch_every).await;
            }
            let now = Instant::now();
            crashes.push_back(now);
            while crashes
                .front()
                .is_some_and(|at| now.duration_since(*at) > CRASH_WINDOW)
            {
                crashes.pop_front();
            }
            tracing::warn!(
                crashes = crashes.len(),
                "streaming host stopped unexpectedly"
            );
            if crashes.len() >= CRASHES {
                self.set_status(HostState::NeedsAttention, None, Some(not_working()));
                return;
            }
            let wait = self.inner.timing.restart_after * 2u32.pow(crashes.len() as u32 - 1);
            tokio::time::sleep(wait).await;
        }
    }

    /// Waits for the started host to answer; its version.
    async fn until_ready(&self) -> Result<Option<String>> {
        #[cfg(feature = "net")]
        {
            let deadline = Instant::now() + self.inner.timing.ready_wait;
            loop {
                let answer = async { self.api().await?.version().await }.await;
                match answer {
                    Ok(version) => return Ok(Some(version)),
                    // A refused sign-in won't change by waiting.
                    Err(error @ Error::Forbidden(_)) => return Err(error),
                    Err(_) if !self.inner.runner.running() => {
                        return Err(Error::Invalid(not_working()));
                    }
                    Err(error) if Instant::now() >= deadline => return Err(error),
                    Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
                }
            }
        }
        #[cfg(not(feature = "net"))]
        Ok(None)
    }

    /// Stops watching and stops the host.
    async fn halt(&self) {
        if let Some(watch) = crate::lock(&self.inner.watch).take() {
            watch.abort();
        }
        let runner = self.inner.runner.clone();
        let _ = tokio::task::spawn_blocking(move || runner.stop()).await;
        self.set_status(HostState::Off, None, None);
    }

    /// When Caskade quits: the host stops with it (blocking). A window
    /// closed to the tray doesn't quit, so the host keeps running.
    pub fn shutdown(&self) {
        if let Some(watch) = crate::lock(&self.inner.watch).take() {
            watch.abort();
        }
        if let Err(error) = self.inner.runner.stop() {
            tracing::warn!(%error, "streaming host didn't stop");
        }
        self.set_status(HostState::Off, None, None);
    }

    /// Devices waiting to pair (#263); expired ones are gone from the list.
    #[cfg(feature = "net")]
    pub async fn pair_requests(&self) -> Result<Vec<crate::streaming::api::PairRequest>> {
        self.api().await?.pair_requests().await
    }

    /// Enters the code device `id` shows, naming it `name`.
    #[cfg(feature = "net")]
    pub async fn accept_pairing(&self, id: &str, pin: &str, name: &str) -> Result<()> {
        let pin = pin.trim();
        if pin.len() != 4 || !pin.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Invalid(crate::tr!("The code is four digits")));
        }
        self.api().await?.accept_pairing(id, pin, name.trim()).await
    }

    #[cfg(feature = "net")]
    pub async fn decline_pairing(&self, id: &str) -> Result<()> {
        self.api().await?.decline_pairing(id).await
    }

    /// The devices paired with this host.
    #[cfg(feature = "net")]
    pub async fn paired_devices(&self) -> Result<Vec<crate::streaming::api::PairedClient>> {
        self.api().await?.paired_clients().await
    }

    #[cfg(feature = "net")]
    pub async fn unpair(&self, uuid: &str) -> Result<()> {
        self.api().await?.unpair(uuid).await
    }

    /// Forgets every paired device: each pairs again with a code.
    #[cfg(feature = "net")]
    pub async fn unpair_all(&self) -> Result<()> {
        self.api().await?.unpair_all().await
    }

    /// The running host's API (publishing games, pairing).
    #[cfg(feature = "net")]
    pub async fn api(&self) -> Result<crate::streaming::api::HostApi> {
        let (credentials, cert) = if self.existing_mode().await {
            let cert = (self.inner.existing)()
                .and_then(|existing| existing.cert)
                .ok_or_else(|| Error::Invalid(not_working()))?;
            (self.inner.secrets.existing().await?, cert)
        } else {
            (
                self.inner.secrets.get().await?,
                self.running_files()?.cert(),
            )
        };
        let credentials = credentials.ok_or_else(|| Error::Invalid(not_working()))?;
        crate::streaming::api::HostApi::with_cert_file(
            self.inner.api_base.clone(),
            credentials,
            &cert,
        )
    }
}

/// The parts [`StreamingHost::with`] takes.
pub struct Parts {
    pub os: OperatingSystem,
    pub runner: Arc<dyn HostRunner>,
    pub existing: Arc<dyn Fn() -> Option<crate::streaming::existing::Existing> + Send + Sync>,
    pub secrets: HostSecrets,
    pub timing: Timing,
    #[cfg(feature = "net")]
    pub api_base: url::Url,
    /// The host's `/serverinfo` port ([`HTTP_PORT`]; another in tests).
    pub http_port: u16,
}
