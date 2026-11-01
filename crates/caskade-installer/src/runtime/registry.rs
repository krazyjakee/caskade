//! Prefix registry I/O (#46): `.reg` files to replay installer registry
//! entries into a Proton/Wine prefix with `regedit /S`, and a reader for a
//! prefix's `system.reg`/`user.reg` so the reference scanner (#32) can see
//! values written inside it. The host registry is never touched.
use crate::{RegistryEntry, RegistryHive, RegistryValue, scan};
use std::io;

pub mod read;

fn hive_name(hive: RegistryHive) -> &'static str {
    match hive {
        RegistryHive::CurrentUser => "HKEY_CURRENT_USER",
        RegistryHive::LocalMachine => "HKEY_LOCAL_MACHINE",
        RegistryHive::ClassesRoot => "HKEY_CLASSES_ROOT",
        RegistryHive::Users => "HKEY_USERS",
        RegistryHive::CurrentConfig => "HKEY_CURRENT_CONFIG",
    }
}

fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\r' => quoted.push_str("\\r"),
            '\n' => quoted.push_str("\\n"),
            '\0' => quoted.push_str("\\0"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn hex(kind: Option<u8>, bytes: &[u8]) -> String {
    let prefix = kind.map_or_else(|| "hex:".to_owned(), |kind| format!("hex({kind:x}):"));
    let body: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    prefix + &body.join(",")
}

fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// A `regedit` (version 5) import file for `entries`, grouped by key in
/// input order. Keys and names are written literally; values are typed.
pub fn reg_file(entries: &[RegistryEntry]) -> String {
    let mut text = String::from("Windows Registry Editor Version 5.00\r\n");
    let mut current: Option<String> = None;
    for entry in entries {
        let key = format!(
            "{}\\{}",
            hive_name(entry.hive),
            entry.key.trim_matches('\\')
        );
        if current.as_deref() != Some(key.as_str()) {
            text.push_str(&format!("\r\n[{key}]\r\n"));
            current = Some(key);
        }
        let name = if entry.name.is_empty() {
            "@".to_owned()
        } else {
            quote(&entry.name)
        };
        let value = match &entry.value {
            RegistryValue::String(value) => quote(value),
            RegistryValue::ExpandString(value) => hex(Some(2), &utf16z(value)),
            RegistryValue::MultiString(values) => {
                let mut bytes = vec![];
                for value in values {
                    bytes.extend(utf16z(value));
                }
                bytes.extend([0, 0]);
                hex(Some(7), &bytes)
            }
            RegistryValue::Dword(value) => format!("dword:{value:08x}"),
            RegistryValue::Qword(value) => hex(Some(0xb), &value.to_le_bytes()),
            RegistryValue::Binary(bytes) => hex(None, bytes),
        };
        text.push_str(&format!("{name}={value}\r\n"));
    }
    text
}

/// Values in a Wine prefix registry file (`system.reg` → `hive` "HKLM",
/// `user.reg` → "HKCU"). Strings are returned as UTF-16LE like native
/// `REG_SZ` data; `hex(n):` values keep their bytes. Malformed lines are
/// skipped; an invalid key or an exceeded size limit is an error rather than
/// an empty or partial list.
pub fn parse_prefix_reg(text: &str, hive: &str) -> io::Result<Vec<scan::RegistryValue>> {
    read::parse(text, hive, read::Limits::default()).map(|document| document.values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_file_escapes_and_types_values() {
        let text = reg_file(&[
            RegistryEntry {
                hive: RegistryHive::LocalMachine,
                key: r"Software\CaskadeFixture\FixtureGame".into(),
                name: "InstallPath".into(),
                value: RegistryValue::String(r#"G:\ "quoted""#.into()),
            },
            RegistryEntry {
                hive: RegistryHive::LocalMachine,
                key: r"Software\CaskadeFixture\FixtureGame".into(),
                name: String::new(),
                value: RegistryValue::Dword(0x2a),
            },
        ]);
        assert!(text.starts_with("Windows Registry Editor Version 5.00\r\n"));
        assert!(text.contains("[HKEY_LOCAL_MACHINE\\Software\\CaskadeFixture\\FixtureGame]\r\n"));
        assert!(text.contains(r#""InstallPath"="G:\\ \"quoted\"""#));
        assert!(text.contains("@=dword:0000002a"));
    }

    #[test]
    fn prefix_registry_is_parsed_into_scan_values() {
        let text = "WINE REGISTRY Version 2\n;; All keys relative to REGISTRY\\\\Machine\n\n\
            [Software\\\\CaskadeFixture\\\\FixtureGame] 1793041349\n#time=1dd\n\
            \"InstallPath\"=\"Z:\\\\tmp\\\\game \\\"x\\\"\"\n\
            \"Count\"=dword:00000010\n\
            \"Blob\"=hex:01,02,\\\n  03\n\
            @=\"default\"\n";
        let values = parse_prefix_reg(text, "HKLM").unwrap();
        assert_eq!(values.len(), 4);
        assert_eq!(values[0].key, r"Software\CaskadeFixture\FixtureGame");
        let path: Vec<u8> =
            r#"Z:\tmp\game "x""#.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(values[0].data, path);
        assert_eq!(values[1].data, 16u32.to_le_bytes());
        assert_eq!(
            (values[2].value_type.as_str(), values[2].data.as_slice()),
            ("REG_BINARY", &[1, 2, 3][..])
        );
        assert_eq!(values[3].name, "");
    }
}
