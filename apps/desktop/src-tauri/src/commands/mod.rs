//! Tauri commands: thin wrappers that call into caskade-core. One
//! submodule per area (library, launch, settings, …), owned with that area.

pub mod accounts;
pub mod achievement_icons;
pub mod achievements;
pub mod actions;
pub mod addon_packages;
pub mod addons;
pub mod app_update;
pub mod backup;
pub mod browse;
pub mod collections;
pub mod console;
pub mod discovery;
pub mod emulation;
pub mod emulator_setup;
pub mod game_edit;
pub mod guide;
pub mod i18n;
pub mod igdb;
pub mod input;
pub mod install;
mod install_picker;
pub mod install_requests;
pub mod launch;
pub mod library;
pub mod manual_games;
pub mod notifications;
pub mod plugin_ui;
pub mod plugins;
pub mod rom_scanners;
pub mod romm;
pub mod scripts;
pub mod settings;
pub mod storage;
pub mod streaming;
pub mod store_accounts;
pub mod sync;
pub mod system_stats;
#[cfg(test)]
pub(crate) mod test_ipc;
pub mod themes;
pub mod torrent_links;
pub mod updates;
pub mod video;
pub mod web;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// An error as the text a command answers with.
fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The core's answer to `hello`: proof that typed commands and events work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Greeting {
    pub message: String,
    pub core_version: String,
}

/// Greets `name` and emits the `hello` event with the same greeting.
#[tauri::command]
#[specta::specta]
pub fn hello<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
) -> Result<Greeting, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    let greeting = Greeting {
        message: format!("Hello, {name}!"),
        core_version: caskade_core::VERSION.into(),
    };
    crate::events::Hello(greeting.clone())
        .emit(&app)
        .map_err(|error| error.to_string())?;
    Ok(greeting)
}
