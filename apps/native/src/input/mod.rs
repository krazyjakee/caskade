//! Input and focus in the native UI (#92): the webview's rules
//! (`caskade_ui_input`) over Blitz's document. Every view works with only a
//! pad or only a keyboard:
//!
//! - Pads: `caskade-input` reads them ([`start_pads`]) and the app layer's
//!   sink sends their actions as app events while the window has focus;
//!   `guide` always gets through, and raises the window.
//! - Keys: [`InputRoot`] maps them to the same actions while navigation is
//!   on (console mode, or once a pad is used), leaving a field's typing keys
//!   alone.
//! - Actions go to the innermost binding around focus
//!   ([`use_input_action`]), else move focus spatially, press, go back or
//!   page. Dialogs keep focus inside; focus comes back to where a view was
//!   left ([`use_focus_location`]).
//! - Focus rings: Blitz never matches `:focus-visible`, so the focused
//!   element gets `data-focus-visible` when keys or a pad moved focus there.
//! - The on-screen keyboard types into a field for a pad.
//!
//! Blitz gaps this layer works around (noted for the upstream list, #82): a
//! click focuses only fields, and clears focus anywhere else, so the root
//! puts it on the clicked control after the click; app code can't send a
//! key through the page's listeners, so native controls take actions through
//! bindings; a scroll made from code sends no scroll event, so
//! [`Input::scrolled`] counts them.

mod doc;
pub mod e2e;
// A text field's edit commands for its menu and a pad (#84).
mod field;
mod guide;
mod keyboard;
mod root;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use doc::find_attr;
pub use root::{Bind, Input, InputRoot, use_focus_location, use_input, use_input_action};

use caskade_app::commands::input::{Summoner, sink};
use caskade_input::InputService;
use std::sync::{Arc, OnceLock};

/// The pads' reader, for as long as the app runs.
static PADS: OnceLock<InputService> = OnceLock::new();

/// The pads connected now (first-run setup's controller hint, #97).
pub fn connected_pads() -> Vec<caskade_input::PadInfo> {
    PADS.get()
        .map(caskade_app::commands::input::input_devices)
        .unwrap_or_default()
}

/// Starts reading pads: their actions reach [`InputRoot`] as app events
/// while the main window has focus, and `guide` raises the window.
pub fn start_pads(
    services: &caskade_app::start::Services,
    window: Arc<dyn dioxus_native::winit::window::Window>,
) {
    let summoner = Summoner::default();
    let summon = move || {
        if summoner.due(std::time::Instant::now()) {
            crate::launch::ask(crate::launch::Request::Show);
            tracing::info!("guide: Caskade raised");
        }
    };
    // A scripted end-to-end run is the only "pad", and the window counts as
    // focused (Xvfb may run without a window manager).
    if e2e::enabled() {
        let events = sink(
            crate::shell::sink(),
            Some(services.bus.clone()),
            || true,
            summon,
        );
        let service = PADS
            .get_or_init(|| InputService::without_pads(events, caskade_input::Timing::default()));
        e2e::start(service.clone());
        return;
    }
    let events = sink(
        crate::shell::sink(),
        Some(services.bus.clone()),
        move || window.has_focus(),
        summon,
    );
    let _ = PADS.get_or_init(|| InputService::start(events));
}
