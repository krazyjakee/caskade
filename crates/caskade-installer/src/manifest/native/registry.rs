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
pub(crate) use windows::{Owner, VIEWS, created_effects, software_keys};
#[cfg(windows)]
mod windows {
    use super::*;
    use crate::RegistryValue;
    use crate::manifest::{ExternalLocation, ManifestPath};
    use std::collections::BTreeSet;
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
    /// Typed data of a raw value; types without a typed form keep their bytes.
    fn typed(raw: winreg::RegValue) -> RegistryValue {
        use winreg::types::FromRegValue;
        let decoded = match raw.vtype {
            REG_SZ => String::from_reg_value(&raw).ok().map(RegistryValue::String),
            REG_EXPAND_SZ => String::from_reg_value(&raw)
                .ok()
                .map(RegistryValue::ExpandString),
            REG_MULTI_SZ => Vec::<String>::from_reg_value(&raw)
                .ok()
                .map(RegistryValue::MultiString),
            REG_DWORD => u32::from_reg_value(&raw).ok().map(RegistryValue::Dword),
            REG_QWORD => u64::from_reg_value(&raw).ok().map(RegistryValue::Qword),
            _ => None,
        };
        decoded.unwrap_or_else(|| RegistryValue::Binary(raw.bytes.to_vec()))
    }
    fn read(key: &RegKey, name: &str) -> io::Result<Option<RegistryValue>> {
        match key.get_raw_value(name) {
            Ok(raw) => Ok(Some(typed(raw))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Views an install can write to below Software: HKLM's two, and HKCU
    /// (its Software is shared by both views).
    pub(crate) const VIEWS: [(bool, bool); 3] = [(true, true), (true, false), (false, true)];
    /// System and shared trees whose new keys during an install are not the
    /// installer's to own; Uninstall registrations are recorded separately.
    const SHARED: [&str; 4] = ["Microsoft", "Classes", "Policies", "WOW6432Node"];
    const MAX_KEYS: usize = 200_000;
    /// Keys one and two levels below Software in a view, as `Vendor` and
    /// `Vendor\\Product`.
    pub(crate) fn software_keys(machine: bool, bits64: bool) -> io::Result<BTreeSet<String>> {
        let mut keys = BTreeSet::new();
        let software =
            match root(machine).open_subkey_with_flags("Software", KEY_READ | view(bits64)) {
                Ok(key) => key,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(keys),
                Err(error) => return Err(error),
            };
        for name in software.enum_keys() {
            let name = name?;
            if SHARED
                .iter()
                .any(|shared| shared.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            if let Ok(child) = software.open_subkey_with_flags(&name, KEY_READ | view(bits64)) {
                for sub in child.enum_keys() {
                    keys.insert(format!("{name}\\{}", sub?));
                    if keys.len() > MAX_KEYS {
                        return Err(io::Error::other("registry snapshot budget exceeded"));
                    }
                }
            }
            keys.insert(name);
        }
        Ok(keys)
    }
    /// What ties a created key to the game being installed: its folder (as
    /// the installer was given it and as it really is) and the names of its
    /// own Uninstall registrations (Inno's AppId, an MSI's ProductCode).
    pub(crate) struct Owner {
        paths: Vec<String>,
        names: Vec<String>,
    }
    impl Owner {
        pub(crate) fn new(game: &std::path::Path, args: &[String]) -> Self {
            let mut paths = Vec::new();
            let mut add = |path: &str| {
                let path = normalized(path.trim_matches('"'));
                if path.len() > 3 && !paths.contains(&path) {
                    paths.push(path);
                }
            };
            add(&game.to_string_lossy());
            if let Ok(real) = dunce::canonicalize(game) {
                add(&real.to_string_lossy());
            }
            // /DIR=, /D=, INSTALLDIR=...: the path the installer wrote with,
            // an alias's included.
            for arg in args {
                if let Some((_, value)) = arg.split_once('=')
                    && (value.contains(":\\") || value.contains(":/") || value.starts_with("\\\\"))
                {
                    add(value);
                }
            }
            let names = PlatformRegistry
                .uninstall_entries()
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| super::super::detection::owned(game, entry))
                .filter_map(|entry| {
                    let name = entry
                        .location
                        .path
                        .as_str()
                        .rsplit('/')
                        .next()?
                        .to_ascii_lowercase();
                    Some(name.trim_end_matches("_is1").to_owned())
                })
                .filter(|name| !name.is_empty())
                .collect();
            Self { paths, names }
        }
        /// Whether `text` names the game's folder or something inside it.
        pub(crate) fn mentions(&self, text: &str) -> bool {
            let text = normalized(text);
            self.paths.iter().any(|path| {
                text.match_indices(path.as_str()).any(|(at, _)| {
                    matches!(
                        text[at + path.len()..].chars().next(),
                        None | Some('\\' | '"' | ';' | ' ')
                    )
                })
            })
        }
        fn owns(&self, key: &str, values: &[(String, String, RegistryValue)]) -> bool {
            key.split('\\').any(|part| {
                self.names
                    .iter()
                    .any(|name| part.eq_ignore_ascii_case(name))
            }) || values.iter().any(|(_, _, data)| match data {
                RegistryValue::String(text) | RegistryValue::ExpandString(text) => {
                    self.mentions(text)
                }
                RegistryValue::MultiString(texts) => texts.iter().any(|text| self.mentions(text)),
                _ => false,
            })
        }
    }
    fn normalized(path: &str) -> String {
        path.trim_start_matches("\\\\?\\")
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }
    /// Effects that undo the keys an install created below Software in one
    /// view: every created key (parents first), then each value with the data
    /// it holds now, so removal keeps what the user changed since. A key is
    /// the game's only when its name or one of its values says so (see
    /// `Owner`): anything else that appeared meanwhile belongs to whatever
    /// else ran, and is never recorded.
    pub(crate) fn created_effects(
        machine: bool,
        bits64: bool,
        created: &BTreeSet<String>,
        owner: &Owner,
    ) -> io::Result<Vec<RegistryEffect>> {
        let scope = format!(
            "registry.{}.{}",
            if machine { "hklm" } else { "hkcu" },
            if bits64 { 64 } else { 32 }
        );
        let mut keys = Vec::new();
        let mut values = Vec::new();
        // A new key's parent may be new too; walking the topmost covers both.
        for key in created.iter().filter(|key| {
            !key.rsplit_once('\\')
                .is_some_and(|(parent, _)| created.contains(parent))
        }) {
            let path = format!("Software\\{key}");
            let (mut subtree_keys, mut subtree_values) = (Vec::new(), Vec::new());
            walk(
                machine,
                bits64,
                &path,
                0,
                &mut subtree_keys,
                &mut subtree_values,
            )?;
            if owner.owns(key, &subtree_values) {
                keys.extend(subtree_keys);
                values.extend(subtree_values);
            } else {
                tracing::info!(key = %path, "registry key created during the install isn't the game's; not recorded");
            }
        }
        let location = |path: &str| -> io::Result<ExternalLocation> {
            Ok(ExternalLocation {
                scope: scope.clone(),
                path: ManifestPath::new(path.replace('\\', "/")).map_err(io::Error::other)?,
            })
        };
        let mut effects = Vec::new();
        for key in keys {
            effects.push(RegistryEffect::Key {
                location: location(&key)?,
            });
        }
        for (key, name, data) in values {
            effects.push(RegistryEffect::Value {
                location: location(&key)?,
                name,
                original: None,
                installed: Some(data),
            });
        }
        Ok(effects)
    }
    fn walk(
        machine: bool,
        bits64: bool,
        path: &str,
        depth: usize,
        keys: &mut Vec<String>,
        values: &mut Vec<(String, String, RegistryValue)>,
    ) -> io::Result<()> {
        // A '/' in a key name would read as a separator in its manifest
        // path: such a subtree is left alone rather than misrecorded.
        if depth > 16 || path.contains('/') {
            return Ok(());
        }
        if keys.len() >= 1_000 || values.len() >= 10_000 {
            return Err(io::Error::other("created registry keys exceed the budget"));
        }
        let key = match root(machine).open_subkey_with_flags(path, KEY_READ | view(bits64)) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        bounded(&key)?;
        keys.push(path.to_owned());
        for value in key.enum_values() {
            let (name, raw) = value?;
            values.push((path.to_owned(), name, typed(raw)));
        }
        let children: Vec<String> = key.enum_keys().collect::<io::Result<_>>()?;
        for child in children {
            walk(
                machine,
                bits64,
                &format!("{path}\\{child}"),
                depth + 1,
                keys,
                values,
            )?;
        }
        Ok(())
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
        /// A per-machine product's record lives in HKLM's Classes, a per-user
        /// one's in HKCU. Neither tree is redirected between the views.
        fn windows_installer_product(&self, product_code: &str) -> io::Result<bool> {
            let Some(packed) = packed_product_code(product_code) else {
                return Ok(false);
            };
            for (machine, products) in [
                (true, "Software\\Classes\\Installer\\Products"),
                (false, "Software\\Microsoft\\Installer\\Products"),
            ] {
                match root(machine)
                    .open_subkey_with_flags(format!("{products}\\{packed}"), KEY_READ)
                {
                    Ok(_) => return Ok(true),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(false)
        }
        fn undo_effect(&self, effect: &RegistryEffect) -> io::Result<()> {
            effect.validate()?;
            match effect {
                RegistryEffect::Value {
                    location,
                    name,
                    original,
                    installed,
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
                    // Data the user changed since the install is theirs now.
                    if let Some(installed) = installed {
                        match read(&key, name)? {
                            None if original.is_none() => return Ok(()),
                            Some(current) if current != *installed => {
                                return Err(io::Error::other(
                                    "registry value changed since the install; preserved",
                                ));
                            }
                            _ => {}
                        }
                    }
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::RegistryValue;
    use std::collections::BTreeSet;
    use winreg::{RegKey, enums::*};

    /// Per-user keys named for this process, removed whatever the outcome.
    struct Scratch(Vec<String>);
    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Ok(software) =
                RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software", KEY_ALL_ACCESS)
            {
                for name in &self.0 {
                    let _ = software.delete_subkey_all(name);
                }
            }
        }
    }

    fn set(path: &str, name: &str, data: &str) {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(format!("Software\\{path}"))
            .unwrap();
        key.set_value(name, &String::from(data)).unwrap();
    }

    fn exists(name: &str) -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(format!("Software\\{name}"))
            .is_ok()
    }

    /// The game's installer writes `name\Game`; meanwhile another program
    /// creates `other`. Only the game's key is recorded.
    fn install(name: &str, other: &str) -> Vec<RegistryEffect> {
        set(&format!("{name}\\Game"), "InstallPath", "C:\\Games\\Game");
        set(other, "LastRun", "today");
        let owner = Owner::new(std::path::Path::new("C:\\Games\\Game"), &[]);
        let effects = created_effects(
            false,
            true,
            &BTreeSet::from([name.to_owned(), other.to_owned()]),
            &owner,
        )
        .unwrap();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            RegistryEffect::Value { name, installed: Some(RegistryValue::String(data)), .. }
                if name == "InstallPath" && data == "C:\\Games\\Game"
        )));
        assert!(
            effects
                .iter()
                .all(|effect| !effect.location().path.as_str().contains(other)),
            "another program's key was claimed: {effects:?}"
        );
        effects
    }

    #[test]
    fn created_keys_go_and_values_the_user_changed_stay() {
        let name = format!("CaskadeRegistryWatchTest{}", std::process::id());
        let other = format!("CaskadeUnrelatedTest{}", std::process::id());
        let _scratch = Scratch(vec![name.clone(), other.clone()]);

        // Unchanged since the install: the game's keys go, values first; the
        // key another program made meanwhile stays.
        for effect in install(&name, &other).iter().rev() {
            PlatformRegistry.undo_effect(effect).unwrap();
        }
        assert!(!exists(&name));
        assert!(exists(&other));

        // Changed by the user: the value, and so its keys, stay.
        let effects = install(&name, &other);
        set(&format!("{name}\\Game"), "InstallPath", "D:\\Mine");
        let failures = effects
            .iter()
            .rev()
            .filter(|effect| PlatformRegistry.undo_effect(effect).is_err())
            .count();
        assert_eq!(failures, 3, "the value and both keys are preserved");
        let kept: String = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(format!("Software\\{name}\\Game"))
            .unwrap()
            .get_value("InstallPath")
            .unwrap();
        assert_eq!(kept, "D:\\Mine");
    }

    #[test]
    fn a_key_mentions_the_game_only_by_its_folder_or_below() {
        let owner = Owner::new(
            std::path::Path::new("C:\\Games\\Game"),
            &["/D=E:\\Alias\\G".into()],
        );
        for (text, mentions) in [
            ("C:\\Games\\Game", true),
            ("c:/games/game/bin/game.exe", true),
            ("\"C:\\Games\\Game\\game.exe\" -fullscreen", true),
            ("E:\\Alias\\G\\save", true),
            ("C:\\Games\\Gamebox", false),
            ("C:\\Games", false),
        ] {
            assert_eq!(owner.mentions(text), mentions, "{text}");
        }
    }
}
