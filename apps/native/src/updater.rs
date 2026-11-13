//! Caskade's own updates in the native app (#88): the same feed, key and
//! status as the Tauri app (`caskade_app::self_update`,
//! `caskade_app::commands::app_update::AppUpdater`). While
//! `core.appUpdates.check` is on, and once first-run setup is done, checks
//! run shortly after startup and then every six hours; nothing downloads
//! until the user asks, and the new version runs after a restart.

use std::sync::LazyLock;
use std::time::Instant;

use caskade_app::commands::app_update::{
    self as app, AppRelease, AppUpdateChanged, AppUpdateStatus, AppUpdater,
};
use caskade_app::events::Event;
use caskade_app::self_update::{self, Bundle, Release};
use caskade_core::settings::SettingsService;

/// Why this copy can't replace itself, as the Tauri app says it.
pub(crate) fn unavailable() -> Option<caskade_core::i18n::Message> {
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
pub fn status() -> AppUpdateStatus {
    #[cfg(test)]
    if let Some(status) = fake::status() {
        return status;
    }
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
    #[cfg(test)]
    if let Some(status) = fake::call("check") {
        return Ok(status);
    }
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
pub fn install() -> Result<AppUpdateStatus, String> {
    #[cfg(test)]
    if let Some(status) = fake::call("install") {
        return Ok(status);
    }
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
        let bytes = self_update::download(&pending, &key, app::DOWNLOAD_TIMEOUT, |got, total| {
            let status = UPDATER.progress(got, total);
            if sent.elapsed() >= app::PROGRESS_EVERY {
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

/// Starts the version just installed, as Settings' Restart now: only once
/// an update is ready.
pub fn restart() -> Result<(), String> {
    #[cfg(test)]
    if fake::call("restart").is_some() {
        return Ok(());
    }
    UPDATER.ensure_ready()?;
    crate::restart()
}

/// Checks in the background while automatic checks are on.
pub fn watch(settings: SettingsService) {
    caskade_app::runtime::spawn(async move { app::watch(&*UPDATER, &settings, check).await });
}

/// Finds, downloads, verifies and installs the newest release now; its
/// version, or `None` when there's none newer.
pub(crate) async fn install_now() -> Result<Option<String>, String> {
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
    let bytes = self_update::download(&found, &key, app::DOWNLOAD_TIMEOUT, |_, _| {}).await?;
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

/// A test's stand-in for the release feed, as the webview's tests fake
/// `AppUpdateApi`: a check finds 0.2.0, an install starts downloading, and
/// the test moves the status on with [`fake::emit`].
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use caskade_app::commands::app_update::AppUpdatePhase;
    use std::sync::Mutex;

    struct Fake {
        status: AppUpdateStatus,
        calls: Vec<&'static str>,
    }

    static FAKE: Mutex<Option<Fake>> = Mutex::new(None);

    fn fake() -> std::sync::MutexGuard<'static, Option<Fake>> {
        caskade_core::lock(&FAKE)
    }

    /// Fakes the updater from `status` until the guard drops.
    pub(crate) fn start(status: AppUpdateStatus) -> Faked {
        *fake() = Some(Fake {
            status,
            calls: Vec::new(),
        });
        Faked
    }

    pub(crate) struct Faked;

    impl Drop for Faked {
        fn drop(&mut self) {
            *fake() = None;
        }
    }

    pub(super) fn status() -> Option<AppUpdateStatus> {
        fake().as_ref().map(|fake| fake.status.clone())
    }

    /// What the UI asked for, in order.
    pub(crate) fn calls() -> Vec<&'static str> {
        fake()
            .as_ref()
            .map(|fake| fake.calls.clone())
            .unwrap_or_default()
    }

    pub(super) fn call(what: &'static str) -> Option<AppUpdateStatus> {
        let next = {
            let mut fake = fake();
            let fake = fake.as_mut()?;
            fake.calls.push(what);
            match what {
                "check" => {
                    fake.status.phase = AppUpdatePhase::Available;
                    fake.status.release = Some(AppRelease {
                        version: "0.2.0".into(),
                        notes: Some("Faster library scans.".into()),
                        date: None,
                    });
                    fake.status.checked_at = Some(1_700_000_000_000);
                }
                "install" => fake.status.phase = AppUpdatePhase::Downloading,
                _ => {}
            }
            fake.status.clone()
        };
        emit(next.clone());
        Some(next)
    }

    /// Moves the status on, as the running updater's events do.
    pub(crate) fn emit(status: AppUpdateStatus) {
        if let Some(fake) = fake().as_mut() {
            fake.status = status.clone();
        }
        let _ = crate::frame::tests::source()
            .events
            .send(Event::AppUpdateChanged(AppUpdateChanged(status)));
    }
}
