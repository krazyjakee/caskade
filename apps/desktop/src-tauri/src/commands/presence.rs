//! Which games are running on which paired device (#240): presence from the
//! core's memory, never a wait on the network.
use crate::error::{CommandError, report};
use caskade_core::{db::LibraryStore, devices::Devices, sync::presence::Presence};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_specta::Event;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
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
#[tauri::command]
#[specta::specta]
pub async fn devices_playing(
    presence: State<'_, Presence>,
    devices: State<'_, Devices>,
    store: State<'_, LibraryStore>,
) -> Result<Vec<PlayingElsewhere>, CommandError> {
    playing(&presence, &devices, &store).await.map_err(report)
}

/// Sends what's running elsewhere to the webview whenever it changes, and
/// manages `presence`.
pub fn setup<R: Runtime>(app: &AppHandle<R>, presence: Presence, devices: Devices) {
    let mut changes = presence.changes();
    let handle = app.clone();
    let watched = presence.clone();
    tauri::async_runtime::spawn(async move {
        // Until the sender goes; a lagged receiver just reads the latest.
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            changes.recv().await
        {
            let Some(store) = handle.try_state::<LibraryStore>() else {
                continue;
            };
            match playing(&watched, &devices, &store).await {
                Ok(now) => {
                    if let Err(error) = PlayingElsewhereChanged(now).emit(&handle) {
                        tracing::warn!(%error, "presence change not delivered");
                    }
                }
                Err(error) => tracing::warn!(%error, "presence unreadable"),
            }
        }
    });
    app.manage(presence);
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
