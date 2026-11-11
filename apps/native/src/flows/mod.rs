//! Dialogs and multi-step flows outside the main views (#108), as the
//! webview's `components/{manualGame,pathBrowser,shares,sync,storage}`:
//! each with the same steps and wording, over the native data layer.

// Until the library's toolbar mounts `AddGame` (#99) and the path
// browser picks folders (#108).
#![allow(dead_code)]

pub mod add_game;
mod anatomy;
pub mod devices;
pub mod fields;
pub mod folder_browser;
pub mod manual_game;
pub mod pairing;
pub mod paths;
pub mod pin_pad;
pub mod receive_share;
pub mod remote_play;
pub mod scripts;
pub mod share_game;
pub mod shares_common;
pub mod shares_list;
pub mod storage;
pub mod streaming;
pub mod tool_request;

use caskade_ui_theme::anatomy::Entry;
use dioxus_native::prelude::*;

/// The flows' catalogued roots.
pub const ANATOMY: &[Entry] = &[
    manual_game::ANATOMY,
    folder_browser::ANATOMY,
    anatomy::SHARE_GAME,
    anatomy::RECEIVE_SHARE,
    anatomy::SHARE_INFO,
    anatomy::SHARE_RECIPIENT,
    anatomy::PAIR_DEVICE,
    anatomy::SHARE,
    anatomy::RECEIVED_SHARE,
    anatomy::TOOL_FAILED,
    anatomy::SCRIPTS,
    anatomy::ABOUT,
    anatomy::LEGAL,
    anatomy::SYSTEM_STATS,
    anatomy::PEER_LINK,
    anatomy::PAIRED_DEVICE,
    anatomy::TRANSFER,
    anatomy::PIN_PAD,
    anatomy::ACCOUNT_LIST,
    anatomy::ACCOUNT_EDITOR,
    anatomy::ACCOUNT_PROFILE,
    anatomy::ROMM,
    anatomy::ROMM_SERVERS,
];

/// The flows that come over any view: pairing (#226), games other devices
/// ask to start here (#234) and devices asking to stream (#265), as the
/// webview's shell mounts `PairingHost`, `RemotePlayHost` and
/// `StreamPairingHost`. Each is absent where the app has no such service.
#[component]
pub fn Hosts() -> Element {
    let pairing = pairing::use_pairing();
    let plays = remote_play::use_remote_plays();
    let streaming = streaming::use_stream_pairing();
    rsx! {
        if let Some(pairing) = pairing {
            pairing::PairingHost { pairing }
        }
        if let Some(plays) = plays {
            remote_play::RemotePlayHost { plays }
        }
        if let Some(pairing) = streaming {
            streaming::StreamPairingHost { pairing }
        }
    }
}

#[cfg(test)]
mod share_tests;
#[cfg(test)]
mod tests;
