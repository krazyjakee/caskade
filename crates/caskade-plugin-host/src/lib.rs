//! Caskade's plugin host: reads `plugin.json` manifests,
//! discovers plugin folders, keeps their state, and runs backend halves as
//! WebAssembly components in wasmtime against the `caskade:plugin@0.2.0`
//! WIT world (`wit/caskade-plugin.wit`). Backends built for 0.1 load
//! through the frozen 0.1.1 world (`wit-0.1/`); wasmtime links patch
//! versions of a 0.x package to each other.
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
pub use runtime::{CallError, CompileCounts, InterceptResult, Limits};

pub(crate) use caskade_core::{lock, read, write};

use caskade_core::{bus::Bus, db::LibraryStore, settings::SettingsService};

/// What the host functions reach on a plugin's behalf.
pub struct PluginServices {
    pub store: LibraryStore,
    pub bus: Option<Bus>,
    pub settings: Option<SettingsService>,
}

/// A plugin's settings namespace (#128).
pub fn settings_namespace(plugin: &str) -> String {
    format!("plugins.{plugin}")
}
