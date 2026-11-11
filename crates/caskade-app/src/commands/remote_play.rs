//! Play a game on another device (#234): asking one, and starting what
//! another asks for here, through the app's own Play (age limits, the admin
//! PIN, console mode).

use caskade_core::remote_play::{PlayOutcome, RemotePlay};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{CommandError, report};
use crate::events::{Event, Events};

/// Another device asks to start a game here: the app plays it as Play would
/// and says how it went (`remote_play_finished`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlayRequested {
    pub request_id: String,
    pub game_id: String,
    pub device_name: String,
}

/// Starts `game_id` on `device_id`, waking it first when it's out of reach
/// and can be woken. Answers once it started there, or why not.
pub async fn game_play_on(
    play: &RemotePlay,
    game_id: &str,
    device_id: &str,
) -> Result<(), CommandError> {
    crate::permissions::enforce("game_play_on", serde_json::Value::default)?;
    play.play_on(device_id, game_id, |_| {})
        .await
        .map_err(report)
}

/// How starting a game another device asked for went here.
pub fn remote_play_finished(
    play: &RemotePlay,
    request_id: &str,
    started: bool,
    reason: Option<String>,
) {
    if crate::permissions::enforce("remote_play_finished", serde_json::Value::default).is_err() {
        return;
    }
    let outcome = if started {
        PlayOutcome::Started
    } else {
        PlayOutcome::NotStarted(
            reason.unwrap_or_else(|| caskade_core::tr!("it didn't start on that device")),
        )
    };
    play.finished(request_id, outcome);
}

/// Hands other devices' play requests to the UI; one nobody saw is
/// answered as not started.
pub fn setup(events: Events, play: &RemotePlay, devices: caskade_core::devices::Devices) {
    let answer = play.clone();
    play.on_request(move |request| {
        let events = events.clone();
        let devices = devices.clone();
        let answer = answer.clone();
        crate::runtime::spawn(async move {
            let device_name = devices
                .status()
                .await
                .devices
                .into_iter()
                .find(|device| device.id == request.device_id)
                .map(|device| device.name)
                .unwrap_or_default();
            let event = RemotePlayRequested {
                request_id: request.id.clone(),
                game_id: request.game_id,
                device_name,
            };
            if let Err(error) = events.emit(Event::RemotePlayRequested(event)) {
                tracing::warn!(%error, "a play request wasn't shown");
                answer.finished(
                    &request.id,
                    PlayOutcome::NotStarted(caskade_core::tr!("it didn't start on that device")),
                );
            }
        });
    });
}
