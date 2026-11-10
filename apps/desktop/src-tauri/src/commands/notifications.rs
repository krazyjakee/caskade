//! The notification feed (#198): pages of it, the unread count and read
//! state. The feed lives in the core (`caskade_achievements::notifications`);
//! `NotificationsChanged` tells the webview to read it again.
pub use app::*;
use caskade_achievements::notifications::{MemberPage, NotificationPage, Notifications};
use caskade_app::commands::notifications as app;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event as _;
/// Manages the feed (which the app layer follows and sends to the webview)
/// and notes Caskade's own updates in it.
pub fn setup<R: Runtime>(app: &AppHandle<R>, notifications: Notifications) {
    app.manage(notifications.clone());
    // Caskade's updates: noted while one is on offer, resolved once none is.
    super::app_update::AppUpdateChanged::listen_any(app, move |event| {
        use super::app_update::AppUpdatePhase::*;
        let status = event.payload.0.0;
        let version = match status.phase {
            Available => status.release.map(|release| release.version),
            UpToDate | Downloading | Installing | Ready => None,
            Idle | Checking | Failed => return,
        };
        let notifications = notifications.clone();
        caskade_app::runtime::spawn(async move {
            if let Err(error) = notifications.set_app_update(version.as_deref()).await {
                tracing::warn!(%error, "app update notification not recorded");
            }
        });
    });
}

/// A page of the feed, newest change first; only unread items with
/// `unreadOnly`.
#[tauri::command]
#[specta::specta]
pub async fn notifications_list(
    notifications: State<'_, Notifications>,
    unread_only: bool,
    offset: u32,
    limit: u32,
) -> Result<NotificationPage, String> {
    app::notifications_list(&notifications, unread_only, offset, limit).await
}

/// How many items are unread, for the bell.
#[tauri::command]
#[specta::specta]
pub async fn notifications_unread(notifications: State<'_, Notifications>) -> Result<u32, String> {
    app::notifications_unread(&notifications).await
}

/// What an item is about (the games of a batch, the updates on offer),
/// each followed to the game it is now.
#[tauri::command]
#[specta::specta]
pub async fn notification_members(
    notifications: State<'_, Notifications>,
    id: String,
    offset: u32,
    limit: u32,
) -> Result<MemberPage, String> {
    app::notification_members(&notifications, id, offset, limit).await
}

/// Marks an item read up to the `version` the webview showed: a change
/// since leaves it unread.
#[tauri::command]
#[specta::specta]
pub async fn notification_mark_read(
    notifications: State<'_, Notifications>,
    id: String,
    version: i64,
) -> Result<bool, String> {
    app::notification_mark_read(&notifications, id, version).await
}

#[tauri::command]
#[specta::specta]
pub async fn notification_mark_unread(
    notifications: State<'_, Notifications>,
    id: String,
) -> Result<bool, String> {
    app::notification_mark_unread(&notifications, id).await
}

/// Marks read what the feed held at `revision` (a page's): items changed
/// since stay unread. How many changed.
#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_all_read(
    notifications: State<'_, Notifications>,
    revision: i64,
) -> Result<u32, String> {
    app::notifications_mark_all_read(&notifications, revision).await
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
