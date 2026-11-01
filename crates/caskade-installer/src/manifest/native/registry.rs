use super::model::*;
use std::io;
/// Host registry adapter. Non-Windows hosts expose no host registry; callers
/// running Wine/Proton inject a provider for that prefix instead.
#[derive(Default)]
pub struct PlatformRegistry;
#[cfg(not(windows))]
impl NativeRegistry for PlatformRegistry {
    fn available(&self) -> bool {
        false
    }
    fn uninstall_entries(&self) -> io::Result<Vec<UninstallRegistration>> {
        Ok(vec![])
    }
    fn remove_uninstall_entry(&self, _: &UninstallRegistration) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "host registry unavailable",
        ))
    }
    fn undo_effect(&self, _: &RegistryEffect) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "host registry unavailable",
        ))
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use crate::RegistryValue;
    use crate::manifest::{ExternalLocation, ManifestPath};
    use winreg::{RegKey, enums::*, types::ToRegValue};
    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    const LIMIT: u32 = 64 * 1024;
    fn root(machine: bool) -> RegKey {
        RegKey::predef(if machine {
            HKEY_LOCAL_MACHINE
        } else {
            HKEY_CURRENT_USER
        })
    }
    fn view(bits64: bool) -> u32 {
        if bits64 {
            KEY_WOW64_64KEY
        } else {
            KEY_WOW64_32KEY
        }
    }
    fn absent(error: io::Error) -> io::Result<()> {
        if error.kind() == io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(error)
        }
    }
    fn open(location: &ExternalLocation, rights: u32) -> io::Result<RegKey> {
        let (machine, bits64, key) = registry_address(location)?;
        root(machine).open_subkey_with_flags(key, rights | view(bits64))
    }
    fn bounded(key: &RegKey) -> io::Result<()> {
        let info = key.query_info()?;
        if info.max_value_len > LIMIT || info.values > 1024 || info.sub_keys > 10_000 {
            return Err(io::Error::other("registry enumeration budget exceeded"));
        }
        Ok(())
    }
    fn text(key: &RegKey, name: &str) -> io::Result<Option<String>> {
        bounded(key)?;
        match key.get_value(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn registration(location: ExternalLocation) -> io::Result<UninstallRegistration> {
        let key = open(&location, KEY_READ)?;
        bounded(&key)?;
        Ok(UninstallRegistration {
            location,
            install_location: text(&key, "InstallLocation")?.unwrap_or_default(),
            uninstall_string: text(&key, "UninstallString")?,
            quiet_uninstall_string: text(&key, "QuietUninstallString")?,
            windows_installer: key.get_value::<u32, _>("WindowsInstaller").unwrap_or(0) == 1,
        })
    }
    impl NativeRegistry for PlatformRegistry {
        fn uninstall_entries(&self) -> io::Result<Vec<UninstallRegistration>> {
            let mut entries = Vec::new();
            for machine in [false, true] {
                for bits64 in [false, true] {
                    let key = match root(machine)
                        .open_subkey_with_flags(UNINSTALL, KEY_READ | view(bits64))
                    {
                        Ok(key) => key,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => return Err(error),
                    };
                    bounded(&key)?;
                    for name in key.enum_keys() {
                        if entries.len() >= 10_000 {
                            return Err(io::Error::other("Uninstall key count exceeds budget"));
                        }
                        let location = ExternalLocation {
                            scope: format!(
                                "registry.{}.{}",
                                if machine { "hklm" } else { "hkcu" },
                                if bits64 { 64 } else { 32 }
                            ),
                            path: ManifestPath::new(format!(
                                "{}/{}",
                                UNINSTALL.replace('\\', "/"),
                                name?
                            ))
                            .map_err(io::Error::other)?,
                        };
                        match registration(location) {
                            Ok(entry) => entries.push(entry),
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
            Ok(entries)
        }
        fn remove_uninstall_entry(&self, entry: &UninstallRegistration) -> io::Result<()> {
            let prefix = "software/microsoft/windows/currentversion/uninstall/";
            let lower = entry.location.path.as_str().to_ascii_lowercase();
            if !lower
                .strip_prefix(prefix)
                .is_some_and(|name| !name.is_empty() && !name.contains('/'))
            {
                return Err(io::Error::other("expected exact Uninstall subkey"));
            }
            match registration(entry.location.clone()) {
                Ok(current) if current != *entry => {
                    return Err(io::Error::other(
                        "Uninstall registration changed; preserved",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
                _ => {}
            }
            let (machine, bits64, key) = registry_address(&entry.location)?;
            let (parent, name) = key
                .rsplit_once('\\')
                .ok_or_else(|| io::Error::other("missing Uninstall parent"))?;
            let parent =
                root(machine).open_subkey_with_flags(parent, KEY_ALL_ACCESS | view(bits64))?;
            parent.delete_subkey_all(name).or_else(absent)
        }
        fn undo_effect(&self, effect: &RegistryEffect) -> io::Result<()> {
            effect.validate()?;
            match effect {
                RegistryEffect::Value {
                    location,
                    name,
                    original,
                } => {
                    let key = match open(location, KEY_READ | KEY_WRITE) {
                        Ok(key) => key,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            if original.is_none() {
                                return Ok(());
                            }
                            let (machine, bits64, path) = registry_address(location)?;
                            root(machine)
                                .create_subkey_with_flags(
                                    path,
                                    KEY_READ | KEY_WRITE | view(bits64),
                                )?
                                .0
                        }
                        Err(error) => return Err(error),
                    };
                    if let Some(value) = original {
                        let raw = match value {
                            RegistryValue::String(value) => value.to_reg_value(),
                            RegistryValue::ExpandString(value) => {
                                let mut raw = value.to_reg_value();
                                raw.vtype = REG_EXPAND_SZ;
                                raw
                            }
                            RegistryValue::MultiString(value) => value.to_reg_value(),
                            RegistryValue::Dword(value) => value.to_reg_value(),
                            RegistryValue::Qword(value) => value.to_reg_value(),
                            RegistryValue::Binary(value) => winreg::RegValue {
                                bytes: value.clone().into(),
                                vtype: REG_BINARY,
                            },
                        };
                        key.set_raw_value(name, &raw)
                    } else {
                        key.delete_value(name).or_else(absent)
                    }
                }
                RegistryEffect::UninstallKey {
                    location,
                    fingerprint,
                } => match registration(location.clone()) {
                    Ok(current) if registration_fingerprint(&current) == *fingerprint => {
                        self.remove_uninstall_entry(&current)
                    }
                    Ok(_) => Err(io::Error::other(
                        "recorded Uninstall registration changed; preserved",
                    )),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error),
                },
                RegistryEffect::Key { location } => {
                    let key = match open(location, KEY_READ) {
                        Ok(key) => key,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                        Err(error) => return Err(error),
                    };
                    let info = key.query_info()?;
                    if info.values != 0 || info.sub_keys != 0 {
                        return Err(io::Error::other(
                            "registry key contains unrecorded data; preserved",
                        ));
                    }
                    drop(key);
                    let (machine, bits64, path) = registry_address(location)?;
                    let (parent, name) = path
                        .rsplit_once('\\')
                        .ok_or_else(|| io::Error::other("missing registry parent"))?;
                    root(machine)
                        .open_subkey_with_flags(parent, KEY_WRITE | view(bits64))?
                        .delete_subkey(name)
                        .or_else(absent)
                }
            }
        }
    }
}
