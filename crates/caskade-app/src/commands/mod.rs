//! The app's commands as plain async functions (#86): every shell calls
//! these, the Tauri adapter through one-line `#[tauri::command]`
//! forwarders. Each takes the services it uses, so a shell passes what it
//! holds and a test what it builds. Who may call which is
//! [`crate::permissions`]'; the shell checks before calling.

pub mod accounts;
pub mod achievement_icons;
pub mod actions;
pub mod collections;
pub mod emulation;
pub mod emulator_setup;
pub mod igdb;
pub mod launch;
pub mod library;
pub mod manual_games;
pub mod ports;
pub mod rom_scanners;
pub mod scripts;
pub mod settings;
pub mod share_items;
pub mod storage;
pub mod updates;
pub mod web;
