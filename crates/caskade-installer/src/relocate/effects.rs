//! The manifest's registry effects follow the values a move rewrote (#38):
//! uninstall compares a created value with the data it held after the
//! install, and an Uninstall registration with its fingerprint, so both are
//! moved along with the registry, in the same transaction.
use super::{RegistryStore, RegistryWrite, replace_all};
use crate::{
    RegistryValue,
    manifest::{
        ExternalEffect, ExternalLocation,
        native::{RegistryEffect, UninstallRegistration},
    },
};
use std::io;

/// Where HKLM's 32-bit view keeps `key` as the 64-bit view addresses it:
/// `Software\X` → `Software\WOW6432Node\X`. `None` outside `Software` or
/// for a key already there.
pub fn wow64_32bit_key(key: &str) -> Option<String> {
    let key = key.replace('/', "\\");
    let key = key.trim_matches('\\');
    let (first, rest) = key.split_once('\\')?;
    let next = rest.split('\\').next().unwrap_or(rest);
    if !first.eq_ignore_ascii_case("Software")
        || next.is_empty()
        || next.eq_ignore_ascii_case("WOW6432Node")
    {
        return None;
    }
    Some(format!("{first}\\WOW6432Node\\{rest}"))
}

/// Whether a value at `hive`\\`key` (HKLM's 32-bit view at its
/// `Software\WOW6432Node` path, as the reference scan reports it) is in the
/// key a manifest effect records at `location` (`registry.<hive>.<bits>`).
fn registry_key_matches(hive: &str, key: &str, location: &ExternalLocation) -> bool {
    let lower = hive.to_ascii_lowercase().replace('_', "");
    let machine = if lower == "hklm" || lower.contains("localmachine") {
        true
    } else if lower == "hkcu" || lower.contains("currentuser") {
        false
    } else {
        return false;
    };
    let recorded = location.path.as_str().replace('/', "\\");
    let expected = match location.scope.as_str() {
        "registry.hklm.64" if machine => recorded,
        "registry.hklm.32" if machine => wow64_32bit_key(&recorded).unwrap_or(recorded),
        // HKCU's Software is shared by both views.
        "registry.hkcu.32" | "registry.hkcu.64" if !machine => recorded,
        _ => return false,
    };
    expected.eq_ignore_ascii_case(key.replace('/', "\\").trim_matches('\\'))
}

/// Text of a string value as `winreg` reads it: UTF-16LE up to its trailing
/// NULs, a multi-string's parts joined by newlines. `None` for other types.
fn text(value: &RegistryWrite) -> Option<String> {
    if !matches!(
        value.value_type.as_str(),
        "REG_SZ" | "REG_EXPAND_SZ" | "REG_MULTI_SZ"
    ) {
        return None;
    }
    let units: Vec<u16> = value
        .data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let text = text.trim_end_matches('\0');
    Some(if value.value_type == "REG_MULTI_SZ" {
        text.replace('\0', "\n")
    } else {
        text.to_owned()
    })
}

fn dword(value: &RegistryWrite) -> u32 {
    let Ok(bytes) = <[u8; 4]>::try_from(value.data.as_slice()) else {
        return 0;
    };
    match value.value_type.as_str() {
        "REG_DWORD" => u32::from_le_bytes(bytes),
        "REG_DWORD_BIG_ENDIAN" => u32::from_be_bytes(bytes),
        _ => 0,
    }
}

/// The registration at `hive`\\`key` before (`after = false`) or after the
/// rewrites, read as the native registry reads it; `None` when it couldn't
/// have been recorded (a path value that isn't text).
fn registration(
    location: &ExternalLocation,
    (hive, key): (&str, &str),
    rewrites: &[(RegistryWrite, RegistryWrite)],
    store: &dyn RegistryStore,
    after: bool,
) -> io::Result<Option<UninstallRegistration>> {
    let value = |name: &str| -> io::Result<Option<RegistryWrite>> {
        match rewrites.iter().find(|(original, _)| {
            original.hive == hive && original.key == key && original.name.eq_ignore_ascii_case(name)
        }) {
            Some((original, written)) => Ok(Some(if after { written } else { original }.clone())),
            None => store.read(hive, key, name),
        }
    };
    let mut texts = vec![];
    for name in ["InstallLocation", "UninstallString", "QuietUninstallString"] {
        match value(name)? {
            Some(found) => match text(&found) {
                Some(text) => texts.push(Some(text)),
                None => return Ok(None),
            },
            None => texts.push(None),
        }
    }
    let windows_installer = value("WindowsInstaller")?.is_some_and(|value| dword(&value) == 1);
    let [install_location, uninstall_string, quiet_uninstall_string] =
        <[Option<String>; 3]>::try_from(texts).expect("three names");
    Ok(Some(UninstallRegistration {
        location: location.clone(),
        install_location: install_location.unwrap_or_default(),
        uninstall_string,
        quiet_uninstall_string,
        windows_installer,
    }))
}

fn rewritten(value: &RegistryValue, spellings: &[(String, String)]) -> Option<RegistryValue> {
    let text = |text: &str| {
        replace_all(text.as_bytes(), spellings)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    match value {
        RegistryValue::String(value) => text(value).map(RegistryValue::String),
        RegistryValue::ExpandString(value) => text(value).map(RegistryValue::ExpandString),
        RegistryValue::MultiString(parts) => {
            let updated: Vec<Option<String>> = parts.iter().map(|part| text(part)).collect();
            updated.iter().any(Option::is_some).then(|| {
                RegistryValue::MultiString(
                    updated
                        .into_iter()
                        .zip(parts)
                        .map(|(new, old)| new.unwrap_or_else(|| old.clone()))
                        .collect(),
                )
            })
        }
        _ => None,
    }
}

/// Update `effects` for the `(original, written)` registry rewrites of a
/// move: a created value's installed data gets the new spelling, and an
/// Uninstall registration that still matched its fingerprint gets the
/// fingerprint of what it holds now. A registration that had already
/// changed keeps its old fingerprint, so uninstall still preserves it.
pub fn follow_rewrites(
    effects: &mut [ExternalEffect],
    rewrites: &[(RegistryWrite, RegistryWrite)],
    spellings: &[(String, String)],
    store: &dyn RegistryStore,
) -> io::Result<()> {
    if rewrites.is_empty() {
        return Ok(());
    }
    for effect in effects {
        let ExternalEffect::Registry { effect } = effect else {
            continue;
        };
        match effect {
            RegistryEffect::Value {
                location,
                name,
                installed: Some(installed),
                ..
            } => {
                let location: &ExternalLocation = location;
                let name: &str = name;
                if rewrites.iter().any(|(original, _)| {
                    original.name.eq_ignore_ascii_case(name)
                        && registry_key_matches(&original.hive, &original.key, location)
                }) && let Some(updated) = rewritten(installed, spellings)
                {
                    *installed = updated;
                }
            }
            RegistryEffect::UninstallKey {
                location,
                fingerprint,
            } => {
                let location: &ExternalLocation = location;
                let Some((original, _)) = rewrites.iter().find(|(original, _)| {
                    registry_key_matches(&original.hive, &original.key, location)
                }) else {
                    continue;
                };
                let key = (original.hive.as_str(), original.key.as_str());
                let before = registration(location, key, rewrites, store, false)?;
                let after = registration(location, key, rewrites, store, true)?;
                if let (Some(before), Some(after)) = (before, after)
                    && before.fingerprint() == *fingerprint
                {
                    *fingerprint = after.fingerprint();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestPath;
    use std::{collections::BTreeMap, sync::Mutex};

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn location(scope: &str, path: &str) -> ExternalLocation {
        ExternalLocation {
            scope: scope.into(),
            path: ManifestPath::new(path).unwrap(),
        }
    }

    fn sz(hive: &str, key: &str, name: &str, text: &str) -> RegistryWrite {
        RegistryWrite {
            hive: hive.into(),
            key: key.into(),
            name: name.into(),
            value_type: "REG_SZ".into(),
            data: utf16(text),
        }
    }

    /// The registry after the move, keyed by (key, name).
    #[derive(Default)]
    struct Store(Mutex<BTreeMap<(String, String), RegistryWrite>>);
    impl Store {
        fn put(&self, value: RegistryWrite) {
            self.0.lock().unwrap().insert(
                (
                    value.key.to_ascii_lowercase(),
                    value.name.to_ascii_lowercase(),
                ),
                value,
            );
        }
    }
    impl RegistryStore for Store {
        fn read(&self, _: &str, key: &str, name: &str) -> io::Result<Option<RegistryWrite>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(key.to_ascii_lowercase(), name.to_ascii_lowercase()))
                .cloned())
        }
        fn apply(&self, writes: &[RegistryWrite]) -> io::Result<()> {
            for write in writes {
                self.put(write.clone());
            }
            Ok(())
        }
    }

    #[test]
    fn the_32_bit_view_is_named_by_its_wow6432node_path() {
        assert_eq!(
            wow64_32bit_key(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\X_is1")
                .as_deref(),
            Some(r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\X_is1")
        );
        assert_eq!(
            wow64_32bit_key("SOFTWARE/Vendor/Game").as_deref(),
            Some(r"SOFTWARE\WOW6432Node\Vendor\Game")
        );
        assert_eq!(wow64_32bit_key(r"Software\Wow6432Node\Vendor"), None);
        assert_eq!(wow64_32bit_key(r"SYSTEM\CurrentControlSet"), None);
        assert_eq!(wow64_32bit_key("Software"), None);

        let uninstall = "Software/Microsoft/Windows/CurrentVersion/Uninstall/X_is1";
        let wow = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\X_is1";
        let native = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\X_is1";
        let hklm32 = location("registry.hklm.32", uninstall);
        let hklm64 = location("registry.hklm.64", uninstall);
        assert!(registry_key_matches("HKLM", wow, &hklm32));
        assert!(registry_key_matches(
            "HKEY_LOCAL_MACHINE",
            &wow.to_ascii_lowercase(),
            &hklm32
        ));
        assert!(!registry_key_matches("HKLM", native, &hklm32));
        assert!(registry_key_matches("HKLM", native, &hklm64));
        assert!(!registry_key_matches("HKLM", wow, &hklm64));
        assert!(!registry_key_matches("HKCU", native, &hklm64));
        assert!(registry_key_matches(
            "HKCU",
            native,
            &location("registry.hkcu.32", uninstall)
        ));
    }

    #[test]
    fn installed_values_and_registration_fingerprints_follow_the_move() {
        let (old, new) = (r"C:\Temp\Moving Game", r"D:\Tests\Moving Game");
        let spellings = super::super::spelling_pairs(old, new);
        let game_key = r"Software\CaskadeFixture\FixtureGame";
        let wow = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Fixture_is1";
        let registration_at = location(
            "registry.hklm.32",
            "Software/Microsoft/Windows/CurrentVersion/Uninstall/Fixture_is1",
        );
        let before = UninstallRegistration {
            location: registration_at.clone(),
            install_location: format!("{old}\\"),
            uninstall_string: Some(format!("\"{old}\\unins000.exe\"")),
            quiet_uninstall_string: None,
            windows_installer: false,
        };
        let originals = [
            sz("HKLM", game_key, "InstallPath", old),
            sz("HKLM", wow, "InstallLocation", &format!("{old}\\")),
            sz(
                "HKLM",
                wow,
                "UninstallString",
                &format!("\"{old}\\unins000.exe\""),
            ),
        ];
        let rewrites: Vec<_> = originals
            .iter()
            .map(|original| {
                let data = replace_all(&original.data, &spellings).unwrap();
                (
                    original.clone(),
                    RegistryWrite {
                        data,
                        ..original.clone()
                    },
                )
            })
            .collect();
        let store = Store::default();
        store.put(sz("HKLM", wow, "DisplayName", "Fixture"));
        store
            .apply(
                &rewrites
                    .iter()
                    .map(|(_, written)| written.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        // The same name in the 64-bit view: the move didn't touch it, so its
        // record stays as it was.
        let foreign = location(
            "registry.hklm.64",
            "Software/Microsoft/Windows/CurrentVersion/Uninstall/Fixture_is1",
        );
        let stale = UninstallRegistration {
            location: foreign.clone(),
            ..before.clone()
        }
        .fingerprint();
        let mut effects = vec![
            ExternalEffect::Registry {
                effect: RegistryEffect::Value {
                    location: location("registry.hklm.64", "Software/CaskadeFixture/FixtureGame"),
                    name: "installpath".into(),
                    original: None,
                    installed: Some(RegistryValue::String(old.into())),
                },
            },
            ExternalEffect::Registry {
                effect: RegistryEffect::Value {
                    location: location("registry.hklm.32", "Software/CaskadeFixture/FixtureGame"),
                    name: "InstallPath".into(),
                    original: None,
                    installed: Some(RegistryValue::String(old.into())),
                },
            },
            ExternalEffect::Registry {
                effect: RegistryEffect::UninstallKey {
                    location: registration_at.clone(),
                    fingerprint: before.fingerprint(),
                },
            },
            ExternalEffect::Registry {
                effect: RegistryEffect::UninstallKey {
                    location: foreign,
                    fingerprint: stale.clone(),
                },
            },
        ];
        follow_rewrites(&mut effects, &rewrites, &spellings, &store).unwrap();
        let after = UninstallRegistration {
            install_location: format!("{new}\\"),
            uninstall_string: Some(format!("\"{new}\\unins000.exe\"")),
            ..before
        };
        let ExternalEffect::Registry {
            effect: RegistryEffect::Value { installed, .. },
        } = &effects[0]
        else {
            unreachable!()
        };
        assert_eq!(installed, &Some(RegistryValue::String(new.into())));
        // The same name in the other view wasn't rewritten.
        let ExternalEffect::Registry {
            effect: RegistryEffect::Value { installed, .. },
        } = &effects[1]
        else {
            unreachable!()
        };
        assert_eq!(installed, &Some(RegistryValue::String(old.into())));
        let ExternalEffect::Registry {
            effect: RegistryEffect::UninstallKey { fingerprint, .. },
        } = &effects[2]
        else {
            unreachable!()
        };
        assert_eq!(*fingerprint, after.fingerprint());
        let ExternalEffect::Registry {
            effect: RegistryEffect::UninstallKey { fingerprint, .. },
        } = &effects[3]
        else {
            unreachable!()
        };
        assert_eq!(*fingerprint, stale);
    }

    #[test]
    fn a_registration_changed_before_the_move_keeps_its_fingerprint() {
        let spellings = super::super::spelling_pairs(r"C:\G", r"D:\G");
        let wow = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\G_is1";
        let at = location(
            "registry.hklm.32",
            "Software/Microsoft/Windows/CurrentVersion/Uninstall/G_is1",
        );
        let recorded = UninstallRegistration {
            location: at.clone(),
            install_location: r"C:\G\".into(),
            uninstall_string: None,
            quiet_uninstall_string: None,
            windows_installer: false,
        }
        .fingerprint();
        let original = sz("HKLM", wow, "InstallLocation", r"C:\G\");
        let written = RegistryWrite {
            data: replace_all(&original.data, &spellings).unwrap(),
            ..original.clone()
        };
        let store = Store::default();
        store.put(written.clone());
        // Someone added an uninstall command since the install.
        store.put(sz("HKLM", wow, "UninstallString", r"C:\Other\unins.exe"));
        let mut effects = vec![ExternalEffect::Registry {
            effect: RegistryEffect::UninstallKey {
                location: at,
                fingerprint: recorded.clone(),
            },
        }];
        follow_rewrites(&mut effects, &[(original, written)], &spellings, &store).unwrap();
        assert!(matches!(
            &effects[0],
            ExternalEffect::Registry { effect: RegistryEffect::UninstallKey { fingerprint, .. } }
                if *fingerprint == recorded
        ));
    }
}
