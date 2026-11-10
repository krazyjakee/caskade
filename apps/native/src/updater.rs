//! Caskade's own updates in the native app (#88): the same feed, key and
//! status as the Tauri app (`caskade_app::self_update`,
//! `caskade_app::commands::app_update::AppUpdater`). While
//! `core.appUpdates.check` is on, and once first-run setup is done, checks
//! run shortly after startup and then every six hours; nothing downloads
//! until the user asks, and the new version runs after a restart.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use caskade_app::commands::app_update::{
    self as app, AppRelease, AppUpdateChanged, AppUpdateStatus, AppUpdater,
};
use caskade_app::events::Event;
use caskade_app::self_update::{self, Bundle, Release};
use caskade_core::settings::SettingsService;

/// The whole download, which is the size of the app.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Download progress reaches the UI at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Why this copy can't replace itself, as the Tauri app says it.
fn unavailable() -> Option<caskade_core::i18n::Message> {
    match Bundle::of_this_copy() {
        Some(_) if self_update::public_key().is_none() => Some(caskade_core::msg!(
            "This copy of Caskade can't update itself: download the new version instead."
        )),
        Some(_) => None,
        None => Some(caskade_core::msg!(
            "Development builds don't update themselves."
        )),
    }
}

static UPDATER: LazyLock<AppUpdater<Release>> = LazyLock::new(|| {
    AppUpdater::new(
        env!("CARGO_PKG_VERSION").to_owned(),
        unavailable(),
        app::ENDPOINT,
    )
});

/// The update status, for the UI.
// Settings → General calls this (#104); the startup dialog and the
// schedule use the rest.
#[allow(dead_code)]
pub fn status() -> AppUpdateStatus {
    UPDATER.status()
}

fn emit(status: AppUpdateStatus) -> AppUpdateStatus {
    crate::shell::sink().send(Event::AppUpdateChanged(AppUpdateChanged(status.clone())));
    status
}

fn release(release: &Release) -> AppRelease {
    AppRelease {
        version: release.version.clone(),
        notes: release.notes.clone(),
        date: release.date.clone(),
    }
}

/// Asks the release feed for a newer version.
pub async fn check() -> Result<AppUpdateStatus, String> {
    emit(UPDATER.begin_check()?);
    let bundle = Bundle::of_this_copy()
        .ok_or_else(|| caskade_core::tr!("Development builds don't update themselves."))?;
    let outcome = self_update::find(UPDATER.endpoint(), env!("CARGO_PKG_VERSION"), bundle)
        .await
        .map(|found| found.map(|found| (release(&found), found)));
    if let Err(error) = &outcome {
        tracing::warn!(%error, "app update check failed");
    }
    Ok(emit(UPDATER.checked(outcome)))
}

/// Downloads and installs the release a check found, in the background.
// Settings → General calls this (#104); the startup dialog and the
// schedule use the rest.
#[allow(dead_code)]
pub fn install() -> Result<AppUpdateStatus, String> {
    let (pending, status) = UPDATER.begin_install()?;
    caskade_app::runtime::spawn(async move {
        let Some(key) = self_update::public_key() else {
            emit(UPDATER.install_failed(
                Some(pending),
                caskade_core::tr!(
                    "This copy of Caskade can't update itself: download the new version instead."
                ),
            ));
            return;
        };
        let mut sent = Instant::now();
        let bytes = self_update::download(&pending, &key, DOWNLOAD_TIMEOUT, |got, total| {
            let status = UPDATER.progress(got, total);
            if sent.elapsed() >= PROGRESS_EVERY {
                sent = Instant::now();
                emit(status);
            }
        })
        .await;
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%error, "app update download failed");
                emit(UPDATER.install_failed(Some(pending), error));
                return;
            }
        };
        emit(UPDATER.installing());
        let version = pending.version.clone();
        let installed = caskade_app::runtime::spawn_blocking(move || {
            let bundle = Bundle::of_this_copy().ok_or_else(|| "not a bundle".to_owned())?;
            self_update::install(bundle, &bytes, &version).map(|()| bundle)
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|installed| installed);
        match installed {
            Ok(bundle) => {
                tracing::info!(version = %pending.version, "app update installed");
                emit(UPDATER.installed());
                // NSIS's installer closes Caskade and starts the new one.
                if bundle == Bundle::Nsis {
                    crate::launch::ask(crate::launch::Request::Quit);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "app update install failed");
                emit(UPDATER.install_failed(
                    Some(pending),
                    caskade_core::tr!("Couldn't install the update: {error}", error = error),
                ));
            }
        }
    });
    Ok(emit(status))
}

/// Checks in the background while automatic checks are on.
pub fn watch(settings: SettingsService) {
    caskade_app::runtime::spawn(async move {
        tokio::time::sleep(app::FIRST_CHECK).await;
        loop {
            if UPDATER.status().unavailable.is_none()
                && app::automatic(&settings).await
                && let Err(error) = check().await
            {
                tracing::debug!(%error, "automatic app update check skipped");
            }
            tokio::time::sleep(app::CHECK_EVERY).await;
        }
    });
}

/// A newer Caskade changed the library in a way this one can't read: a copy
/// that updates itself offers to, installs the newest release and restarts
/// into it; otherwise it says why and quits.
pub fn update_for_library(summary: String) -> ! {
    let quit = caskade_core::tr!("Quit");
    if let Some(reason) = unavailable() {
        crate::dialogs::ask(
            "Caskade",
            &format!("{summary}\n\n{}", reason.render()),
            crate::dialogs::Kind::Error,
            &[quit],
        );
        std::process::exit(1);
    }
    loop {
        let text = caskade_core::tr!("{summary}\n\nUpdate Caskade to open it.", summary = summary);
        if crate::dialogs::ask(
            "Caskade",
            &text,
            crate::dialogs::Kind::Error,
            &[caskade_core::tr!("Update Caskade"), quit.clone()],
        ) != Some(0)
        {
            std::process::exit(1);
        }
        match caskade_app::runtime::block_on(install_now()) {
            Ok(Some(_)) => crate::restart(),
            Ok(None) => {
                crate::dialogs::ask(
                    "Caskade",
                    &caskade_core::tr!(
                        "No released version of Caskade can open this library yet: a development build changed it."
                    ),
                    crate::dialogs::Kind::Error,
                    &[quit],
                );
                std::process::exit(1);
            }
            Err(error) => {
                tracing::warn!(%error, "update from the startup dialog failed");
                if crate::dialogs::ask(
                    "Caskade",
                    &error,
                    crate::dialogs::Kind::Error,
                    &[caskade_core::tr!("Try again"), quit.clone()],
                ) != Some(0)
                {
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Finds, downloads, verifies and installs the newest release now; its
/// version, or `None` when there's none newer.
async fn install_now() -> Result<Option<String>, String> {
    let bundle = Bundle::of_this_copy()
        .ok_or_else(|| caskade_core::tr!("Development builds don't update themselves."))?;
    let Some(found) = self_update::find(app::ENDPOINT, env!("CARGO_PKG_VERSION"), bundle).await?
    else {
        return Ok(None);
    };
    let key = self_update::public_key().ok_or_else(|| {
        caskade_core::tr!(
            "This copy of Caskade can't update itself: download the new version instead."
        )
    })?;
    let bytes = self_update::download(&found, &key, DOWNLOAD_TIMEOUT, |_, _| {}).await?;
    let version = found.version.clone();
    caskade_app::runtime::spawn_blocking(move || self_update::install(bundle, &bytes, &version))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| {
            caskade_core::tr!("Couldn't install the update: {error}", error = error)
        })?;
    tracing::info!(version = %found.version, "app update installed from the startup dialog");
    Ok(Some(found.version))
}
