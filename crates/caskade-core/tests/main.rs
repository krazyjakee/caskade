//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod i18n;
mod installer_settings;
#[cfg(feature = "net")]
mod milestone_editing;
mod rom_scan;
#[cfg(feature = "net")]
mod romm;
