//! The notification feed (#198): pages of it, the unread count and read
//! state. The feed lives in the core (`caskade_core::notifications`);
//! `NotificationsChanged` tells the webview to read it again.
use caskade_core::notifications::{MemberPage, NotificationPage, Notifications};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

/// The feed or its read state changed: read it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsChanged {
    pub unread: u32,
}

/// Manages the feed, follows the bus for it, forwards its changes to the
/// webview and notes Caskade's own updates.
pub fn setup<R: Runtime>(
    app: &AppHandle<R>,
    notifications: Notifications,
    bus: &caskade_core::bus::Bus,
) {
    app.manage(notifications.clone());
    crate::background::supervise("notifications", {
        let (notifications, bus) = (notifications.clone(), bus.clone());
        move || notifications.watch(&bus)
    });
    let mut changes = notifications.subscribe();
    tauri::async_runtime::spawn({
        let (app, notifications) = (app.clone(), notifications.clone());
        async move {
            while changes.changed().await.is_ok() {
                // A failed count keeps the badge's last one rather than
                // showing 0 (#160); the next change tries again.
                let unread = match notifications.unread().await {
                    Ok(unread) => unread,
                    Err(error) => {
                        tracing::warn!(%error, "unread notifications not counted");
                        continue;
                    }
                };
                let changed = NotificationsChanged { unread };
                if let Err(error) = changed.emit(&app) {
                    tracing::warn!(%error, "notifications event not delivered");
                }
            }
        }
    });
    // Caskade's updates: noted while one is on offer, resolved once none is.
    super::app_update::AppUpdateChanged::listen_any(app, move |event| {
        use super::app_update::AppUpdatePhase::*;
        let status = event.payload.0;
        let version = match status.phase {
            Available => status.release.map(|release| release.version),
            UpToDate | Downloading | Installing | Ready => None,
            Idle | Checking | Failed => return,
        };
        let notifications = notifications.clone();
        tauri::async_runtime::spawn(async move {
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
    notifications
        .list(unread_only, offset, limit)
        .await
        .map_err(|error| error.to_string())
}

/// How many items are unread, for the bell.
#[tauri::command]
#[specta::specta]
pub async fn notifications_unread(notifications: State<'_, Notifications>) -> Result<u32, String> {
    notifications
        .unread()
        .await
        .map_err(|error| error.to_string())
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
    notifications
        .members(&id, offset, limit)
        .await
        .map_err(|error| error.to_string())
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
    notifications
        .mark_read(&id, version)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn notification_mark_unread(
    notifications: State<'_, Notifications>,
    id: String,
) -> Result<bool, String> {
    notifications
        .mark_unread(&id)
        .await
        .map_err(|error| error.to_string())
}

/// Marks read what the feed held at `revision` (a page's): items changed
/// since stay unread. How many changed.
#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_all_read(
    notifications: State<'_, Notifications>,
    revision: i64,
) -> Result<u32, String> {
    notifications
        .mark_all_read(revision)
        .await
        .map(|changed| u32::try_from(changed).unwrap_or(u32::MAX))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
