//! Caskade's plugin host: reads `plugin.json` manifests,
//! discovers plugin folders, keeps their state, and runs backend halves as
//! WebAssembly components in wasmtime against the `caskade:plugin@0.1.1`
//! WIT world (`wit/caskade-plugin.wit`). Backends built for 0.1.0 load too:
//! wasmtime links patch versions of a 0.x package to each other.
//!
//! Capabilities (#124), hook adaptation (#125), providers (#126), the UI
//! half (#127) and plugins' own translations (#219, [`locales`]) build on
//! this crate.

pub mod capabilities;
pub mod dev;
pub mod hooks;
mod host;
mod host_api;
pub mod locales;
pub mod manifest;
mod outbound;
pub mod providers;
mod runtime;
pub mod settings;

pub use hooks::PluginHooks;
pub use host::{HostError, HostOptions, PluginHost, PluginPreview, PluginStatus, PluginSummary};
pub use locales::{PluginMessage, PluginMessages};
pub use manifest::{HOST_API, Manifest, ManifestError, Permissions};
pub use runtime::{CallError, CompileCounts, DATA_MOUNT, InterceptResult, Limits, Sandbox};

use caskade_core::{bus::Bus, db::LibraryStore, settings::SettingsService};

/// What the host functions reach on a plugin's behalf.
pub struct PluginServices {
    pub store: LibraryStore,
    pub bus: Option<Bus>,
    pub settings: Option<SettingsService>,
}

/// Locks a mutex, carrying on with the data a panicking holder left.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read-locks an `RwLock`, carrying on with the data a panicking holder left.
pub(crate) fn read<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Write-locks an `RwLock`, carrying on with the data a panicking holder left.
pub(crate) fn write<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A plugin's settings namespace (#128).
pub fn settings_namespace(plugin: &str) -> String {
    format!("plugins.{plugin}")
}
