use super::{
    Limits, Registry, RegistryEntry, RegistryHive, RegistryKey, RegistryView, memory::canonical,
    wine::typed_value,
};
use std::io;
use winreg::{RegKey, enums::*};

/// Read-only native Windows access. Handles are opened with query/enumerate
/// rights and an explicit WOW64 view; there are no create/set/delete methods.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeRegistry {
    pub limits: Limits,
}
impl NativeRegistry {
    fn open(&self, key: &RegistryKey) -> io::Result<Option<RegKey>> {
        let key = canonical(key);
        let hive = match key.hive {
            RegistryHive::CurrentUser => HKEY_CURRENT_USER,
            RegistryHive::LocalMachine => HKEY_LOCAL_MACHINE,
            RegistryHive::Users => HKEY_USERS,
            RegistryHive::ClassesRoot => HKEY_CLASSES_ROOT,
            RegistryHive::CurrentConfig => HKEY_CURRENT_CONFIG,
        };
        let view = match key.view {
            RegistryView::Registry32 => KEY_WOW64_32KEY,
            _ => KEY_WOW64_64KEY,
        };
        match RegKey::predef(hive)
            .open_subkey_with_flags(&key.path, KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | view)
        {
            Ok(key) => Ok(Some(key)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}
fn limit() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "native registry enumeration limit exceeded",
    )
}
impl Registry for NativeRegistry {
    fn key_exists(&self, key: &RegistryKey) -> io::Result<bool> {
        Ok(self.open(key)?.is_some())
    }
    fn subkeys(&self, key: &RegistryKey) -> io::Result<Vec<String>> {
        let Some(key) = self.open(key)? else {
            return Ok(vec![]);
        };
        let info = key.query_info()?;
        if info.sub_keys as usize > self.limits.keys
            || info.max_sub_key_len as usize > self.limits.line_bytes / 2
        {
            return Err(limit());
        }
        let mut result = vec![];
        let mut total = 0usize;
        for name in key.enum_keys() {
            let name = name?;
            total = total.saturating_add(name.len());
            if result.len() >= self.limits.keys || total > self.limits.output_bytes {
                return Err(limit());
            }
            result.push(name);
        }
        result.sort_by_key(|name| name.to_lowercase());
        Ok(result)
    }
    fn values(&self, key: &RegistryKey) -> io::Result<Vec<RegistryEntry>> {
        let Some(key) = self.open(key)? else {
            return Ok(vec![]);
        };
        let info = key.query_info()?;
        if info.values as usize > self.limits.values
            || info.max_value_len as usize > self.limits.value_bytes
            || info.max_value_name_len as usize > self.limits.line_bytes / 2
        {
            return Err(limit());
        }
        let mut result = vec![];
        let mut total = 0usize;
        for entry in key.enum_values() {
            let (name, value) = entry?;
            total = total
                .saturating_add(name.len())
                .saturating_add(value.bytes.len());
            if result.len() >= self.limits.values
                || value.bytes.len() > self.limits.value_bytes
                || total > self.limits.output_bytes
            {
                return Err(limit());
            }
            let kind = match value.vtype {
                REG_SZ => "REG_SZ",
                REG_EXPAND_SZ => "REG_EXPAND_SZ",
                REG_MULTI_SZ => "REG_MULTI_SZ",
                REG_DWORD => "REG_DWORD",
                REG_QWORD => "REG_QWORD",
                REG_BINARY => "REG_BINARY",
                _ => continue,
            };
            let value = typed_value(kind, &value.bytes).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid typed registry data")
            })?;
            result.push(RegistryEntry { name, value });
        }
        result.sort_by_key(|value| value.name.to_lowercase());
        Ok(result)
    }
}
