//! Companion remote control (`Purpose::Control`): bridges shell-level
//! actions from [`CompanionControl`] into UI events (`InputActionEvent`,
//! console-mode switches, top-level navigation) and sync triggers.

use caskade_core::companion_control::{CompanionControl, ControlAction};
use caskade_core::sync::SyncService;
use caskade_input::{Action, GlyphFamily, InputAction, Source};
use serde::{Deserialize, Serialize};

use crate::commands::input::InputActionEvent;
use crate::events::{Event, Events};

/// A paired companion asked to switch between desktop and console mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionConsoleRequested {
    pub enabled: bool,
}

/// A paired companion asked to open a top-level view (`/`, `/library`, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionViewRequested {
    pub route: String,
}

/// Parses a `caskade-input` wire action name (`nav.up`, `details`, `guide`, …).
pub(crate) fn parse_pad_action(wire: &str) -> Option<Action> {
    serde_json::from_value(serde_json::Value::String(wire.to_owned())).ok()
}

/// Hands companion shell actions to the UI and sync service.
pub fn setup(events: Events, control: &CompanionControl, sync: SyncService) {
    control.on_action(move |action| match action {
        ControlAction::SetConsoleMode { enabled } => {
            events.send(Event::CompanionConsoleRequested(
                CompanionConsoleRequested { enabled },
            ));
        }
        ControlAction::OpenView { route } => {
            events.send(Event::CompanionViewRequested(CompanionViewRequested {
                route,
            }));
        }
        ControlAction::PadAction { action } => {
            if let Some(parsed) = parse_pad_action(&action) {
                events.send(Event::InputActionEvent(InputActionEvent(InputAction {
                    action: parsed,
                    source: Source::Pad,
                    repeat: false,
                    glyphs: GlyphFamily::Generic,
                })));
            }
        }
        ControlAction::SyncNow { device_id } => {
            sync.sync_now(&device_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_allowed_wire_pad_action_parses_into_caskade_input_action() {
        for wire in caskade_sync::control::ALLOWED_PAD_ACTIONS {
            assert!(
                parse_pad_action(wire).is_some(),
                "wire action {wire} should parse into caskade_input::Action"
            );
        }
        assert!(parse_pad_action("unknown").is_none());
    }
}
