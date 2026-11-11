//! A game on the user's other devices (#232, #234, #235), for the game
//! menu's Your devices section: the handle, the systems' names
//! (`components/sync/common.tsx`'s `osNames`) and what to say about a copy
//! there (`views/game/OtherDevices.tsx`'s `usePeerNote`), which the game
//! page's Other devices (#100) shares.

use caskade_ui_data::devices::{CopyBlocked, DeviceLink, DeviceOs, OtherDevices, PeerCopy};
use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

/// The app's other devices: a test's, else the running app's; `None`
/// where it moves no files between devices.
pub fn use_other_devices() -> Option<OtherDevices> {
    try_use_context::<OtherDevices>()
        .or_else(|| crate::shell::services().and_then(OtherDevices::of))
}

/// A system's name, as people know it (not translated).
pub fn os_name(os: DeviceOs) -> &'static str {
    match os {
        DeviceOs::Linux => "Linux",
        DeviceOs::Windows => "Windows",
        DeviceOs::Mac => "macOS",
    }
}

/// Why a copy there can't come here or be played, or what's special about
/// it; `None` when there's nothing to say.
pub fn peer_note(peer: &PeerCopy) -> Option<String> {
    let device = peer.device_name.clone();
    if peer.playing {
        return Some(t!("Playing on {device} now.", device = device));
    }
    match peer.blocked {
        Some(CopyBlocked::LauncherOnly) => {
            return Some(t!(
                "It starts through its store's launcher there. Install it from the store here."
            ));
        }
        Some(CopyBlocked::OtherSystem) => {
            return Some(t!(
                "{device}'s copy is for {system} and can't run here.",
                device = device,
                system = os_name(peer.os)
            ));
        }
        None => {}
    }
    if peer.link == DeviceLink::Offline {
        return Some(t!("{device} is out of reach.", device = device));
    }
    if peer.through_proton {
        return Some(t!("A Windows game: it runs here through Proton."));
    }
    None
}
