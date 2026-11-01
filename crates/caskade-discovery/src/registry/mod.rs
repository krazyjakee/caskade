//! Read-only registry access, injected into [`crate::env::HostEnv`].
//! The value representation is shared with the installer rather than copied.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::io;

mod memory;
#[cfg(windows)]
mod native;
mod wine;
pub use caskade_installer::runtime::registry::read::{Architecture, Limits};
pub use memory::MemoryRegistry;
#[cfg(windows)]
pub use native::NativeRegistry;
pub use wine::WineRegistry;

pub use caskade_installer::{RegistryHive, RegistryValue};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RegistryView {
    Native,
    Registry32,
    Registry64,
}

impl RegistryView {
    pub(crate) fn concrete(self) -> Self {
        match self {
            Self::Native if cfg!(target_pointer_width = "32") => Self::Registry32,
            Self::Native => Self::Registry64,
            view => view,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RegistryKey {
    pub hive: RegistryHive,
    pub view: RegistryView,
    pub path: String,
}
impl RegistryKey {
    pub fn new(hive: RegistryHive, view: RegistryView, path: impl Into<String>) -> Self {
        Self {
            hive,
            view,
            path: path.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RegistryEntry {
    pub name: String,
    pub value: RegistryValue,
}

/// No mutation methods: discovery cannot create, import or delete registry data.
/// Missing keys return `false`/empty collections. Errors remain distinguishable
/// from missing data, so a source can report inaccessible keys.
pub trait Registry: Send + Sync {
    fn key_exists(&self, key: &RegistryKey) -> io::Result<bool>;
    fn subkeys(&self, key: &RegistryKey) -> io::Result<Vec<String>>;
    fn values(&self, key: &RegistryKey) -> io::Result<Vec<RegistryEntry>>;
    fn open_key(
        &self,
        hive: RegistryHive,
        view: RegistryView,
        path: &str,
    ) -> io::Result<Option<RegistryKey>> {
        let key = RegistryKey::new(hive, view, path);
        Ok(self.key_exists(&key)?.then_some(key))
    }
    fn value(&self, key: &RegistryKey, name: &str) -> io::Result<Option<RegistryValue>> {
        let name = name.to_lowercase();
        Ok(self
            .values(key)?
            .into_iter()
            .find(|entry| entry.name.to_lowercase() == name)
            .map(|entry| entry.value))
    }
}

/// An explicit empty registry; fixture mode never falls back to the host.
#[derive(Debug, Default)]
pub struct EmptyRegistry;
impl Registry for EmptyRegistry {
    fn key_exists(&self, _: &RegistryKey) -> io::Result<bool> {
        Ok(false)
    }
    fn subkeys(&self, _: &RegistryKey) -> io::Result<Vec<String>> {
        Ok(vec![])
    }
    fn values(&self, _: &RegistryKey) -> io::Result<Vec<RegistryEntry>> {
        Ok(vec![])
    }
}
