//! Concrete [`RegistryStore`] / [`ShortcutEditor`] backends for moves.
use super::{RegistryStore, ShortcutEditor};
use crate::{
    RegistryEntry, RegistryHive, RegistryValue,
    runtime::{ProcessCommand, Runtime, proton::GAME_DIR, registry},
    scan,
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

fn utf16_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

fn hive(name: &str) -> io::Result<RegistryHive> {
    RegistryHive::parse(name)
        .ok_or_else(|| io::Error::other(format!("unsupported registry hive {name}")))
}

/// Raw value bytes (as read) to a typed entry for a `.reg` import.
fn typed_entry(write: &scan::RegistryValue) -> io::Result<RegistryEntry> {
    let text = || utf16_text(&write.data).trim_end_matches('\0').to_owned();
    let value = match write.value_type.as_str() {
        "REG_SZ" => RegistryValue::String(text()),
        "REG_EXPAND_SZ" => RegistryValue::ExpandString(text()),
        "REG_MULTI_SZ" => RegistryValue::MultiString(
            text()
                .split('\0')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        _ => RegistryValue::Binary(write.data.clone()),
    };
    Ok(RegistryEntry {
        hive: hive(&write.hive)?,
        key: write.key.clone(),
        name: write.name.clone(),
        value,
    })
}

fn run(runtime: &dyn Runtime, command: &ProcessCommand, what: &str) -> io::Result<()> {
    let process = runtime.spawn(command).map_err(io::Error::other)?;
    let exit = runtime.wait(process, Duration::from_secs(600));
    runtime.kill(process).map_err(io::Error::other)?;
    let exit = exit.map_err(io::Error::other)?;
    if exit.code == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{what} exited with code {}",
            exit.code
        )))
    }
}

/// The registry of a Proton/Wine prefix: values are read from its
/// `system.reg`/`user.reg`; a batch is one `regedit /S` import run through
/// the prefix runtime (game directory = `G:\Game`).
pub struct PrefixRegistry {
    runtime: Arc<dyn Runtime>,
    prefix: PathBuf,
    game_dir: PathBuf,
    /// Each hive file's values, parsed once until the file changes on disk
    /// or `apply` writes: a move reads one value per reference.
    hives: Mutex<BTreeMap<&'static str, ParsedHive>>,
}

struct ParsedHive {
    modified: SystemTime,
    len: u64,
    values: Vec<scan::RegistryValue>,
}

impl PrefixRegistry {
    pub fn new(runtime: Arc<dyn Runtime>, prefix: PathBuf, game_dir: PathBuf) -> Self {
        Self {
            runtime,
            prefix,
            game_dir,
            hives: Mutex::default(),
        }
    }
}

impl RegistryStore for PrefixRegistry {
    fn read(
        &self,
        hive_name: &str,
        key: &str,
        name: &str,
    ) -> io::Result<Option<scan::RegistryValue>> {
        let file = match hive(hive_name)? {
            RegistryHive::LocalMachine => "system.reg",
            _ => "user.reg",
        };
        let path = self.prefix.join(file);
        let not_found = |error: &io::Error| error.kind() == io::ErrorKind::NotFound;
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if not_found(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let (modified, len) = (metadata.modified()?, metadata.len());
        let mut hives = self.hives.lock().unwrap_or_else(PoisonError::into_inner);
        if !hives
            .get(file)
            .is_some_and(|parsed| parsed.modified == modified && parsed.len == len)
        {
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) if not_found(&error) => return Ok(None),
                Err(error) => return Err(error),
            };
            let values = registry::parse_prefix_reg(&text, hive_name)?;
            hives.insert(
                file,
                ParsedHive {
                    modified,
                    len,
                    values,
                },
            );
        }
        Ok(hives[file]
            .values
            .iter()
            .find(|value| {
                value.key.eq_ignore_ascii_case(key) && value.name.eq_ignore_ascii_case(name)
            })
            .map(|value| scan::RegistryValue {
                hive: hive_name.into(),
                ..value.clone()
            }))
    }

    fn apply(&self, writes: &[scan::RegistryValue]) -> io::Result<()> {
        if writes.is_empty() {
            return Ok(());
        }
        let entries = writes
            .iter()
            .map(typed_entry)
            .collect::<io::Result<Vec<_>>>()?;
        // A private, never-preexisting file: create_new refuses to follow or
        // clobber anything already at that name.
        let name = format!(".caskade-relocate-{}.reg", std::process::id());
        let path = self.game_dir.join(&name);
        {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(registry::reg_file(&entries).as_bytes())?;
        }
        let result = run(
            &*self.runtime,
            &ProcessCommand {
                program: "regedit".into(),
                args: vec!["/S".into(), format!("{GAME_DIR}\\{name}")],
                env: BTreeMap::from([(
                    "WINEPREFIX".to_owned(),
                    self.prefix.to_string_lossy().into_owned(),
                )]),
                cwd: self.game_dir.clone(),
            },
            "regedit",
        );
        self.hives
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let removed = fs::remove_file(&path);
        result.and(removed)
    }
}

/// No registry (hosts or games without one): nothing to read or write.
pub struct NoRegistry;
impl RegistryStore for NoRegistry {
    fn read(&self, _: &str, _: &str, _: &str) -> io::Result<Option<scan::RegistryValue>> {
        Ok(None)
    }
    fn apply(&self, writes: &[scan::RegistryValue]) -> io::Result<()> {
        if writes.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other("no registry backend for this game"))
        }
    }
}

/// Shortcuts cannot be edited on this host; any `.lnk` rewrite fails, so the
/// move rolls back instead of leaving a stale shortcut.
pub struct NoShortcuts;
impl ShortcutEditor for NoShortcuts {
    fn rewrite(&self, path: &Path, _: &str, _: &str) -> io::Result<()> {
        Err(io::Error::other(format!(
            "cannot edit shortcut {} on this host",
            path.display()
        )))
    }
}

/// The host registry on Windows, always in its 64-bit view: HKLM's 32-bit
/// view is addressed at its `Software\WOW6432Node` path, as the reference
/// scan reports it. Values are written directly; HKLM values this process
/// may not write go into one `.reg` imported by a single elevated
/// `reg import` (one OS prompt). Caskade never elevates otherwise. Every
/// write is read back: the elevated import's exit status is not evidence
/// that it wrote anything (#38).
#[cfg(windows)]
pub struct WindowsRegistry {
    pub runtime: Arc<dyn Runtime>,
    pub scratch: PathBuf,
}

#[cfg(windows)]
impl WindowsRegistry {
    fn import_elevated(&self, entries: &[RegistryEntry]) -> io::Result<()> {
        fs::create_dir_all(&self.scratch)?;
        let file = self.scratch.join("caskade-relocate-hklm.reg");
        fs::write(&file, registry::reg_file(entries))?;
        // One argument string with the path quoted ([char]34): Start-Process
        // joins an argument array with spaces and quotes nothing, so a path
        // with a space reached reg.exe as several arguments.
        let script = "$p = Start-Process -FilePath reg.exe \
                      -ArgumentList ('import ' + [char]34 + $env:CASKADE_REG + [char]34 + ' /reg:64') \
                      -Verb RunAs -Wait -WindowStyle Hidden -PassThru; exit $p.ExitCode";
        let result = run(
            &*self.runtime,
            &ProcessCommand {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    script.into(),
                ],
                env: BTreeMap::from([(
                    "CASKADE_REG".to_owned(),
                    file.to_string_lossy().into_owned(),
                )]),
                cwd: self.scratch.clone(),
            },
            "elevated reg import",
        );
        let removed = fs::remove_file(&file);
        result.and(removed)
    }
}

#[cfg(windows)]
impl RegistryStore for WindowsRegistry {
    fn read(
        &self,
        hive_name: &str,
        key: &str,
        name: &str,
    ) -> io::Result<Option<scan::RegistryValue>> {
        use winreg::{
            RegKey,
            enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY},
        };
        let root = match hive(hive_name)? {
            RegistryHive::LocalMachine => HKEY_LOCAL_MACHINE,
            _ => HKEY_CURRENT_USER,
        };
        let opened =
            match RegKey::predef(root).open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY) {
                Ok(opened) => opened,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
        match opened.get_raw_value(name) {
            Ok(value) => Ok(Some(scan::RegistryValue {
                hive: hive_name.into(),
                key: key.into(),
                name: name.into(),
                value_type: format!("{:?}", value.vtype),
                data: value.bytes.into_owned(),
            })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn apply(&self, writes: &[scan::RegistryValue]) -> io::Result<()> {
        use winreg::{
            RegKey, RegValue,
            enums::{
                HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, KEY_WRITE, REG_BINARY,
                REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ,
            },
        };
        let mut elevated = vec![];
        for write in writes {
            let machine = hive(&write.hive)? == RegistryHive::LocalMachine;
            let root = RegKey::predef(if machine {
                HKEY_LOCAL_MACHINE
            } else {
                HKEY_CURRENT_USER
            });
            let vtype = match write.value_type.as_str() {
                "REG_SZ" => REG_SZ,
                "REG_EXPAND_SZ" => REG_EXPAND_SZ,
                "REG_MULTI_SZ" => REG_MULTI_SZ,
                "REG_DWORD" => REG_DWORD,
                "REG_QWORD" => REG_QWORD,
                _ => REG_BINARY,
            };
            let written = root
                .create_subkey_with_flags(&write.key, KEY_WRITE | KEY_WOW64_64KEY)
                .and_then(|(key, _)| {
                    key.set_raw_value(
                        &write.name,
                        &RegValue {
                            bytes: write.data.clone().into(),
                            vtype,
                        },
                    )
                });
            match written {
                Ok(()) => {}
                // Not elevated: HKLM goes to the one elevated import.
                Err(error) if machine && error.kind() == io::ErrorKind::PermissionDenied => {
                    elevated.push(typed_entry(write)?)
                }
                Err(error) => return Err(error),
            }
        }
        if !elevated.is_empty() {
            self.import_elevated(&elevated)?;
        }
        for write in writes {
            let now = self.read(&write.hive, &write.key, &write.name)?;
            let written = match &now {
                Some(now) => typed_entry(now)?.value == typed_entry(write)?.value,
                None => false,
            };
            if !written {
                return Err(io::Error::other(format!(
                    "registry value {}\\{}\\{} was not written",
                    write.hive, write.key, write.name
                )));
            }
        }
        Ok(())
    }
}

/// `.lnk` target, working directory, arguments and icon rewritten through
/// the Windows Shell (`WScript.Shell`, i.e. `IShellLinkW`) from PowerShell,
/// so this crate needs no unsafe COM code.
#[cfg(windows)]
pub struct ShellShortcuts {
    pub runtime: Arc<dyn Runtime>,
}

#[cfg(windows)]
impl ShortcutEditor for ShellShortcuts {
    fn rewrite(&self, path: &Path, old: &str, new: &str) -> io::Result<()> {
        let script = "$l=(New-Object -ComObject WScript.Shell).CreateShortcut($env:CASKADE_LNK); \
                      $o=[regex]::Escape($env:CASKADE_OLD); \
                      foreach($p in 'TargetPath','WorkingDirectory','Arguments','IconLocation'){ \
                      $l.$p = ($l.$p -ireplace $o, $env:CASKADE_NEW.Replace('$','$$')) }; $l.Save()";
        run(
            &*self.runtime,
            &ProcessCommand {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    script.into(),
                ],
                env: BTreeMap::from([
                    (
                        "CASKADE_LNK".to_owned(),
                        path.to_string_lossy().into_owned(),
                    ),
                    ("CASKADE_OLD".to_owned(), old.to_owned()),
                    ("CASKADE_NEW".to_owned(), new.to_owned()),
                ]),
                cwd: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            },
            "shortcut update",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_become_typed_reg_entries() {
        let utf16 =
            |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
        let write = |value_type: &str, data: Vec<u8>| scan::RegistryValue {
            hive: "HKLM".into(),
            key: r"Software\Game".into(),
            name: "Path".into(),
            value_type: value_type.into(),
            data,
        };
        assert_eq!(
            typed_entry(&write("REG_SZ", utf16("D:\\Games\\X\0")))
                .unwrap()
                .value,
            RegistryValue::String("D:\\Games\\X".into())
        );
        assert_eq!(
            typed_entry(&write("REG_MULTI_SZ", utf16("a\0b\0\0")))
                .unwrap()
                .value,
            RegistryValue::MultiString(vec!["a".into(), "b".into()])
        );
        assert_eq!(
            typed_entry(&write("REG_BINARY", vec![1, 2])).unwrap().value,
            RegistryValue::Binary(vec![1, 2])
        );
        assert_eq!(
            typed_entry(&write("REG_SZ", vec![])).unwrap().hive,
            RegistryHive::LocalMachine
        );
        assert!(NoRegistry.apply(&[write("REG_SZ", vec![])]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn prefix_registry_reads_wine_reg_files() {
        let prefix = tempfile::tempdir().unwrap();
        fs::write(
            prefix.path().join("system.reg"),
            "WINE REGISTRY Version 2\n\n[Software\\\\Indie] 1\n\"InstallPath\"=\"G:\\\\Game\"\n",
        )
        .unwrap();
        let store = PrefixRegistry::new(
            Arc::new(crate::runtime::NativeRuntime::new()),
            prefix.path().into(),
            prefix.path().into(),
        );
        let value = store
            .read("HKLM", r"software\indie", "installpath")
            .unwrap()
            .unwrap();
        assert_eq!(value.value_type, "REG_SZ");
        assert_eq!(utf16_text(&value.data), "G:\\Game");
        assert!(
            store
                .read("HKCU", r"Software\Indie", "InstallPath")
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn prefix_registry_read_fails_on_an_unreadable_registry() {
        let prefix = tempfile::tempdir().unwrap();
        // A key that decodes to a NUL is rejected by the bounded reader; the
        // store must report it instead of answering "no such value".
        fs::write(
            prefix.path().join("system.reg"),
            "WINE REGISTRY Version 2\n\n[Software\\\\Bad\\0Key] 1\n\"InstallPath\"=\"G:\\\\Game\"\n",
        )
        .unwrap();
        let store = PrefixRegistry::new(
            Arc::new(crate::runtime::NativeRuntime::new()),
            prefix.path().into(),
            prefix.path().into(),
        );
        let error = store
            .read("HKLM", r"Software\Bad", "InstallPath")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
