//! Console mode input (#146): gamepads read by `caskade-input`, sent to the
//! UI as input actions while a Caskade window has focus. `guide` is always
//! sent, so it can summon Caskade over a game.

use crate::events::{Event as AppEvent, Events};
use caskade_core::bus::{Bus, Event};
use caskade_input::{Action, InputAction, InputEvent, InputService, PadInfo};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One normalized action from a pad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct InputActionEvent(pub InputAction);

/// Where the service's events go. `focused` decides whether the user is in
/// Caskade; `summon` runs on `guide` (#149) before the UI hears it; pad
/// connects reach the bus as `input.devices.changed`.
pub fn sink(
    events: Events,
    bus: Option<Bus>,
    focused: impl Fn() -> bool + Send + Sync + 'static,
    summon: impl Fn() + Send + Sync + 'static,
) -> impl Fn(InputEvent) + Send + Sync + 'static {
    move |event| match event {
        InputEvent::Action(action) => {
            if action.action == Action::Guide {
                summon();
            }
            if (action.action == Action::Guide || focused())
                && let Err(error) =
                    events.emit(AppEvent::InputActionEvent(InputActionEvent(action)))
            {
                tracing::warn!(%error, "input action not delivered");
            }
        }
        InputEvent::Devices(pads) => {
            tracing::info!(pads = pads.len(), "gamepads changed");
            if let Some(bus) = &bus {
                bus.emit(Event::InputDevicesChanged {
                    connected: u32::try_from(pads.len()).unwrap_or(u32::MAX),
                });
            }
        }
    }
}

/// Repeated guide presses within this raise the window once.
const SUMMON_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);

/// Debounces `guide`.
#[derive(Default)]
pub struct Summoner(std::sync::Mutex<Option<std::time::Instant>>);

impl Summoner {
    /// Whether a press at `now` should raise the window.
    pub fn due(&self, now: std::time::Instant) -> bool {
        let mut last = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(|at| now.saturating_duration_since(at) < SUMMON_DEBOUNCE) {
            return false;
        }
        *last = Some(now);
        true
    }
}

/// The connected gamepads; the one used last is active.
pub fn input_devices(state: &InputService) -> Vec<PadInfo> {
    state.devices()
}
