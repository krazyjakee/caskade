//! Caskade's own updates: the status a check and an install move through,
//! which every shell shows, and installing a `.deb` with one polkit prompt.
//! The release feed and its signing key are fixed at build time; finding,
//! downloading and verifying a release is the shell's updater's (the Tauri
//! plugin in the desktop app).

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use caskade_core::i18n::Message;
use caskade_core::settings::{AppUpdateSettings, SettingsService};
use serde::{Deserialize, Serialize};
use specta::Type;

/// The release feed: `latest.json` on the newest published GitHub release.
/// A build can point elsewhere with `CASKADE_UPDATER_ENDPOINT`, to try an
/// update before publishing it.
pub const ENDPOINT: &str = match option_env!("CASKADE_UPDATER_ENDPOINT") {
    Some(endpoint) => endpoint,
    None => "https://github.com/krazyjakee/caskade/releases/latest/download/latest.json",
};
/// How long after startup the first automatic check runs.
const FIRST_CHECK: Duration = Duration::from_secs(30);
/// How often automatic checks run after that.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The whole download, which is the size of the app.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Download progress reaches the UI at most this often.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Where an update is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AppUpdatePhase {
    /// Not checked since startup.
    Idle,
    Checking,
    UpToDate,
    /// `release` is newer than this version.
    Available,
    Downloading,
    Installing,
    /// Installed: the new version runs after a restart.
    Ready,
    /// The last check or install failed (`error`). After a failed install
    /// the release can still be installed again.
    Failed,
}

impl AppUpdatePhase {
    /// A check or an install runs.
    pub fn busy(self) -> bool {
        matches!(self, Self::Checking | Self::Downloading | Self::Installing)
    }
}

/// A newer release of Caskade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppRelease {
    pub version: String,
    /// The release notes, Markdown.
    pub notes: Option<String>,
    /// RFC 3339.
    pub date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub current_version: String,
    /// Why this copy of Caskade can't update itself; `None` when it can.
    pub unavailable: Option<String>,
    pub phase: AppUpdatePhase,
    /// What the last check found.
    pub release: Option<AppRelease>,
    /// When the last check finished, Unix milliseconds.
    pub checked_at: Option<i64>,
    pub downloaded: u64,
    /// The download's size, once the server has said.
    pub total: Option<u64>,
    pub error: Option<String>,
}

/// Every change to the update status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppUpdateChanged(pub AppUpdateStatus);

struct Inner<P> {
    status: AppUpdateStatus,
    /// The release the last check found, ready to download.
    pending: Option<P>,
}

/// The update status, and the release waiting to be installed. `P` is the
/// release a shell's updater found (the Tauri plugin's `Update`), or
/// anything in tests.
pub struct AppUpdater<P> {
    endpoint: String,
    /// Why this copy can't replace itself: the reason, not its text, so the
    /// status follows a language switch.
    unavailable: Option<Message>,
    inner: Mutex<Inner<P>>,
}

impl<P> AppUpdater<P> {
    pub fn new(current_version: String, unavailable: Option<Message>, endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.into(),
            inner: Mutex::new(Inner {
                status: AppUpdateStatus {
                    current_version,
                    unavailable: unavailable.as_ref().map(Message::render),
                    phase: AppUpdatePhase::Idle,
                    release: None,
                    checked_at: None,
                    downloaded: 0,
                    total: None,
                    error: None,
                },
                pending: None,
            }),
            unavailable,
        }
    }

    /// The state, its text in the language in use: every status goes out
    /// through here.
    fn lock(&self) -> MutexGuard<'_, Inner<P>> {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner.status.unavailable = self.unavailable.as_ref().map(Message::render);
        inner
    }

    /// The release feed this updater checks.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn status(&self) -> AppUpdateStatus {
        self.lock().status.clone()
    }

    /// Refuses while this copy can't update, while a check or an install
    /// runs, and once an update is installed.
    fn ensure_idle(status: &AppUpdateStatus) -> Result<(), String> {
        if let Some(reason) = &status.unavailable {
            return Err(reason.clone());
        }
        if status.phase.busy() {
            return Err(caskade_core::tr!(
                "Caskade is already checking for or installing an update."
            ));
        }
        if status.phase == AppUpdatePhase::Ready {
            return Err(caskade_core::tr!(
                "An update is installed: restart Caskade to finish."
            ));
        }
        Ok(())
    }

    /// Starts a check, forgetting what the last one found.
    pub fn begin_check(&self) -> Result<AppUpdateStatus, String> {
        let mut inner = self.lock();
        Self::ensure_idle(&inner.status)?;
        inner.pending = None;
        let status = &mut inner.status;
        status.phase = AppUpdatePhase::Checking;
        status.release = None;
        status.error = None;
        status.downloaded = 0;
        status.total = None;
        Ok(status.clone())
    }

    /// A check's outcome: a newer release, none, or why it failed.
    pub fn checked(&self, outcome: Result<Option<(AppRelease, P)>, String>) -> AppUpdateStatus {
        let mut inner = self.lock();
        inner.status.checked_at = Some(caskade_core::now_ms());
        match outcome {
            Ok(Some((release, pending))) => {
                inner.status.phase = AppUpdatePhase::Available;
                inner.status.release = Some(release);
                inner.pending = Some(pending);
            }
            Ok(None) => inner.status.phase = AppUpdatePhase::UpToDate,
            Err(error) => {
                inner.status.phase = AppUpdatePhase::Failed;
                inner.status.error = Some(error);
            }
        }
        inner.status.clone()
    }

    /// Takes the release the last check found, to download it.
    pub fn begin_install(&self) -> Result<(P, AppUpdateStatus), String> {
        let mut inner = self.lock();
        Self::ensure_idle(&inner.status)?;
        let pending = inner.pending.take().ok_or_else(|| {
            caskade_core::tr!("Check for updates first: there's no new version to install.")
        })?;
        let status = &mut inner.status;
        status.phase = AppUpdatePhase::Downloading;
        status.error = None;
        status.downloaded = 0;
        status.total = None;
        Ok((pending, status.clone()))
    }

    pub fn progress(&self, chunk: u64, total: Option<u64>) -> AppUpdateStatus {
        let mut inner = self.lock();
        inner.status.downloaded += chunk;
        inner.status.total = total;
        inner.status.clone()
    }

    pub fn installing(&self) -> AppUpdateStatus {
        self.set_phase(AppUpdatePhase::Installing)
    }

    pub fn installed(&self) -> AppUpdateStatus {
        self.set_phase(AppUpdatePhase::Ready)
    }

    fn set_phase(&self, phase: AppUpdatePhase) -> AppUpdateStatus {
        let mut inner = self.lock();
        inner.status.phase = phase;
        inner.status.clone()
    }

    /// The download or install failed; with `pending` back, it can be
    /// tried again without another check.
    pub fn install_failed(&self, pending: Option<P>, error: String) -> AppUpdateStatus {
        let mut inner = self.lock();
        if pending.is_some() {
            inner.pending = pending;
        }
        inner.status.phase = AppUpdatePhase::Failed;
        inner.status.error = Some(error);
        inner.status.clone()
    }

    pub fn ensure_ready(&self) -> Result<(), String> {
        if self.lock().status.phase == AppUpdatePhase::Ready {
            Ok(())
        } else {
            Err(caskade_core::tr!("Install an update before restarting."))
        }
    }
}

/// Installs a downloaded `.deb` over this copy with one polkit prompt.
/// The updater plugin's own deb install falls back to a password box whose
/// answer it pipes into sudo, then to sudo on a terminal nobody may see:
/// Caskade never handles the password, so it asks pkexec alone.
#[cfg(target_os = "linux")]
pub mod deb {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// pkexec's exit status when the user dismissed its prompt.
    const DISMISSED: i32 = 126;
    /// pkexec's exit status when the account isn't allowed, or no prompt
    /// could be shown.
    const NOT_AUTHORIZED: i32 = 127;

    pub fn install(bytes: &[u8], version: &str) -> Result<(), String> {
        install_with(&["pkexec".into()], bytes, version)
    }

    /// `elevate` runs the package manager as root: pkexec, or a stand-in
    /// in tests.
    pub fn install_with(elevate: &[OsString], bytes: &[u8], version: &str) -> Result<(), String> {
        if !bytes.starts_with(b"!<arch>\ndebian-binary") {
            return Err(caskade_core::tr!("the download isn't a Debian package"));
        }
        let scratch = Scratch::new().map_err(|error| error.to_string())?;
        let package = scratch.0.join(format!("caskade_{version}_amd64.deb"));
        std::fs::write(&package, bytes).map_err(|error| error.to_string())?;
        let (program, args) = elevate.split_first().expect("an elevation command");
        let mut command = Command::new(program);
        command.args(args).args(package_manager()).arg(&package);
        caskade_core::managed::outside_own_appimage(&mut command);
        let output = command.output().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                caskade_core::tr!(
                    "pkexec isn't installed, so Caskade can't ask for permission. Install the new .deb from the release page yourself."
                )
            } else {
                error.to_string()
            }
        })?;
        match output.status.code() {
            Some(0) => Ok(()),
            Some(DISMISSED) => Err(caskade_core::tr!("the password prompt was dismissed")),
            Some(NOT_AUTHORIZED) => Err(caskade_core::tr!(
                "this account may not install packages, or no password prompt could be shown"
            )),
            _ => Err(caskade_core::tr!(
                "the package manager failed: {detail}",
                detail = last_lines(&output.stderr)
                    .or_else(|| last_lines(&output.stdout))
                    .unwrap_or_else(|| output.status.to_string())
            )),
        }
    }

    /// apt-get, which also fetches any package the new version came to
    /// need, waiting up to two minutes for another package manager to
    /// finish; dpkg where there's no apt.
    fn package_manager() -> &'static [&'static str] {
        if Path::new("/usr/bin/apt-get").exists() {
            &[
                "/usr/bin/apt-get",
                "install",
                "--yes",
                "-o",
                "DPkg::Lock::Timeout=120",
            ]
        } else {
            &["/usr/bin/dpkg", "--install"]
        }
    }

    fn last_lines(output: &[u8]) -> Option<String> {
        let text = String::from_utf8_lossy(output);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" ");
        (!tail.is_empty()).then_some(tail)
    }

    /// A folder only this user can open, removed afterwards. Root reads
    /// the package from it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> std::io::Result<Self> {
            use std::os::unix::fs::DirBuilderExt;
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "caskade-update-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            if let Err(error) = std::fs::remove_dir_all(&path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(error);
            }
            std::fs::DirBuilder::new().mode(0o700).create(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Checks with `check` shortly after startup and then every six hours,
/// while this copy can update, the setting allows it and first-run setup is
/// done.
pub async fn watch<P, F>(updater: &AppUpdater<P>, settings: &SettingsService, check: impl Fn() -> F)
where
    F: Future<Output = Result<AppUpdateStatus, String>>,
{
    tokio::time::sleep(FIRST_CHECK).await;
    loop {
        if updater.status().unavailable.is_none()
            && automatic(settings).await
            && let Err(error) = check().await
        {
            tracing::debug!(%error, "automatic app update check skipped");
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

/// Whether checks run on their own: the setting is on and first-run setup
/// is done.
async fn automatic(settings: &SettingsService) -> bool {
    let check = match settings.get("core.appUpdates").await {
        Ok(snapshot) => {
            serde_json::from_value::<AppUpdateSettings>(snapshot.value)
                .unwrap_or_default()
                .check
        }
        Err(error) => {
            tracing::warn!(%error, "app update settings unavailable");
            false
        }
    };
    check && crate::onboarding::completed(settings).await
}

#[cfg(test)]
#[path = "app_update_tests.rs"]
mod tests;
