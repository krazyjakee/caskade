//! Native registry reads for the scanner (Windows hosts). Wine/Proton
//! prefixes are read from their `.reg` files in I4.
use super::RegistryValue;
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
};

/// Read every value under each recorded `(hive, key)` and its subkeys, in
/// both registry views for HKLM. Hive spellings from Inno (`HKeyLocalMachine`),
/// MSI and `.reg` files (`HKEY_CURRENT_USER`, `HKCU`) are accepted; unknown
/// hives and missing keys are skipped. At most `max_values` values are read.
pub fn read_keys(keys: &[(String, String)], max_values: usize) -> Vec<RegistryValue> {
    let mut values = vec![];
    for (hive, key) in keys {
        let lower = hive.to_ascii_lowercase().replace('_', "");
        let (root, name, views): (_, _, &[u32]) =
            if lower.contains("currentuser") || lower == "hkcu" {
                (HKEY_CURRENT_USER, "HKCU", &[0])
            } else if lower.contains("localmachine") || lower == "hklm" {
                (
                    HKEY_LOCAL_MACHINE,
                    "HKLM",
                    &[KEY_WOW64_64KEY, KEY_WOW64_32KEY],
                )
            } else {
                continue;
            };
        for view in views {
            let Ok(opened) = RegKey::predef(root).open_subkey_with_flags(key, KEY_READ | view)
            else {
                continue;
            };
            walk(&opened, name, key, *view, &mut values, max_values);
        }
    }
    values.sort_by(|a, b| (&a.hive, &a.key, &a.name).cmp(&(&b.hive, &b.key, &b.name)));
    values.dedup();
    values
}

fn walk(
    key: &RegKey,
    hive: &str,
    path: &str,
    view: u32,
    values: &mut Vec<RegistryValue>,
    max_values: usize,
) {
    for (name, value) in key.enum_values().flatten() {
        if values.len() >= max_values {
            return;
        }
        values.push(RegistryValue {
            hive: hive.into(),
            key: path.into(),
            name,
            value_type: format!("{:?}", value.vtype),
            data: value.bytes.into_owned(),
        });
    }
    for child in key.enum_keys().flatten() {
        if values.len() >= max_values {
            return;
        }
        if let Ok(opened) = key.open_subkey_with_flags(&child, KEY_READ | view) {
            walk(
                &opened,
                hive,
                &format!("{path}\\{child}"),
                view,
                values,
                max_values,
            );
        }
    }
}
