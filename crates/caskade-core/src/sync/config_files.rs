//! Emulator configuration between the user's devices (#275): controller
//! mappings, core options and shader presets, for emulators whose
//! definitions name those files in `[config]`, while Emulator configuration
//! syncs (an optional category, off until turned on).
//!
//! **What's here.** Each exchange, this device looks at the `[config]`
//! files of every emulator it has, and lists them in the blob round's
//! `emulatorConfig` note: per file, its definition, the named folder it sits
//! under (`{config}`, from the definition's variables, as save versions do,
//! #239) and its path inside it, its SHA-256 and when it last changed. An
//! index in `<data>/emulator-config/index.json` keeps what was seen, so a
//! file is read again only when its size or time changed.
//!
//! **What arrives.** A file another device changed later than this one did,
//! for an emulator this device has, is asked for and written into this
//! device's own folder of that name: RetroArch's `config/` is
//! `~/.config/retroarch/config` on Linux, inside the Flatpak's sandbox for
//! the Flatpak, and beside `retroarch.exe` on Windows. A file changed here
//! since it was last seen is kept: it goes the other way on the next
//! exchange. Removing a file isn't carried.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::blobs::{BlobKind, BlobWriter};
use super::policy::Category;
use crate::db::LibraryStore;
use crate::emulation::{Catalog, OperatingSystem, OsFolders, VarContext, emulator_dir};
use crate::saves::versions::{Roots, hash_bytes, hash_file};
use crate::{Error, Result};

const INDEX: &str = "index.json";
/// Larger files aren't configuration.
const MAX_FILE: u64 = 4 * 1024 * 1024;

/// One file, as a device knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    definition: String,
    /// The named folder it's under, like `{config}`.
    root: String,
    /// `/`-separated, inside the folder.
    path: String,
    /// `sha256:<hex>`.
    hash: String,
    size: u64,
    /// When this content was written, on the device that wrote it.
    changed_ms: i64,
}

impl Entry {
    fn key(&self) -> String {
        format!("{}|{}|{}", self.definition, self.root, self.path)
    }
}

/// What this device saw of one of its files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Seen {
    entry: Entry,
    /// Where it is here.
    file: PathBuf,
    /// Its modification time when last read, to skip reading it again.
    modified_ms: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Note {
    #[serde(default)]
    files: Vec<Entry>,
}

/// Emulator configuration files exchanged with paired devices. Cheap to
/// clone.
#[derive(Clone)]
pub struct EmulatorConfig {
    inner: Arc<Inner>,
}

struct Inner {
    store: LibraryStore,
    catalog: Arc<Catalog>,
    os: OperatingSystem,
    folders: OsFolders,
    dir: PathBuf,
    /// This device's files, by key.
    seen: tokio::sync::Mutex<BTreeMap<String, Seen>>,
    /// Files other devices offered that this one will take, by hash.
    wanted: Mutex<HashMap<String, Vec<Entry>>>,
}

fn modified_ms(path: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((metadata.len(), modified))
}

impl EmulatorConfig {
    /// For this machine, keeping its index in `<data>/emulator-config/`.
    pub fn new(store: LibraryStore, catalog: Arc<Catalog>, data: &Path) -> Self {
        Self::with_folders(
            store,
            catalog,
            data,
            crate::emulation::host_os(),
            OsFolders::host(),
        )
    }

    /// [`new`](Self::new) for `os` with `folders` (tests).
    pub fn with_folders(
        store: LibraryStore,
        catalog: Arc<Catalog>,
        data: &Path,
        os: OperatingSystem,
        folders: OsFolders,
    ) -> Self {
        let dir = data.join("emulator-config");
        let seen: Vec<Seen> = std::fs::read(dir.join(INDEX))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            inner: Arc::new(Inner {
                store,
                catalog,
                os,
                folders,
                dir,
                seen: tokio::sync::Mutex::new(
                    seen.into_iter()
                        .map(|seen| (seen.entry.key(), seen))
                        .collect(),
                ),
                wanted: Mutex::default(),
            }),
        }
    }

    /// Each emulator here with `[config]` files: its definition, its named
    /// folders and its expanded patterns.
    async fn installs(&self) -> Result<Vec<(String, Roots, Vec<String>)>> {
        let catalog = &self.inner.catalog;
        let mut installs = Vec::new();
        for emulator in self.inner.store.emulators().await? {
            let Some(definition) = emulator
                .definition
                .as_deref()
                .and_then(|id| catalog.emulator(id))
            else {
                continue;
            };
            if definition.config.files.is_empty() {
                continue;
            }
            let kind = emulator.install.kind();
            let context = VarContext {
                emulator_dir: emulator_dir(&emulator.install, &self.inner.folders, None),
                folders: self.inner.folders.clone(),
                ..Default::default()
            };
            let Ok(patterns) = definition.resolve_config(self.inner.os, kind, &context) else {
                continue;
            };
            let mut roots = Roots::new(self.inner.os == OperatingSystem::Linux);
            for (name, path) in definition.save_roots(self.inner.os, kind, &context) {
                roots.insert(name, path);
            }
            installs.push((definition.id.clone(), roots, patterns));
        }
        Ok(installs)
    }

    /// Looks at this device's files again: new and changed ones are read.
    async fn scan(&self) -> Result<Vec<Entry>> {
        let installs = self.installs().await?;
        let case_sensitive = self.inner.os == OperatingSystem::Linux;
        let mut seen = self.inner.seen.lock().await;
        let known: BTreeMap<String, Seen> = seen.clone();
        let found = tokio::task::spawn_blocking(move || {
            let mut found = BTreeMap::new();
            for (definition, roots, patterns) in installs {
                for pattern in patterns {
                    let Ok(paths) = crate::saves::expand_glob(&pattern, case_sensitive) else {
                        continue;
                    };
                    for file in paths {
                        let Some((size, modified)) = modified_ms(&file) else {
                            continue;
                        };
                        if !file.is_file() || size > MAX_FILE {
                            continue;
                        }
                        let (root, path) = roots.portable(&file.to_string_lossy());
                        if root.is_empty() {
                            continue;
                        }
                        let key = format!("{definition}|{root}|{path}");
                        if found.contains_key(&key) {
                            continue;
                        }
                        let previous = known.get(&key);
                        let unchanged = previous.is_some_and(|previous| {
                            previous.entry.size == size && previous.modified_ms == modified
                        });
                        let entry = match previous {
                            Some(previous) if unchanged => previous.entry.clone(),
                            _ => {
                                let Ok((hash, size)) = hash_file(&file) else {
                                    continue;
                                };
                                match previous {
                                    Some(previous) if previous.entry.hash == hash => {
                                        previous.entry.clone()
                                    }
                                    _ => Entry {
                                        definition: definition.clone(),
                                        root,
                                        path,
                                        hash,
                                        size,
                                        changed_ms: modified,
                                    },
                                }
                            }
                        };
                        found.insert(
                            key,
                            Seen {
                                entry,
                                file,
                                modified_ms: modified,
                            },
                        );
                    }
                }
            }
            found
        })
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        let entries = found.values().map(|seen| seen.entry.clone()).collect();
        *seen = found;
        self.save(&seen);
        Ok(entries)
    }

    fn save(&self, seen: &BTreeMap<String, Seen>) {
        let list: Vec<&Seen> = seen.values().collect();
        let Ok(bytes) = serde_json::to_vec(&list) else {
            return;
        };
        if std::fs::create_dir_all(&self.inner.dir).is_err() {
            return;
        }
        let partial = self.inner.dir.join(".index.json.partial");
        if std::fs::write(&partial, bytes).is_ok() {
            let _ = std::fs::rename(&partial, self.inner.dir.join(INDEX));
        }
    }

    /// Writes `bytes` (checked as `hash`) for every entry waiting on it,
    /// into each install's folder of that name.
    async fn arrived(&self, hash: &str, bytes: Vec<u8>) -> Result<()> {
        let entries = crate::lock(&self.inner.wanted)
            .remove(hash)
            .unwrap_or_default();
        if entries.is_empty() {
            return Ok(());
        }
        let installs = self.installs().await?;
        let mut seen = self.inner.seen.lock().await;
        for entry in entries {
            let key = entry.key();
            let local = seen.get(&key).cloned();
            for (definition, roots, _) in &installs {
                if *definition != entry.definition {
                    continue;
                }
                let Some(target) = roots.place(&entry.root, &entry.path) else {
                    continue;
                };
                // Changed here since it was last seen: this device's copy
                // is newer, and goes the other way next time.
                if let Some((size, modified)) = modified_ms(&target) {
                    let known = local.as_ref().is_some_and(|local| {
                        local.file == target
                            && local.entry.size == size
                            && local.modified_ms == modified
                    });
                    if !known && modified > entry.changed_ms {
                        continue;
                    }
                }
                let written = {
                    let target = target.clone();
                    let bytes = bytes.clone();
                    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                        if let Some(parent) = target.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut partial = target.as_os_str().to_owned();
                        partial.push(".caskade-partial");
                        std::fs::write(&partial, &bytes)?;
                        std::fs::rename(&partial, &target)
                    })
                    .await
                    .map_err(|error| Error::Io(std::io::Error::other(error)))?
                };
                if let Err(error) = written {
                    tracing::warn!(%error, "an emulator's configuration file wasn't written");
                    continue;
                }
                let Some((_, modified)) = modified_ms(&target) else {
                    continue;
                };
                seen.insert(
                    key.clone(),
                    Seen {
                        entry: entry.clone(),
                        file: target,
                        modified_ms: modified,
                    },
                );
            }
        }
        self.save(&seen);
        Ok(())
    }
}

/// A file arriving whole in memory: configuration is small.
struct Receiving {
    config: EmulatorConfig,
    hash: String,
    bytes: Vec<u8>,
    too_big: bool,
}

#[async_trait]
impl BlobWriter for Receiving {
    async fn write(&mut self, chunk: &[u8]) -> Result<()> {
        if self.bytes.len() as u64 + chunk.len() as u64 > MAX_FILE {
            self.too_big = true;
        } else {
            self.bytes.extend_from_slice(chunk);
        }
        Ok(())
    }

    async fn finish(self: Box<Self>) -> Result<bool> {
        if self.too_big || hash_bytes(&self.bytes) != self.hash {
            return Ok(false);
        }
        self.config.arrived(&self.hash, self.bytes).await?;
        Ok(true)
    }
}

#[async_trait]
impl BlobKind for EmulatorConfig {
    fn name(&self) -> &'static str {
        "emulatorConfig"
    }

    fn category(&self) -> Category {
        Category::EmulatorConfig
    }

    async fn wanted(&self) -> Result<Vec<String>> {
        Ok(crate::lock(&self.inner.wanted).keys().cloned().collect())
    }

    async fn has(&self, hash: &str) -> bool {
        self.inner
            .seen
            .lock()
            .await
            .values()
            .any(|seen| seen.entry.hash == hash && seen.file.is_file())
    }

    async fn read(&self, hash: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let file = self
            .inner
            .seen
            .lock()
            .await
            .values()
            .find(|seen| seen.entry.hash == hash)
            .map(|seen| seen.file.clone())
            .ok_or_else(|| Error::Invalid("no such configuration file here".into()))?;
        let bytes = tokio::fs::read(&file).await?;
        let start = (offset as usize).min(bytes.len());
        let end = start.saturating_add(len).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn receive(&self, hash: &str) -> Result<Box<dyn BlobWriter>> {
        Ok(Box::new(Receiving {
            config: self.clone(),
            hash: hash.to_owned(),
            bytes: Vec::new(),
            too_big: false,
        }))
    }

    async fn announce(&self, _peer: &str) -> Option<Value> {
        let files = match self.scan().await {
            Ok(files) => files,
            Err(error) => {
                tracing::warn!(%error, "emulator configuration not looked at");
                return None;
            }
        };
        if files.is_empty() {
            return None;
        }
        serde_json::to_value(Note { files }).ok()
    }

    async fn heard(&self, _peer: &str, note: Value) {
        let Ok(note) = serde_json::from_value::<Note>(note) else {
            return;
        };
        let Ok(installs) = self.installs().await else {
            return;
        };
        let seen = self.inner.seen.lock().await;
        let mut wanted = crate::lock(&self.inner.wanted);
        for entry in note.files {
            let here = installs.iter().any(|(definition, roots, _)| {
                *definition == entry.definition && roots.place(&entry.root, &entry.path).is_some()
            });
            if !here || entry.size > MAX_FILE || !entry.hash.starts_with("sha256:") {
                continue;
            }
            if let Some(local) = seen.get(&entry.key())
                && (local.entry.hash == entry.hash || local.entry.changed_ms >= entry.changed_ms)
            {
                continue;
            }
            let waiting = wanted.entry(entry.hash.clone()).or_default();
            if !waiting.contains(&entry) {
                waiting.push(entry);
            }
        }
    }
}
