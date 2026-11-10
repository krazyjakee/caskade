//! Dialogs and multi-step flows outside the main views (#108), as the
//! webview's `components/{manualGame,pathBrowser,shares,sync,storage}`:
//! each with the same steps and wording, over the native data layer.

// Until the library's toolbar mounts `AddGame` (#99) and the path
// browser picks folders (#108).
#![allow(dead_code)]

pub mod add_game;
pub mod folder_browser;
pub mod manual_game;
pub mod paths;
pub mod storage;

use caskade_ui_theme::anatomy::Entry;

/// The flows' catalogued roots.
pub const ANATOMY: &[Entry] = &[manual_game::ANATOMY, folder_browser::ANATOMY];

#[cfg(test)]
mod tests;
