//! What the shell's status bar and Activity button read (#96): jobs, unread
//! notifications, paired devices, the app's update and its background
//! tasks. They come from the app's services, loaded again when their app
//! event says they changed; fixtures mode has no services, so they read as
//! nothing running, nothing unread, no devices and no update.

use super::{DataSource, Query};
use caskade_achievements::notifications::{MemberPage, NotificationPage};
use caskade_app::background::AppStatus;
use caskade_app::commands::app_update::AppUpdateStatus;
use caskade_core::devices::DevicesStatus;
use caskade_core::install::JobSnapshot;
use caskade_ui_data::{Changed, ErrorReport, Key as DataKey};
use dioxus_native::prelude::*;
use std::future::Future;
use tokio::sync::broadcast;

type Services = &'static caskade_app::start::Services;

/// `load` over the app's services, again whenever an event makes `key`
/// stale; `None` without services (fixtures mode).
fn use_status<T, F, Fut>(key: DataKey, mut load: F) -> Query<Option<T>>
where
    T: Send + 'static,
    F: FnMut(Services) -> Fut + 'static,
    Fut: Future<Output = Result<T, ErrorReport>> + Send + 'static,
{
    let source = use_context::<DataSource>();
    let mut resource = use_resource(move || {
        let work = crate::shell::services().map(&mut load);
        async move {
            match work {
                Some(work) => super::change(work).await.map(Some),
                None => Ok(None),
            }
        }
    });
    use_hook(move || {
        let mut events = source.events.subscribe();
        spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) if Changed::from_event(&event).affects(&key) => resource.restart(),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => resource.restart(),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
    resource
}

fn report(error: impl std::fmt::Display) -> ErrorReport {
    ErrorReport::from_error(&caskade_core::Error::Invalid(error.to_string()))
}

/// Every job: installs, downloads, moves.
pub fn use_jobs() -> Query<Option<Vec<JobSnapshot>>> {
    use_status(DataKey::Jobs, |services| async move {
        Ok(services
            .install
            .as_ref()
            .map(caskade_app::commands::install::jobs_list)
            .unwrap_or_default())
    })
}

/// How many notifications are unread.
pub fn use_unread_notifications() -> Query<Option<u32>> {
    use_status(DataKey::Notifications, |services| async move {
        caskade_app::commands::notifications::notifications_unread(&services.notifications)
            .await
            .map_err(report)
    })
}

/// Paired and nearby devices.
pub fn use_devices() -> Query<Option<DevicesStatus>> {
    use_status(DataKey::Devices, |services| async move {
        caskade_app::commands::sync::devices_status(&services.devices)
            .await
            .map_err(report)
    })
}

/// Whether a new Caskade is available, being downloaded or ready.
pub fn use_app_update() -> Query<Option<AppUpdateStatus>> {
    use_status(DataKey::AppUpdate, |_| async move {
        Ok(crate::updater::status())
    })
}

/// The app's version and its background tasks.
pub fn use_background() -> Query<Option<AppStatus>> {
    use_status(DataKey::Background, |_| async move {
        Ok(caskade_app::background::background_status(
            env!("CARGO_PKG_VERSION").to_owned(),
        ))
    })
}

/// Whether `job` is under way: downloading or installing (the webview's
/// `running`).
pub fn is_running(job: &JobSnapshot) -> bool {
    use caskade_core::install::JobStatus;
    matches!(job.status, JobStatus::Downloading | JobStatus::Installing)
}

/// Whether `job` still runs or waits: it counts on the Activity button
/// (`api/install.ts`'s `isActiveJob`).
pub fn is_active(job: &JobSnapshot) -> bool {
    use caskade_core::install::JobStatus;
    is_running(job) || matches!(job.status, JobStatus::Queued | JobStatus::NeedsUser)
}

/// A page of the notification feed, newest change first; only unread items
/// with `unread_only`.
pub fn use_notifications(
    unread_only: ReadSignal<bool>,
    offset: ReadSignal<u32>,
) -> Query<Option<NotificationPage>> {
    use_status(DataKey::Notifications, move |services| {
        let (unread_only, offset) = (unread_only(), offset());
        async move {
            caskade_app::commands::notifications::notifications_list(
                &services.notifications,
                unread_only,
                offset,
                NOTIFICATION_PAGE,
            )
            .await
            .map_err(report)
        }
    })
}

/// What item `id` is about: its first `limit` games or updates.
pub fn use_notification_members(id: String, limit: ReadSignal<u32>) -> Query<Option<MemberPage>> {
    use_status(DataKey::Notifications, move |services| {
        let (id, limit) = (id.clone(), limit());
        async move {
            caskade_app::commands::notifications::notification_members(
                &services.notifications,
                id,
                0,
                limit,
            )
            .await
            .map_err(report)
        }
    })
}

/// The feed's page size (`api/notifications.ts`'s `NOTIFICATION_PAGE`).
pub const NOTIFICATION_PAGE: u32 = 20;

/// A change to the feed's read state.
pub enum ReadChange {
    /// Read up to the version shown: a change since leaves it unread.
    Read {
        id: String,
        version: i64,
    },
    Unread {
        id: String,
    },
    /// Everything the feed held at `revision`.
    AllRead {
        revision: i64,
    },
}

/// Marks items read or unread; the feed's event then refreshes it.
pub async fn change_read(change: ReadChange) -> Result<(), ErrorReport> {
    let Some(services) = crate::shell::services() else {
        return Ok(());
    };
    use caskade_app::commands::notifications as feed;
    super::change(async move {
        let notifications = &services.notifications;
        match change {
            ReadChange::Read { id, version } => {
                feed::notification_mark_read(notifications, id, version)
                    .await
                    .map(drop)
            }
            ReadChange::Unread { id } => feed::notification_mark_unread(notifications, id)
                .await
                .map(drop),
            ReadChange::AllRead { revision } => {
                feed::notifications_mark_all_read(notifications, revision)
                    .await
                    .map(drop)
            }
        }
        .map_err(report)
    })
    .await
}
