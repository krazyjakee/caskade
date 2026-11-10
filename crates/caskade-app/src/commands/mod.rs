//! The app's commands as plain async functions (#86): every shell calls
//! these, the Tauri adapter through one-line `#[tauri::command]`
//! forwarders. Each takes the services it uses, so a shell passes what it
//! holds and a test what it builds. Who may call which is
//! [`crate::permissions`]'; the shell checks before calling.

pub mod accounts;
pub mod achievement_icons;
pub mod achievements;
pub mod actions;
pub mod addon_offers;
pub mod addon_packages;
pub mod addons;
pub mod app_update;
pub mod backup;
pub mod browse;
pub mod clipboard;
pub mod collections;
pub mod console;
pub mod copies;
pub mod discovery;
pub mod emulation;
pub mod emulator_setup;
pub mod game_edit;
pub mod guide;
pub mod hello;
pub mod i18n;
pub mod igdb;
pub mod input;
pub mod install;
pub mod install_picker;
pub mod install_requests;
pub mod launch;
pub mod library;
pub mod manual_games;
pub mod notifications;
pub mod peer_backups;
pub mod plugin_ui;
pub mod plugins;
pub mod ports;
pub mod presence;
pub mod remote_play;
pub mod rom_scanners;
pub mod romm;
pub mod saves;
pub mod scripts;
pub mod settings;
pub mod share_items;
pub mod shares;
pub mod storage;
pub mod store_accounts;
pub mod streaming;
pub mod sync;
pub mod system_stats;
pub mod themes;
pub mod torrent_links;
pub mod updates;
pub mod video;
pub mod web;
