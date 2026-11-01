//! Caskade's own updates, through tauri-plugin-updater. The release feed
//! and the key its bundles are signed with are fixed at build time: the
//! webview asks to check, install and restart, and never names a URL or a
//! version. The plugin verifies a download's signature before installing it.
//!
//! Only the bundles that can replace themselves update in place (AppImage,
//! NSIS, MSI and the macOS app); a deb, or a development build, says why it
//! can't. While `core.appUpdates.check` is on, and once first-run setup is
//! done, checks run shortly after startup and then every six hours. Nothing
//! downloads until the user asks, and the new version runs after a restart.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use caskade_core::settings::{AppUpdateSettings, SettingsService};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::utils::config::BundleType;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;

/// The release feed: `latest.json` on the newest published GitHub release.
/// A build can point elsewhere with `CASKADE_UPDATER_ENDPOINT`, to try an
/// update before publishing it.
pub const ENDPOINT: &str = match option_env!("CASKADE_UPDATER_ENDPOINT") {
    Some(endpoint) => endpoint,
    None => "https://github.com/krazyjakee/caskade/releases/latest/download/latest.json",
};
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole download, which is the size of the app.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Download progress reaches the webview at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

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
    fn busy(self) -> bool {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct AppUpdateChanged(pub AppUpdateStatus);

/// Why a copy of Caskade from `bundle` can't replace itself.
pub fn unavailable_reason(bundle: Option<BundleType>) -> Option<String> {
    match bundle {
        Some(BundleType::AppImage | BundleType::Nsis | BundleType::Msi | BundleType::App) => None,
        Some(BundleType::Deb | BundleType::Rpm) => Some(caskade_core::tr!(
            "Caskade was installed as a system package: update it with your package manager."
        )),
        Some(_) => Some(caskade_core::tr!(
            "This copy of Caskade can't update itself: download the new version instead."
        )),
        None => Some(caskade_core::tr!(
            "Development builds don't update themselves."
        )),
    }
}

struct Inner<P> {
    status: AppUpdateStatus,
    /// The release the last check found, ready to download.
    pending: Option<P>,
}

/// The update status, and the release waiting to be installed. `P` is the
/// plugin's `Update`, except in tests.
pub struct AppUpdater<P = Update> {
    endpoint: String,
    inner: Mutex<Inner<P>>,
}

impl<P> AppUpdater<P> {
    pub fn new(current_version: String, unavailable: Option<String>, endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.into(),
            inner: Mutex::new(Inner {
                status: AppUpdateStatus {
                    current_version,
                    unavailable,
                    phase: AppUpdatePhase::Idle,
                    release: None,
                    checked_at: None,
                    downloaded: 0,
                    total: None,
                    error: None,
                },
                pending: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner<P>> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
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
        inner.status.checked_at = Some(now_ms());
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

fn emit<R: Runtime>(app: &AppHandle<R>, status: AppUpdateStatus) -> AppUpdateStatus {
    if let Err(error) = AppUpdateChanged(status.clone()).emit(app) {
        tracing::warn!(%error, "app update event not delivered");
    }
    status
}

/// Asks the release feed for a newer version.
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<AppUpdateStatus, String> {
    let updater = app.state::<AppUpdater>();
    emit(app, updater.begin_check()?);
    let outcome = find(app, &updater.endpoint).await.map(|update| {
        update.map(|update| {
            let release = AppRelease {
                version: update.version.clone(),
                notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
                date: update.date.and_then(|date| {
                    date.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                }),
            };
            (release, update)
        })
    });
    if let Err(error) = &outcome {
        tracing::warn!(%error, "app update check failed");
    }
    Ok(emit(app, updater.checked(outcome)))
}

async fn find<R: Runtime>(app: &AppHandle<R>, endpoint: &str) -> Result<Option<Update>, String> {
    let endpoint = endpoint.parse().map_err(|error| {
        caskade_core::tr!(
            "The update feed's address is invalid: {error}",
            error = error
        )
    })?;
    let handle = app.clone();
    app.updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|builder| {
            builder
                .timeout(CHECK_TIMEOUT)
                // The Windows installer replaces Caskade: let the tray icon
                // and the plugins go first.
                .on_before_exit(move || handle.cleanup_before_exit())
                .build()
        })
        .map_err(|error| caskade_core::tr!("Couldn't check for updates: {error}", error = error))?
        .check()
        .await
        .map_err(|error| caskade_core::tr!("Couldn't check for updates: {error}", error = error))
}

/// Downloads the release the last check found and installs it, in the
/// background; the status follows through `AppUpdateChanged`.
pub fn install<R: Runtime>(app: AppHandle<R>) -> Result<AppUpdateStatus, String> {
    let (mut update, status) = app.state::<AppUpdater>().begin_install()?;
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    tauri::async_runtime::spawn({
        let app = app.clone();
        async move {
            let updater = app.state::<AppUpdater>();
            let version = update.version.clone();
            let activity = caskade_core::jobs::global().activity(caskade_core::tr!(
                "Downloading Caskade {version}",
                version = version
            ));
            let mut sent = Instant::now();
            let bytes = update
                .download(
                    |chunk, total| {
                        let status = updater.progress(chunk as u64, total);
                        if sent.elapsed() >= PROGRESS_EVERY {
                            sent = Instant::now();
                            if let Some(total) = total.filter(|total| *total > 0) {
                                activity.set_label(caskade_core::tr!(
                                    "Downloading Caskade {version} ({percent}%)",
                                    version = version,
                                    percent = status.downloaded * 100 / total
                                ));
                            }
                            emit(&app, status);
                        }
                    },
                    || {},
                )
                .await;
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!(%error, "app update download failed");
                    emit(
                        &app,
                        updater.install_failed(
                            Some(update),
                            caskade_core::tr!(
                                "Couldn't download the update: {error}",
                                error = error
                            ),
                        ),
                    );
                    return;
                }
            };
            activity.set_label(caskade_core::tr!(
                "Installing Caskade {version}",
                version = version
            ));
            emit(&app, updater.installing());
            // Installing writes the new bundle over this one.
            let installed = tauri::async_runtime::spawn_blocking(move || {
                let installed = update.install(&bytes);
                (update, installed)
            })
            .await;
            let status = match installed {
                Ok((_, Ok(()))) => {
                    tracing::info!(%version, "app update installed");
                    updater.installed()
                }
                Ok((update, Err(error))) => {
                    tracing::warn!(%error, "app update install failed");
                    updater.install_failed(
                        Some(update),
                        caskade_core::tr!("Couldn't install the update: {error}", error = error),
                    )
                }
                Err(error) => updater.install_failed(
                    None,
                    caskade_core::tr!("Couldn't install the update: {error}", error = error),
                ),
            };
            emit(&app, status);
        }
    });
    Ok(emit(&app, status))
}

/// Checks shortly after startup and then every six hours, while the setting
/// allows it and first-run setup is done. Spawned on the current Tokio
/// runtime, for `background::supervise`.
pub fn watch<R: Runtime>(
    app: AppHandle<R>,
    settings: SettingsService,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if app.state::<AppUpdater>().status().unavailable.is_none()
                && automatic(&settings).await
                && let Err(error) = check(&app).await
            {
                tracing::debug!(%error, "automatic app update check skipped");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    })
}

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

/// The update status: the version running, and what the last check found.
#[tauri::command]
#[specta::specta]
pub fn app_update_status(updater: State<'_, AppUpdater>) -> AppUpdateStatus {
    updater.status()
}

/// Checks the release feed now. Refused while this copy can't update,
/// while a check or an install runs, and once an update is installed.
#[tauri::command]
#[specta::specta]
pub async fn app_update_check<R: Runtime>(app: AppHandle<R>) -> Result<AppUpdateStatus, String> {
    check(&app).await
}

/// Downloads and installs the release the last check found. Returns at
/// once; `AppUpdateChanged` follows the download.
#[tauri::command]
#[specta::specta]
pub fn app_update_install<R: Runtime>(app: AppHandle<R>) -> Result<AppUpdateStatus, String> {
    install(app)
}

/// Restarts into the installed update.
#[tauri::command]
#[specta::specta]
pub fn app_update_restart<R: Runtime>(
    app: AppHandle<R>,
    updater: State<'_, AppUpdater>,
) -> Result<(), String> {
    updater.ensure_ready()?;
    app.restart()
}

#[cfg(test)]
#[path = "app_update_tests.rs"]
mod tests;
