//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod i18n;
mod installer_settings;
#[cfg(feature = "net")]
mod ludusavi;
#[cfg(feature = "net")]
mod milestone_editing;
mod rom_scan;
mod save_locations;
#[cfg(feature = "net")]
mod romm;
#[cfg(feature = "net")]
mod devices;
mod sync_policy;
