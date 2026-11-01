use super::{Registry, RegistryEntry, RegistryHive, RegistryKey, RegistryValue, RegistryView};
use std::{collections::BTreeMap, io};

#[derive(Debug, Clone, Default)]
struct Key {
    spelling: String,
    values: BTreeMap<String, RegistryEntry>,
}

/// Owned registry snapshot, useful both for fixtures and injected fake values.
/// Keys and value names compare case-insensitively. The read-only `Registry`
/// capability never exposes these construction methods to discovery sources.
#[derive(Debug, Clone, Default)]
pub struct MemoryRegistry {
    keys: BTreeMap<(u8, RegistryView, String), Key>,
    bytes: usize,
    value_count: usize,
}
impl MemoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert_key(&mut self, key: RegistryKey) {
        let key = canonical(&key);
        let (hive, view, _) = identity(&key);
        let spelling = normalized_path(&key.path);
        let parts: Vec<_> = spelling.split('\\').collect();
        for count in 0..=parts.len() {
            let spelling = parts[..count].join("\\");
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.keys.entry((hive, view, spelling.to_lowercase()))
            {
                self.bytes = self.bytes.saturating_add(spelling.len().saturating_mul(2));
                entry.insert(Key {
                    spelling,
                    ..Key::default()
                });
            }
        }
    }
    pub fn insert(&mut self, key: RegistryKey, name: impl Into<String>, value: RegistryValue) {
        let id = identity(&key);
        if !self.keys.contains_key(&id) {
            self.insert_key(key);
        }
        let name = name.into();
        let bytes = match &value {
            RegistryValue::String(s) | RegistryValue::ExpandString(s) => s.len(),
            RegistryValue::MultiString(s) => s.iter().map(String::len).sum(),
            RegistryValue::Dword(_) => 4,
            RegistryValue::Qword(_) => 8,
            RegistryValue::Binary(b) => b.len(),
        };
        // Conservative cumulative accounting also bounds repeated overwrites.
        self.bytes = self
            .bytes
            .saturating_add(name.len().saturating_mul(2))
            .saturating_add(bytes);
        let previous = self
            .keys
            .get_mut(&id)
            .expect("key inserted")
            .values
            .insert(name.to_lowercase(), RegistryEntry { name, value });
        if previous.is_none() {
            self.value_count += 1;
        }
    }
    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.keys.len(), self.value_count)
    }
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}
impl Registry for MemoryRegistry {
    fn key_exists(&self, key: &RegistryKey) -> io::Result<bool> {
        Ok(self.keys.contains_key(&identity(key)))
    }
    fn subkeys(&self, key: &RegistryKey) -> io::Result<Vec<String>> {
        let (hive, view, path) = identity(key);
        let prefix = if path.is_empty() { path } else { path + "\\" };
        let mut result = BTreeMap::new();
        for ((h, v, p), value) in &self.keys {
            if *h != hive || *v != view {
                continue;
            }
            let Some(tail) = p.strip_prefix(&prefix) else {
                continue;
            };
            if tail.is_empty() || tail.contains('\\') {
                continue;
            }
            result.insert(
                tail,
                value
                    .spelling
                    .rsplit('\\')
                    .next()
                    .unwrap_or(tail)
                    .to_owned(),
            );
        }
        Ok(result.into_values().collect())
    }
    fn values(&self, key: &RegistryKey) -> io::Result<Vec<RegistryEntry>> {
        Ok(self
            .keys
            .get(&identity(key))
            .map(|key| key.values.values().cloned().collect())
            .unwrap_or_default())
    }
}

pub(crate) fn normalized_path(path: &str) -> String {
    path.split('\\')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\\")
}
pub(crate) fn canonical(key: &RegistryKey) -> RegistryKey {
    let mut parts: Vec<_> = key
        .path
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect();
    let mut view = key.view.concrete();
    if parts
        .first()
        .is_some_and(|p| p.eq_ignore_ascii_case("Software"))
        && parts
            .get(1)
            .is_some_and(|p| p.eq_ignore_ascii_case("Wow6432Node"))
    {
        parts.remove(1);
        view = RegistryView::Registry32;
    }
    RegistryKey::new(key.hive, view, parts.join("\\"))
}
fn identity(key: &RegistryKey) -> (u8, RegistryView, String) {
    let key = canonical(key);
    let hive = match key.hive {
        RegistryHive::CurrentUser => 0,
        RegistryHive::LocalMachine => 1,
        RegistryHive::Users => 2,
        RegistryHive::ClassesRoot => 3,
        RegistryHive::CurrentConfig => 4,
    };
    (hive, key.view, key.path.to_lowercase())
}
