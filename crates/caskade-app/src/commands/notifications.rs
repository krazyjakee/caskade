//! The notification feed (#198): pages of it, the unread count and read
//! state. The feed lives in the core (`caskade_achievements::notifications`);
//! `NotificationsChanged` tells the webview to read it again.
use caskade_achievements::notifications::{MemberPage, NotificationPage, Notifications};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};
/// The feed or its read state changed: read it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsChanged {
    pub unread: u32,
}

/// Follows the bus for the feed and sends its changes (the unread count) to
/// `events`.
pub fn setup(events: Events, notifications: &Notifications, bus: &caskade_core::bus::Bus) {
    crate::background::supervise("notifications", {
        let (notifications, bus) = (notifications.clone(), bus.clone());
        move || notifications.watch(&bus)
    });
    let mut changes = notifications.subscribe();
    let notifications = notifications.clone();
    crate::runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            // A failed count keeps the badge's last one rather than showing
            // 0 (#160); the next change tries again.
            let unread = match notifications.unread().await {
                Ok(unread) => unread,
                Err(error) => {
                    tracing::warn!(%error, "unread notifications not counted");
                    continue;
                }
            };
            events.send(Event::NotificationsChanged(NotificationsChanged { unread }));
        }
    });
}

/// A page of the feed, newest change first; only unread items with
/// `unreadOnly`.
pub async fn notifications_list(
    notifications: &Notifications,
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
pub async fn notifications_unread(notifications: &Notifications) -> Result<u32, String> {
    notifications
        .unread()
        .await
        .map_err(|error| error.to_string())
}

/// What an item is about (the games of a batch, the updates on offer),
/// each followed to the game it is now.
pub async fn notification_members(
    notifications: &Notifications,
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
pub async fn notification_mark_read(
    notifications: &Notifications,
    id: String,
    version: i64,
) -> Result<bool, String> {
    notifications
        .mark_read(&id, version)
        .await
        .map_err(|error| error.to_string())
}

pub async fn notification_mark_unread(
    notifications: &Notifications,
    id: String,
) -> Result<bool, String> {
    notifications
        .mark_unread(&id)
        .await
        .map_err(|error| error.to_string())
}

/// Marks read what the feed held at `revision` (a page's): items changed
/// since stay unread. How many changed.
pub async fn notifications_mark_all_read(
    notifications: &Notifications,
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
