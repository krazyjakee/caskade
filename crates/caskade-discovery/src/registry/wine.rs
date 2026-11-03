use super::{
    Architecture, Limits, MemoryRegistry, Registry, RegistryEntry, RegistryHive, RegistryKey,
    RegistryValue, RegistryView,
};
use caskade_installer::runtime::registry::read;
use std::{
    fs,
    io::{self, Read},
    path::Path,
};

/// A read-only snapshot of a Wine/Proton prefix, or fixture registry files.
#[derive(Debug, Clone)]
pub struct WineRegistry {
    inner: MemoryRegistry,
    native_view: RegistryView,
    pub malformed_records: usize,
}
impl Default for WineRegistry {
    fn default() -> Self {
        Self {
            inner: MemoryRegistry::new(),
            native_view: RegistryView::Registry64,
            malformed_records: 0,
        }
    }
}
impl WineRegistry {
    /// Parse a file without I/O. `Native` follows `#arch` (defaults to win64).
    pub fn parse(
        text: &str,
        hive: RegistryHive,
        view: RegistryView,
        limits: Limits,
    ) -> io::Result<Self> {
        let mut registry = Self::default();
        registry.extend(text, hive, view, limits)?;
        Ok(registry)
    }
    /// Read only system.reg, user.reg and userdef.reg. Missing files are empty;
    /// invalid or inaccessible files remain errors, rather than host fallbacks.
    pub fn from_prefix(prefix: &Path, limits: Limits) -> io::Result<Self> {
        let mut registry = Self::default();
        for (name, hive) in [
            ("system.reg", RegistryHive::LocalMachine),
            ("user.reg", RegistryHive::CurrentUser),
            ("userdef.reg", RegistryHive::Users),
        ] {
            if let Some(text) = read_file(&prefix.join(name), limits, None)? {
                let text = if name == "userdef.reg" {
                    default_user(&text)
                } else {
                    text
                };
                registry.extend(&text, hive, RegistryView::Native, limits)?;
            }
        }
        Ok(registry)
    }
    /// Load a machine scenario's registry/ files, confined to the scenario.
    pub fn from_fixture(scenario: &Path, limits: Limits) -> io::Result<Self> {
        let scenario = dunce::canonicalize(&scenario)?;
        let mut registry = Self::default();
        for (name, hive, view) in [
            ("hkcu.reg", RegistryHive::CurrentUser, RegistryView::Native),
            (
                "hklm32.reg",
                RegistryHive::LocalMachine,
                RegistryView::Registry32,
            ),
            (
                "hklm64.reg",
                RegistryHive::LocalMachine,
                RegistryView::Registry64,
            ),
            ("hku.reg", RegistryHive::Users, RegistryView::Native),
        ] {
            if let Some(text) = read_file(
                &scenario.join("registry").join(name),
                limits,
                Some(&scenario),
            )? {
                registry.extend(&text, hive, view, limits)?;
            }
        }
        Ok(registry)
    }
    fn extend(
        &mut self,
        text: &str,
        hive: RegistryHive,
        view: RegistryView,
        limits: Limits,
    ) -> io::Result<()> {
        let parsed = read::parse(text, "", limits)?;
        let view = if view == RegistryView::Native {
            self.native_view = if parsed.architecture == Some(Architecture::Win32) {
                RegistryView::Registry32
            } else {
                RegistryView::Registry64
            };
            self.native_view
        } else {
            view
        };
        for key in parsed.keys {
            if key.len() > 32768 || key.split('\\').count() > 128 {
                return Err(invalid("registry key size or depth limit exceeded"));
            }
            self.inner
                .insert_key(RegistryKey::new(hive, view, strip_hive(&key)));
            self.check(limits)?;
        }
        for value in parsed.values {
            let Some(typed) = typed_value(&value.value_type, &value.data) else {
                self.malformed_records += 1;
                continue;
            };
            self.inner.insert(
                RegistryKey::new(hive, view, strip_hive(&value.key)),
                value.name,
                typed,
            );
            self.check(limits)?;
        }
        self.malformed_records += parsed.malformed_records;
        self.check(limits)
    }
    fn check(&self, limits: Limits) -> io::Result<()> {
        let (keys, values) = self.inner.counts();
        if keys > limits.keys || values > limits.values {
            return Err(invalid("registry snapshot count limit exceeded"));
        }
        if self.inner.bytes() > limits.output_bytes {
            return Err(invalid("registry snapshot byte limit exceeded"));
        }
        Ok(())
    }
    fn key(&self, key: &RegistryKey) -> RegistryKey {
        RegistryKey {
            view: if key.view == RegistryView::Native {
                self.native_view
            } else {
                key.view
            },
            ..key.clone()
        }
    }
}
impl Registry for WineRegistry {
    fn key_exists(&self, key: &RegistryKey) -> io::Result<bool> {
        self.inner.key_exists(&self.key(key))
    }
    fn subkeys(&self, key: &RegistryKey) -> io::Result<Vec<String>> {
        self.inner.subkeys(&self.key(key))
    }
    fn values(&self, key: &RegistryKey) -> io::Result<Vec<RegistryEntry>> {
        self.inner.values(&self.key(key))
    }
}

fn default_user(text: &str) -> String {
    text.lines()
        .map(|line| {
            if let Some(key) = line.strip_prefix('[') {
                format!("[.Default\\\\{key}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}
fn strip_hive(key: &str) -> &str {
    for prefix in [
        "HKEY_LOCAL_MACHINE\\",
        "HKEY_CURRENT_USER\\",
        "HKEY_USERS\\",
        "HKEY_CLASSES_ROOT\\",
        "HKEY_CURRENT_CONFIG\\",
    ] {
        if key
            .get(..prefix.len())
            .is_some_and(|k| k.eq_ignore_ascii_case(prefix))
        {
            return &key[prefix.len()..];
        }
    }
    key
}
pub(crate) fn typed_value(kind: &str, data: &[u8]) -> Option<RegistryValue> {
    Some(match kind {
        "REG_SZ" => RegistryValue::String(string(data)?),
        "REG_EXPAND_SZ" => RegistryValue::ExpandString(string(data)?),
        "REG_MULTI_SZ" => {
            let text = utf16(data)?;
            let text = text.trim_end_matches('\0');
            RegistryValue::MultiString(if text.is_empty() {
                vec![]
            } else {
                text.split('\0').map(str::to_owned).collect()
            })
        }
        "REG_DWORD" if data.len() == 4 => {
            RegistryValue::Dword(u32::from_le_bytes(data.try_into().ok()?))
        }
        "REG_QWORD" if data.len() == 8 => {
            RegistryValue::Qword(u64::from_le_bytes(data.try_into().ok()?))
        }
        "REG_BINARY" => RegistryValue::Binary(data.to_vec()),
        _ => return None,
    })
}
fn utf16(data: &[u8]) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }
    String::from_utf16(
        &data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>(),
    )
    .ok()
}
fn string(data: &[u8]) -> Option<String> {
    let mut text = utf16(data)?;
    while text.ends_with('\0') {
        text.pop();
    }
    Some(text)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn read_file(path: &Path, limits: Limits, root: Option<&Path>) -> io::Result<Option<String>> {
    let resolved = match dunce::canonicalize(&path) {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if root.is_some_and(|root| !resolved.starts_with(root)) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture registry escapes scenario",
        ));
    }
    // Reject directories/devices/pipes before opening: opening a named pipe
    // can block even though its reported length is zero.
    if !fs::metadata(&resolved)?.is_file() {
        return Err(invalid("registry input is not a regular file"));
    }
    let mut file = fs::File::open(&resolved)?;
    if file.metadata()?.len() > limits.input_bytes as u64 {
        return Err(invalid("registry input byte limit exceeded"));
    }
    let mut bytes = vec![];
    file.by_ref()
        .take(limits.input_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limits.input_bytes {
        return Err(invalid("registry input byte limit exceeded"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| invalid("registry text is not UTF-8"))
}
