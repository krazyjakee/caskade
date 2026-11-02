//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod appimage_env;
mod device_defaults;
mod i18n;
mod installer_settings;
#[cfg(feature = "net")]
mod ludusavi;
#[cfg(feature = "net")]
mod milestone_editing;
mod rom_scan;
mod save_locations;
#[cfg(feature = "net")]
mod presence;
#[cfg(feature = "net")]
mod save_sync;
mod save_versions;
#[cfg(feature = "net")]
mod romm;
#[cfg(feature = "net")]
mod devices;
mod sync_carried;
mod sync_games;
mod sync_log;
mod sync_sections;
mod sync_romm;
#[cfg(feature = "net")]
mod sync_network;
mod sync_policy;
