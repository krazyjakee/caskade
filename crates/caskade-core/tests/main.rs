//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod addon_offers;
mod appimage_env;
#[cfg(feature = "net")]
#[cfg(any(target_os = "linux", windows))]
mod clock_step;
mod cores;
mod device_defaults;
#[cfg(feature = "net")]
mod devices;
#[cfg(feature = "net")]
mod emulator_config;
mod engines;
#[cfg(feature = "net")]
mod game_copy;
mod i18n;
mod installer_settings;
#[cfg(feature = "net")]
mod lan_unplugged;
#[cfg(feature = "net")]
mod ludusavi;
#[cfg(feature = "net")]
mod milestone_editing;
mod missing_programs;
#[cfg(all(feature = "net", unix))]
mod networks;
#[cfg(feature = "net")]
mod new_device;
#[cfg(feature = "net")]
mod peer;
#[cfg(feature = "net")]
mod peer_downloads;
#[cfg(feature = "net")]
mod presence;
#[cfg(all(feature = "net", any(target_os = "linux", windows)))]
mod proof_game;
#[cfg(all(feature = "net", target_os = "linux"))]
mod proton_real;
mod rom_scan;
#[cfg(feature = "net")]
mod romm;
mod save_locations;
#[cfg(feature = "net")]
mod save_sync;
mod save_versions;
mod sync_carried;
mod sync_games;
mod sync_log;
#[cfg(feature = "net")]
mod sync_metadata;
#[cfg(feature = "net")]
mod sync_network;
mod sync_policy;
mod sync_romm;
mod sync_sections;
#[cfg(all(feature = "net", unix))]
mod two_networks;
#[cfg(all(feature = "net", windows))]
mod windows_saves;
