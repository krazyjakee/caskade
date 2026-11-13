//! Caskade's own updates, through tauri-plugin-updater. The release feed
//! and the key its bundles are signed with are fixed at build time: the
//! webview asks to check, install and restart, and never names a URL or a
//! version. The plugin verifies a download's signature before installing it.
//!
//! The AppImage, NSIS, MSI and the macOS app replace themselves; a deb is
//! installed over this one through one polkit prompt (`deb`). An rpm, or a
//! development build, says why it can't update. While `core.appUpdates.check` is on, and once first-run setup is
//! done, checks run shortly after startup and then every six hours. Nothing
//! downloads until the user asks, and the new version runs after a restart.
//!
//! The status and the deb install are `caskade_app::commands::app_update`'s;
//! the plugin's part stays here.

use std::time::{Duration, Instant};

use crate::error::{CommandError, refused};
use caskade_app::commands::app_update as app;
use caskade_core::i18n::Message;
use caskade_core::settings::SettingsService;
use tauri::utils::config::BundleType;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The webview's event, which notifications and the tests listen for.
pub use crate::events::AppUpdateChanged;
#[cfg(target_os = "linux")]
use app::deb;
pub use app::{AppRelease, AppUpdatePhase, AppUpdateStatus, ENDPOINT};

/// The update status, and the release waiting to be installed: the
/// plugin's `Update`, except in tests.
pub type AppUpdater<P = Update> = app::AppUpdater<P>;

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a copy of Caskade from `bundle` can't replace itself.
pub fn unavailable_reason(bundle: Option<BundleType>) -> Option<Message> {
    match bundle {
        Some(BundleType::AppImage | BundleType::Nsis | BundleType::Msi | BundleType::App) => None,
        Some(BundleType::Deb) if cfg!(target_os = "linux") => None,
        Some(BundleType::Deb | BundleType::Rpm) => Some(caskade_core::msg!(
            "Caskade was installed as a system package: update it with your package manager."
        )),
        Some(_) => Some(caskade_core::msg!(
            "This copy of Caskade can't update itself: download the new version instead."
        )),
        None => Some(caskade_core::msg!(
            "Development builds don't update themselves."
        )),
    }
}

fn emit<R: Runtime>(app: &AppHandle<R>, status: AppUpdateStatus) -> AppUpdateStatus {
    crate::events::sink(app.clone()).send(caskade_app::events::Event::AppUpdateChanged(
        app::AppUpdateChanged(status.clone()),
    ));
    status
}

/// Asks the release feed for a newer version.
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<AppUpdateStatus, String> {
    let updater = app.state::<AppUpdater>();
    emit(app, updater.begin_check()?);
    let outcome = find(app, updater.endpoint()).await.map(|update| {
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
    update.timeout = Some(app::DOWNLOAD_TIMEOUT);
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
                        if sent.elapsed() >= app::PROGRESS_EVERY {
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
                let installed = put_in_place(&update, &bytes);
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

/// Finds, downloads and installs the newest release at once, without the
/// status or the library: a library a newer Caskade wrote doesn't open
/// here, so only the startup dialog is left to ask. Returns the installed
/// version, or `None` when the feed has nothing newer.
pub async fn install_now<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>, String> {
    let Some(mut update) = find(app, ENDPOINT).await? else {
        return Ok(None);
    };
    update.timeout = Some(app::DOWNLOAD_TIMEOUT);
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|error| {
        caskade_core::tr!("Couldn't download the update: {error}", error = error)
    })?;
    let version = update.version.clone();
    tauri::async_runtime::spawn_blocking(move || put_in_place(&update, &bytes))
        .await
        .map_err(|error| error.to_string())
        .and_then(|installed| installed)
        .map_err(|error| {
            caskade_core::tr!("Couldn't install the update: {error}", error = error)
        })?;
    tracing::info!(%version, "app update installed from the startup dialog");
    Ok(Some(version))
}

/// Puts a downloaded release in place of this copy, once its signature
/// checks out.
fn put_in_place(update: &Update, bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if matches!(tauri::utils::platform::bundle_type(), Some(BundleType::Deb)) {
        update.verify(bytes).map_err(|error| error.to_string())?;
        return deb::install(bytes, &update.version);
    }
    update.install(bytes).map_err(|error| error.to_string())
}

/// Checks shortly after startup and then every six hours, while the setting
/// allows it and first-run setup is done. Spawned on the current Tokio
/// runtime, for `background::supervise`.
pub fn watch<R: Runtime>(
    app: AppHandle<R>,
    settings: SettingsService,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        app::watch(app.state::<AppUpdater>().inner(), &settings, || check(&app)).await;
    })
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
pub async fn app_update_check<R: Runtime>(
    app: AppHandle<R>,
) -> Result<AppUpdateStatus, CommandError> {
    check(&app).await.map_err(refused)
}

/// Downloads and installs the release the last check found. Returns at
/// once; `AppUpdateChanged` follows the download.
#[tauri::command]
#[specta::specta]
pub fn app_update_install<R: Runtime>(app: AppHandle<R>) -> Result<AppUpdateStatus, CommandError> {
    install(app).map_err(refused)
}

/// Restarts into the installed update.
#[tauri::command]
#[specta::specta]
pub fn app_update_restart<R: Runtime>(
    app: AppHandle<R>,
    updater: State<'_, AppUpdater>,
) -> Result<(), CommandError> {
    updater.ensure_ready().map_err(refused)?;
    crate::tray::restart(&app);
    Ok(())
}

#[cfg(test)]
#[path = "app_update_tests.rs"]
mod tests;
