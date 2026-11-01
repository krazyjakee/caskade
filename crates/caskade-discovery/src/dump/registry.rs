use super::{Result, scrub::Scrubber};
use crate::registry::{Registry, RegistryHive, RegistryKey, RegistryValue, RegistryView};

const ROOTS: &[&str] = &[
    r"Software\GOG.com\Games",
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
    r"Software\Valve\Steam",
    r"Software\Ubisoft\Launcher\Installs",
];
fn allowed(name: &str) -> bool {
    [
        "displayname",
        "displayversion",
        "publisher",
        "installlocation",
        "uninstallstring",
        "quietuninstallstring",
        "displayicon",
        "estimatedsize",
        "systemcomponent",
        "noremove",
        "gameid",
        "gamename",
        "path",
        "exe",
        "launchcommand",
        "workingdir",
        "steampath",
        "steamexe",
        "installdir",
    ]
    .contains(&name.to_ascii_lowercase().as_str())
}
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
fn hex(kind: &str, bytes: impl IntoIterator<Item = u8>) -> String {
    format!(
        "hex{kind}:{}",
        bytes
            .into_iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn value(value: RegistryValue, key: &str, scrub: &Scrubber) -> Option<String> {
    let wide = |text: String| {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    Some(match value {
        RegistryValue::String(text) => {
            let text = scrub.field(key, &text);
            if text.chars().any(char::is_control) {
                hex("(1)", wide(text))
            } else {
                format!("\"{}\"", escape(&text))
            }
        }
        RegistryValue::ExpandString(text) => hex("(2)", wide(scrub.field(key, &text))),
        RegistryValue::MultiString(values) => {
            let text = values
                .iter()
                .map(|value| scrub.field(key, value))
                .collect::<Vec<_>>()
                .join("\0")
                + "\0";
            hex("(7)", wide(text))
        }
        RegistryValue::Dword(n) => format!("dword:{n:08x}"),
        RegistryValue::Qword(n) => hex("(b)", n.to_le_bytes()),
        RegistryValue::Binary(_) => return None,
    })
}
pub(super) fn export(
    registry: &dyn Registry,
    hive: RegistryHive,
    view: RegistryView,
    scrub: &Scrubber,
) -> Result<String> {
    let mut out = "Windows Registry Editor Version 5.00\n".to_owned();
    let mut count = 0;
    for root in ROOTS {
        walk(
            registry,
            &RegistryKey::new(hive, view, *root),
            scrub,
            &mut out,
            &mut count,
            0,
        )?;
    }
    Ok(out)
}
fn walk(
    registry: &dyn Registry,
    key: &RegistryKey,
    scrub: &Scrubber,
    out: &mut String,
    count: &mut usize,
    depth: usize,
) -> Result<()> {
    if depth > 12 || *count > 4096 {
        return Err(super::DumpError::Limit("registry keys"));
    }
    if !registry.key_exists(key)? {
        return Ok(());
    }
    *count += 1;
    let mut values = registry.values(key)?;
    values.sort_by(|a, b| a.name.cmp(&b.name));
    let mut lines = vec![];
    for entry in values {
        if !allowed(&entry.name) {
            continue;
        }
        if let Some(value) = value(entry.value, &entry.name, scrub) {
            lines.push(format!("\"{}\"={value}", escape(&entry.name)));
        }
    }
    if !lines.is_empty() {
        let hive = match key.hive {
            RegistryHive::CurrentUser => "HKEY_CURRENT_USER",
            RegistryHive::LocalMachine => "HKEY_LOCAL_MACHINE",
            _ => return Ok(()),
        };
        out.push_str(&format!(
            "\n[{hive}\\{}]\n{}\n",
            scrub.text(&key.path),
            lines.join("\n")
        ));
    }
    let mut children = registry.subkeys(key)?;
    children.sort();
    for child in children {
        if child.contains(['\\', '/', '\0']) {
            continue;
        }
        walk(
            registry,
            &RegistryKey::new(key.hive, key.view, format!("{}\\{child}", key.path)),
            scrub,
            out,
            count,
            depth + 1,
        )?;
    }
    Ok(())
}
