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
pub mod browse;
pub mod collections;
pub mod copies;
pub mod discovery;
pub mod emulation;
pub mod emulator_setup;
pub mod game_edit;
pub mod igdb;
pub mod launch;
pub mod library;
pub mod manual_games;
pub mod peer_backups;
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
pub mod sync;
pub mod torrent_links;
pub mod updates;
pub mod web;
