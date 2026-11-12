//! Plugin lifecycle: discover → validate → load (manifest
//! only) → enable → activate (instantiate, lazily) → call → deactivate.
//! Nothing runs at startup; a failed call drops the instance, and the next
//! call starts a fresh one.

mod backend;
mod install;
mod ui;

use crate::{
    PluginServices,
    capabilities::Grant,
    locales::{Language, PluginLocales, PluginMessages},
    manifest::{Contributes, MANIFEST, Manifest, ManifestError, Permissions},
    read,
    runtime::{CallError, Instance, Limits, Runtime},
    write,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use wasmtime::component::Component;

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("unknown plugin {0}")]
    Unknown(String),
    #[error("plugin {id} is invalid: {}", .errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid {
        id: String,
        errors: Vec<ManifestError>,
    },
    #[error("plugin {0} is disabled")]
    Disabled(String),
    #[error("plugin {0} has no backend")]
    NoBackend(String),
    #[error("plugin {id}: {source}")]
    Call {
        id: String,
        #[source]
        source: CallError,
    },
    #[error("plugin {id} answered with an error ({code}): {message}")]
    Plugin {
        id: String,
        code: String,
        message: String,
    },
    /// It has no UI to draw: an 0.1 plugin, or one whose backend lacks the
    /// `ui` export (#109).
    #[error("plugin {0} draws no UI")]
    NoUi(String),
    /// It drew a contribution its manifest doesn't declare (#109).
    #[error("plugin {id} declares no {slot} contribution {contribution}")]
    Undeclared {
        id: String,
        slot: String,
        contribution: String,
    },
    /// What it drew or answered isn't a UI Caskade draws (#109).
    #[error("plugin {id} drew something Caskade can't show: {message}")]
    BadUi { id: String, message: String },
    #[error(transparent)]
    Core(#[from] caskade_core::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = HostError> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    /// The manifest is invalid; see `errors`.
    Invalid,
    Disabled,
    /// It asks for more than the user granted; off until granted again.
    NeedsConsent,
    /// Enabled; its backend starts on first use.
    Enabled,
    /// Its backend is running.
    Active,
}

/// A plugin as Settings → Plugins lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    /// The manifest id, or the folder name when the manifest is unreadable.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub folder: String,
    pub status: PluginStatus,
    pub errors: Vec<ManifestError>,
    pub permissions: Option<Permissions>,
    pub contributes: Option<Contributes>,
    pub has_ui: bool,
    pub has_backend: bool,
    /// Calls that crashed, timed out or hit a limit since the app started.
    pub failures: u32,
    pub last_error: Option<String>,
    /// Why its `contributes.settings` schema was refused (#128); the rest of
    /// the plugin still works.
    pub settings_error: Option<String>,
    /// Loaded in place from a dev folder (#131); never installed.
    pub dev: bool,
    /// Problems that don't stop the plugin (#219): a translation catalog in
    /// `locales/` that can't be read, whose English shows instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[specta(optional)]
    pub warnings: Vec<ManifestError>,
}

#[derive(Debug, Clone)]
pub struct HostOptions {
    /// Folders holding plugin folders; a folder with its own `plugin.json`
    /// is a plugin itself (dev mode, tests).
    pub folders: Vec<PathBuf>,
    /// Each plugin gets `<data>/<id>` as its private data folder.
    pub data: PathBuf,
    pub limits: Limits,
}

#[derive(Default)]
struct Live {
    component: Option<Component>,
    instance: Option<Instance>,
    failures: u32,
    last_error: Option<String>,
}

struct Entry {
    folder: PathBuf,
    manifest: std::result::Result<Manifest, Vec<ManifestError>>,
    /// Its own translations (#219), read with the manifest.
    locales: Arc<PluginLocales>,
    /// From a dev folder: enabled in memory with what its manifest asks for.
    dev: bool,
    enabled: bool,
    needs_consent: bool,
    live: tokio::sync::Mutex<Live>,
}

impl Entry {
    /// The manifest with its texts in `locale` (#219), for showing.
    fn translated(&self, locale: &str) -> Option<Manifest> {
        let mut manifest = self.manifest.clone().ok()?;
        self.locales.translate_manifest(locale, &mut manifest);
        Some(manifest)
    }

    /// The plugin as Settings lists it, its texts in `locale`.
    fn summary(&self, id: &str, live: Option<&Live>, locale: &str) -> PluginSummary {
        let active = live.is_some_and(|live| live.instance.is_some());
        let (failures, last_error) = live
            .map(|live| (live.failures, live.last_error.clone()))
            .unwrap_or_default();
        let folder = self.folder.to_string_lossy().into_owned();
        let warnings = self.locales.problems().to_vec();
        let translated = self.translated(locale);
        match (translated, &self.manifest) {
            (Some(manifest), _) => PluginSummary {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: Some(manifest.version.to_string()),
                description: manifest.description.clone(),
                author: manifest.author.clone(),
                folder,
                status: if active {
                    PluginStatus::Active
                } else if self.needs_consent {
                    PluginStatus::NeedsConsent
                } else if self.enabled {
                    PluginStatus::Enabled
                } else {
                    PluginStatus::Disabled
                },
                errors: Vec::new(),
                permissions: Some(manifest.permissions.clone()),
                contributes: Some(manifest.contributes.clone()),
                has_ui: manifest.ui.is_some(),
                has_backend: manifest.backend.is_some(),
                failures,
                last_error,
                settings_error: None,
                dev: self.dev,
                warnings,
            },
            (None, errors) => PluginSummary {
                id: id.to_owned(),
                name: id.to_owned(),
                version: None,
                description: None,
                author: None,
                folder,
                status: PluginStatus::Invalid,
                errors: errors.clone().err().unwrap_or_default(),
                permissions: None,
                contributes: None,
                has_ui: false,
                has_backend: false,
                failures,
                last_error,
                settings_error: None,
                dev: self.dev,
                warnings,
            },
        }
    }
}

/// A plugin folder offered for installation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPreview {
    pub plugin: PluginSummary,
    /// The version already installed under the same id, if any.
    pub installed_version: Option<String>,
}

/// Every discovered plugin and the runtime their backends share.
/// Cheap to clone; clones share the plugins and their backends.
#[derive(Clone)]
pub struct PluginHost {
    runtime: Arc<Runtime>,
    services: Arc<PluginServices>,
    options: HostOptions,
    entries: Arc<RwLock<BTreeMap<String, Arc<Entry>>>>,
    /// Plugins whose settings schema was refused, with the reason.
    settings_errors: Arc<RwLock<BTreeMap<String, String>>>,
    /// Dev plugin folders (#131), loaded in place for this session.
    dev_folders: Arc<RwLock<Vec<PathBuf>>>,
    /// Dev plugins turned off this session (e.g. by the circuit breaker).
    dev_disabled: Arc<RwLock<std::collections::BTreeSet<String>>>,
    /// The language plugins' text shows in (#219).
    language: Language,
}

/// The key of a plugin whose manifest can't be read: its folder's name.
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Plugin folders under `root`, or `root` itself when it is one.
fn plugin_folders(root: &Path) -> Vec<PathBuf> {
    if root.join(MANIFEST).is_file() {
        return vec![root.to_path_buf()];
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST).is_file())
        .collect();
    folders.sort();
    folders
}

impl PluginHost {
    pub fn new(services: PluginServices, options: HostOptions) -> Result<Self> {
        let runtime = Runtime::new(options.limits.clone()).map_err(|source| HostError::Call {
            id: String::new(),
            source,
        })?;
        Ok(Self {
            runtime: Arc::new(runtime),
            services: Arc::new(services),
            options,
            entries: Arc::default(),
            dev_folders: Arc::default(),
            dev_disabled: Arc::default(),
            settings_errors: Arc::default(),
            language: Language::default(),
        })
    }

    /// Reads every plugin folder's manifest; runs no plugin code. Running
    /// backends of plugins whose folder didn't change are kept.
    pub async fn discover(&self) -> Result<Vec<PluginSummary>> {
        let states: BTreeMap<String, (bool, Grant)> = self
            .services
            .store
            .plugin_states()
            .await?
            .into_iter()
            .map(|row| {
                let grant = Grant::from_value(row.grants).unwrap_or_default();
                (row.id, (row.enabled, grant))
            })
            .collect();
        // Dev folders first: a dev copy wins over an installed one.
        let dev: Vec<PathBuf> = read(&self.dev_folders).clone();
        let folders: Vec<PathBuf> = dev
            .iter()
            .cloned()
            .chain(
                self.options
                    .folders
                    .iter()
                    .flat_map(|root| plugin_folders(root)),
            )
            .collect();
        let mut found: BTreeMap<String, Arc<Entry>> = BTreeMap::new();
        for folder in folders {
            let manifest = Manifest::read(&folder);
            let mut key = match &manifest {
                Ok(manifest) => manifest.id.clone(),
                Err(_) => folder_name(&folder),
            };
            let manifest = match manifest {
                Ok(manifest)
                    if found
                        .get(&manifest.id)
                        .is_some_and(|other| other.manifest.is_ok()) =>
                {
                    let first = found[&manifest.id].folder.display().to_string();
                    key = format!("{}@{}", manifest.id, folder.display());
                    Err(vec![ManifestError {
                        key: "id".into(),
                        message: format!("{} is already used by {first}", manifest.id),
                    }])
                }
                other => other,
            };
            if !found.contains_key(&key) || manifest.is_ok() {
                let is_dev = dev.contains(&folder);
                // Enabled only while the grant covers what it asks for; dev
                // plugins unless turned off this session.
                let (enabled, needs_consent) = match &manifest {
                    Ok(manifest) if is_dev => {
                        (!read(&self.dev_disabled).contains(&manifest.id), false)
                    }
                    Ok(manifest) => match states.get(&manifest.id) {
                        Some((true, grant)) if grant.covers(manifest) => (true, false),
                        Some((true, _)) => (false, true),
                        _ => (false, false),
                    },
                    Err(_) => (false, false),
                };
                let locales = PluginLocales::read(&folder);
                let previous = read(&self.entries).get(&key).cloned();
                let entry = match previous {
                    Some(previous)
                        if previous.folder == folder
                            && previous.manifest == manifest
                            && *previous.locales == locales
                            && previous.dev == is_dev
                            && previous.enabled == enabled
                            && previous.needs_consent == needs_consent =>
                    {
                        previous
                    }
                    _ => Arc::new(Entry {
                        folder,
                        manifest,
                        locales: Arc::new(locales),
                        dev: is_dev,
                        enabled,
                        needs_consent,
                        live: tokio::sync::Mutex::default(),
                    }),
                };
                found.insert(key, entry);
            }
        }
        let gone: Vec<String> = read(&self.entries)
            .iter()
            .filter(|(id, entry)| entry.manifest.is_ok() && !found.contains_key(*id))
            .map(|(id, _)| id.clone())
            .collect();
        let manifests: Vec<Manifest> = found
            .values()
            .filter_map(|entry| entry.manifest.clone().ok())
            .collect();
        *write(&self.entries) = found;
        if let Some(settings) = &self.services.settings {
            // A plugin folder that went away keeps its values until uninstall.
            for id in gone {
                settings
                    .remove(&crate::settings_namespace(&id), false)
                    .await
                    .ok();
            }
            let mut errors = BTreeMap::new();
            for manifest in manifests {
                if let Err(error) = crate::settings::register(settings, &manifest).await {
                    tracing::warn!(plugin = %manifest.id, %error, "plugin settings refused");
                    errors.insert(manifest.id, error);
                }
            }
            *write(&self.settings_errors) = errors;
        }
        Ok(self.list().await)
    }

    fn with_settings(&self, mut summary: PluginSummary) -> PluginSummary {
        summary.settings_error = read(&self.settings_errors).get(&summary.id).cloned();
        summary
    }

    pub async fn list(&self) -> Vec<PluginSummary> {
        let entries: Vec<(String, Arc<Entry>)> = read(&self.entries)
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect();
        let mut summaries = Vec::with_capacity(entries.len());
        let locale = self.language.current();
        for (id, entry) in entries {
            let live = entry.live.lock().await;
            summaries.push(self.with_settings(entry.summary(&id, Some(&live), &locale)));
        }
        summaries
    }

    /// A valid plugin's manifest.
    pub fn manifest(&self, id: &str) -> Option<Manifest> {
        read(&self.entries).get(id)?.manifest.clone().ok()
    }

    /// A valid plugin's manifest with its texts in the language in use
    /// (#219): the name, the description, slot, view and command titles and
    /// the settings schema's titles and descriptions. Only for showing.
    pub fn translated_manifest(&self, id: &str) -> Option<Manifest> {
        let entry = read(&self.entries).get(id)?.clone();
        entry.translated(&self.language.current())
    }

    /// A plugin's settings schema (`plugins.<id>`) with its titles and
    /// descriptions in the language in use (#219). The app's settings
    /// listing passes each plugin section through this.
    pub fn translate_settings_schema(&self, id: &str, schema: &mut serde_json::Value) {
        let Some(entry) = read(&self.entries).get(id).cloned() else {
            return;
        };
        entry
            .locales
            .translate_schema(&self.language.current(), schema);
    }

    /// A plugin's catalog for the language in use, for its UI half's `t`
    /// and `<Trans>` (#219). English (no messages) without one.
    pub fn messages(&self, id: &str) -> Result<PluginMessages> {
        let entry = self.entry(id)?;
        Ok(entry.locales.messages(&self.language.current()))
    }

    /// The language plugins' text shows in: the core's.
    pub fn locale(&self) -> String {
        self.language.current()
    }

    /// Shows plugins' text in `locale` instead of the core's language, for
    /// this host only; `None` follows the core again. For tests, which
    /// would race each other switching the app-wide language.
    #[doc(hidden)]
    pub fn pin_locale(&self, locale: Option<&str>) {
        self.language.pin(locale);
    }

    /// A plugin's folder (for serving its UI half).
    pub fn folder(&self, id: &str) -> Option<PathBuf> {
        read(&self.entries)
            .get(id)
            .map(|entry| entry.folder.clone())
    }

    /// What the user granted the plugin; `None` before it was ever enabled.
    pub async fn grants(&self, id: &str) -> Option<Permissions> {
        let entry = self.entry(id).ok()?;
        if entry.dev {
            return entry
                .manifest
                .as_ref()
                .ok()
                .map(|manifest| manifest.permissions.clone());
        }
        let row = self.services.store.plugin_state(id).await.ok()??;
        Grant::from_value(row.grants).map(|grant| grant.permissions)
    }

    /// [`Self::grants`], or nothing when it was never enabled.
    pub(crate) async fn granted(&self, id: &str) -> Permissions {
        self.grants(id).await.unwrap_or_default()
    }

    /// Loads `folder` (a plugin folder) in place for this session (#131).
    pub async fn add_dev_folder(&self, folder: &Path) -> Result<()> {
        {
            let mut folders = write(&self.dev_folders);
            if !folders.iter().any(|known| known == folder) {
                folders.push(folder.to_path_buf());
            }
        }
        self.discover().await?;
        if let Some(id) = self.dev_plugin(folder) {
            self.changed(&id);
        }
        Ok(())
    }

    /// Stops loading `folder`; its plugin's backend stops.
    pub async fn remove_dev_folder(&self, folder: &Path) -> Result<()> {
        let id = self.dev_plugin(folder);
        write(&self.dev_folders).retain(|known| known != folder);
        if let Some(id) = &id {
            self.deactivate(id).await.ok();
            write(&self.dev_disabled).remove(id);
        }
        self.discover().await?;
        if let Some(id) = id {
            self.changed(&id);
        }
        Ok(())
    }

    /// The id of the valid dev plugin loaded from `folder`.
    pub fn dev_plugin(&self, folder: &Path) -> Option<String> {
        read(&self.entries)
            .iter()
            .find(|(_, entry)| entry.dev && entry.folder == folder && entry.manifest.is_ok())
            .map(|(id, _)| id.clone())
    }

    /// Whether the plugin's backend is running.
    pub fn is_active(&self, id: &str) -> bool {
        read(&self.entries)
            .get(id)
            .and_then(|entry| {
                entry
                    .live
                    .try_lock()
                    .ok()
                    .map(|live| live.instance.is_some())
            })
            // A call holds the lock only while the backend runs.
            .unwrap_or(true)
    }

    /// Whether the plugin is enabled.
    pub fn enabled(&self, id: &str) -> bool {
        read(&self.entries)
            .get(id)
            .is_some_and(|entry| entry.enabled)
    }

    fn entry(&self, id: &str) -> Result<Arc<Entry>> {
        read(&self.entries)
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_owned()))
    }

    fn valid(entry: &Entry, id: &str) -> Result<Manifest> {
        entry.manifest.clone().map_err(|errors| HostError::Invalid {
            id: id.to_owned(),
            errors,
        })
    }

    /// Enables (granting what the manifest asks for; #124 lets the user
    /// choose) or disables a plugin. Disabling stops its backend.
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginSummary> {
        let entry = self.entry(id)?;
        let manifest = Self::valid(&entry, id)?;
        if entry.dev {
            // Dev plugins are never persisted (#131).
            let mut disabled = write(&self.dev_disabled);
            if enabled {
                disabled.remove(id);
            } else {
                disabled.insert(id.to_owned());
            }
        } else {
            self.persist_enabled(id, &manifest, enabled).await?;
        }
        let updated = Arc::new(Entry {
            folder: entry.folder.clone(),
            manifest: entry.manifest.clone(),
            locales: entry.locales.clone(),
            dev: entry.dev,
            enabled,
            needs_consent: false,
            live: tokio::sync::Mutex::default(),
        });
        if enabled {
            // Keep a compiled component across the swap.
            let mut old = entry.live.lock().await;
            updated.live.lock().await.component = old.component.take();
        }
        write(&self.entries).insert(id.to_owned(), updated.clone());
        let summary = {
            let live = updated.live.lock().await;
            self.with_settings(updated.summary(id, Some(&live), &self.language.current()))
        };
        self.changed(id);
        Ok(summary)
    }

    async fn persist_enabled(&self, id: &str, manifest: &Manifest, enabled: bool) -> Result<()> {
        let grants = if enabled {
            Grant::of(manifest)
                .to_value()
                .map_err(caskade_core::Error::from)?
        } else {
            self.services
                .store
                .plugin_state(id)
                .await?
                .map_or(serde_json::Value::Null, |row| row.grants)
        };
        self.services
            .store
            .upsert_plugin_state(&caskade_core::db::PluginRow {
                id: id.to_owned(),
                enabled,
                grants,
            })
            .await?;
        Ok(())
    }

    /// Tells the bus (and so the hook adapter, #125) that a plugin was
    /// enabled, disabled or removed.
    fn changed(&self, id: &str) {
        if let Some(bus) = &self.services.bus {
            bus.emit(caskade_core::bus::Event::PluginsChanged {
                plugin: id.to_owned(),
            });
        }
    }

    /// The plugin's private data folder, created on first use.
    pub fn data_folder(&self, id: &str) -> Result<PathBuf> {
        let folder = self.options.data.join(id);
        std::fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    pub(crate) fn services(&self) -> &PluginServices {
        &self.services
    }

    /// Valid plugins and the providers they declare (#126).
    pub(crate) fn providers(&self) -> Vec<(String, Vec<String>)> {
        read(&self.entries)
            .iter()
            .filter_map(|(id, entry)| {
                let manifest = entry.manifest.as_ref().ok()?;
                Some((id.clone(), manifest.contributes.providers.clone()))
            })
            .collect()
    }
}
