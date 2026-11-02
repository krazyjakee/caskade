use super::{Result, SectionDefinition, SettingsError, SettingsSection, registry::bounded};
use crate::db::LibraryStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use tokio::sync::{Mutex, broadcast};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub namespace: String,
    pub version: u32,
    pub revision: u32,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    pub namespace: String,
    pub version: u32,
    pub revision: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Envelope {
    pub schema_version: u32,
    pub revision: u32,
    pub value: Value,
}
struct Section {
    definition: SectionDefinition,
    lock: Mutex<()>,
}

/// Persistent settings with per-section serialization and DB compare-and-set.
/// A publisher connects committed changes to the app's shared hook bus.
#[derive(Clone)]
pub struct SettingsService {
    store: LibraryStore,
    sections: Arc<RwLock<BTreeMap<String, Arc<Section>>>>,
    changes: broadcast::Sender<SettingsChanged>,
    publisher: Arc<dyn Fn(SettingsChanged) + Send + Sync>,
}
impl SettingsService {
    pub fn with_bus(store: LibraryStore, bus: crate::bus::Bus) -> Self {
        Self::with_publisher(store, move |changed| {
            bus.emit(crate::bus::Event::SettingsChanged {
                namespace: changed.namespace,
                version: changed.version,
            });
        })
    }

    pub fn new(store: LibraryStore) -> Self {
        Self::with_publisher(store, |_| {})
    }
    pub fn with_publisher(
        store: LibraryStore,
        publisher: impl Fn(SettingsChanged) + Send + Sync + 'static,
    ) -> Self {
        Self {
            store,
            sections: Arc::new(RwLock::new(BTreeMap::new())),
            changes: broadcast::channel(64).0,
            publisher: Arc::new(publisher),
        }
    }
    pub fn register(&self, definition: SectionDefinition) -> Result<()> {
        let mut sections = crate::write(&self.sections);
        let namespace = definition.descriptor.namespace.clone();
        if sections.contains_key(&namespace) {
            return Err(SettingsError::Registration("duplicate settings namespace"));
        }
        sections.insert(
            namespace,
            Arc::new(Section {
                definition,
                lock: Mutex::new(()),
            }),
        );
        Ok(())
    }
    /// Registers a section or replaces its definition, e.g. when a plugin
    /// update ships a new schema. Stored values are kept.
    pub fn replace(&self, definition: SectionDefinition) -> Result<()> {
        let mut sections = crate::write(&self.sections);
        sections.insert(
            definition.descriptor.namespace.clone(),
            Arc::new(Section {
                definition,
                lock: Mutex::new(()),
            }),
        );
        Ok(())
    }

    /// Unregisters a section; with `forget`, its stored value is deleted too
    /// (a plugin's uninstall).
    pub async fn remove(&self, namespace: &str, forget: bool) -> Result<bool> {
        let removed = crate::write(&self.sections).remove(namespace).is_some();
        let deleted = forget && self.store.delete_setting(&key(namespace)).await?;
        Ok(removed || deleted)
    }

    pub fn sections(&self) -> Result<Vec<SettingsSection>> {
        // A panicked writer leaves the registry as it was: carry on with it
        // instead of losing every settings panel until a restart (#160).
        // Core sections' titles and descriptions in the language in use
        // (#214); a plugin's as it wrote them.
        Ok(crate::read(&self.sections)
            .values()
            .map(|s| {
                let mut descriptor = s.definition.descriptor.clone();
                if !descriptor.namespace.starts_with("plugins.") {
                    super::schema_text::translate(&mut descriptor.schema);
                }
                descriptor
            })
            .collect())
    }
    pub fn subscribe(&self) -> broadcast::Receiver<SettingsChanged> {
        self.changes.subscribe()
    }

    pub async fn get(&self, namespace: &str) -> Result<SettingsSnapshot> {
        let section = self.section(namespace)?;
        let _guard = section.lock.lock().await;
        Ok(self.read(&section).await?.1)
    }

    /// `namespace`'s defaults, without its schema's translated text.
    pub fn defaults(&self, namespace: &str) -> Option<Value> {
        let section = self.section(namespace).ok()?;
        Some(section.definition.descriptor.defaults.clone())
    }

    /// `namespace`'s value as `T`; the default when it can't be read.
    pub async fn read_or_default<T: serde::de::DeserializeOwned + Default>(
        &self,
        namespace: &str,
    ) -> T {
        match self.get(namespace).await {
            Ok(section) => serde_json::from_value(section.value).unwrap_or_default(),
            Err(error) => {
                tracing::warn!(%error, namespace, "settings unavailable");
                T::default()
            }
        }
    }

    /// The boolean `key` of `namespace`'s value; false when it is unset or
    /// can't be read.
    pub async fn flag(&self, namespace: &str, key: &str) -> bool {
        self.get(namespace)
            .await
            .ok()
            .and_then(|section| section.value.get(key)?.as_bool())
            .unwrap_or(false)
    }
    pub async fn set(
        &self,
        namespace: &str,
        value: Value,
        expected_revision: Option<u32>,
    ) -> Result<SettingsSnapshot> {
        let section = self.section(namespace)?;
        section.definition.validate(&value)?;
        let _guard = section.lock.lock().await;
        for _ in 0..8 {
            let (raw, current) = self.read(&section).await?;
            if expected_revision.is_some_and(|expected| expected != current.revision) {
                return Err(SettingsError::Conflict);
            }
            if raw.is_some() && value == current.value {
                return Ok(current);
            }
            let next = SettingsSnapshot {
                revision: current
                    .revision
                    .checked_add(1)
                    .ok_or(SettingsError::Limit)?,
                value: value.clone(),
                ..current
            };
            let encoded = serde_json::to_value(Envelope {
                schema_version: next.version,
                revision: next.revision,
                value: value.clone(),
            })?;
            if self
                .store
                .compare_and_set_setting(&key(namespace), raw.as_ref(), &encoded)
                .await?
            {
                self.changed(&next);
                return Ok(next);
            }
        }
        Err(SettingsError::Conflict)
    }
    pub async fn reset(
        &self,
        namespace: &str,
        expected_revision: Option<u32>,
    ) -> Result<SettingsSnapshot> {
        let defaults = self
            .section(namespace)?
            .definition
            .descriptor
            .defaults
            .clone();
        self.set(namespace, defaults, expected_revision).await
    }
    fn section(&self, namespace: &str) -> Result<Arc<Section>> {
        crate::read(&self.sections)
            .get(namespace)
            .cloned()
            .ok_or_else(|| SettingsError::Unknown(namespace.into()))
    }
    fn changed(&self, snapshot: &SettingsSnapshot) {
        let event = SettingsChanged {
            namespace: snapshot.namespace.clone(),
            version: snapshot.version,
            revision: snapshot.revision,
        };
        let _ = self.changes.send(event.clone());
        (self.publisher)(event);
    }
    async fn read(&self, section: &Section) -> Result<(Option<Value>, SettingsSnapshot)> {
        let descriptor = &section.definition.descriptor;
        for _ in 0..8 {
            let raw = self.store.get_setting(&key(&descriptor.namespace)).await?;
            let Some(document) = &raw else {
                return Ok((
                    None,
                    SettingsSnapshot {
                        namespace: descriptor.namespace.clone(),
                        version: descriptor.version,
                        revision: 0,
                        value: descriptor.defaults.clone(),
                    },
                ));
            };
            // The user value has a 64 KiB budget; its version/revision envelope
            // needs a small additional allowance to round-trip boundary values.
            if serde_json::to_vec(document)?.len() > 65 * 1024 {
                return Err(SettingsError::Limit);
            }
            let mut stored: Envelope =
                serde_json::from_value(document.clone()).map_err(|_| SettingsError::Stored)?;
            if stored.schema_version == 0 {
                return Err(SettingsError::Stored);
            }
            let revalidating =
                section.definition.revalidate && stored.schema_version != descriptor.version;
            if revalidating {
                stored.value = section.definition.salvage(stored.value);
                stored.schema_version = descriptor.version;
            }
            if stored.schema_version > descriptor.version {
                return Err(SettingsError::Version);
            }
            let migrating = revalidating || stored.schema_version < descriptor.version;
            while stored.schema_version < descriptor.version {
                let migrate = section
                    .definition
                    .migrations
                    .get(&stored.schema_version)
                    .ok_or(SettingsError::Migration)?;
                stored.value = migrate(stored.value)?;
                bounded(&stored.value)?;
                stored.schema_version += 1;
            }
            section.definition.validate(&stored.value)?;
            if migrating {
                stored.revision = stored.revision.checked_add(1).ok_or(SettingsError::Limit)?;
            }
            let snapshot = SettingsSnapshot {
                namespace: descriptor.namespace.clone(),
                version: stored.schema_version,
                revision: stored.revision,
                value: stored.value.clone(),
            };
            if !migrating {
                return Ok((raw, snapshot));
            }
            let encoded = serde_json::to_value(stored)?;
            if self
                .store
                .compare_and_set_setting(&key(&descriptor.namespace), raw.as_ref(), &encoded)
                .await?
            {
                self.changed(&snapshot);
                return Ok((Some(encoded), snapshot));
            }
        }
        Err(SettingsError::Conflict)
    }
}

pub(crate) fn key(namespace: &str) -> String {
    format!("settings.section.{namespace}")
}
