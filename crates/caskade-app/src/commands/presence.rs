//! Which games are running on which paired device (#240): presence from the
//! core's memory, never a wait on the network.
use caskade_core::{db::LibraryStore, devices::Devices, sync::presence::Presence};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{CommandError, report};
use crate::events::{Event, Events};

/// A game running on another device, as the webview shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlayingElsewhere {
    pub device_id: String,
    pub device_name: String,
    /// The game in this library.
    pub game_id: String,
    pub title: String,
    /// The account playing it there; empty when none was signed in.
    pub account_id: String,
    pub started_at_ms: i64,
}

/// What's running on other devices changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PlayingElsewhereChanged(pub Vec<PlayingElsewhere>);

async fn playing(
    presence: &Presence,
    devices: &Devices,
    store: &LibraryStore,
) -> caskade_core::Result<Vec<PlayingElsewhere>> {
    let names: std::collections::HashMap<String, String> = devices
        .status()
        .await
        .devices
        .into_iter()
        .map(|device| (device.id, device.name))
        .collect();
    let mut shown = Vec::new();
    for each in presence.playing(devices).await {
        let Some(game) = store.game(&each.game_id).await? else {
            continue;
        };
        shown.push(PlayingElsewhere {
            device_name: names.get(&each.device_id).cloned().unwrap_or_default(),
            device_id: each.device_id,
            game_id: each.game_id,
            title: game.title,
            account_id: each.account_id,
            started_at_ms: each.started_at_ms,
        });
    }
    Ok(shown)
}

/// The games running on other devices now, by what they last said.
pub async fn devices_playing(
    presence: &Presence,
    devices: &Devices,
    store: &LibraryStore,
) -> Result<Vec<PlayingElsewhere>, CommandError> {
    playing(presence, devices, store).await.map_err(report)
}

/// Sends what's running elsewhere to the UI whenever it changes.
pub fn setup(events: Events, presence: &Presence, devices: Devices, store: LibraryStore) {
    let mut changes = presence.changes();
    let watched = presence.clone();
    crate::runtime::spawn(async move {
        // Until the sender goes; a lagged receiver just reads the latest.
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            changes.recv().await
        {
            match playing(&watched, &devices, &store).await {
                Ok(now) => {
                    events.send(Event::PlayingElsewhereChanged(PlayingElsewhereChanged(now)))
                }
                Err(error) => tracing::warn!(%error, "presence unreadable"),
            }
        }
    });
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
