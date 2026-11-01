//! The config an install writes so the first launch needs no setup
//! (EMULATOR-SETUP-PLAN §2.3): INI keys set, whole files written once.

use crate::{
    Error, Result,
    emulation::{
        EmulatorDefinition, EmulatorInstall, OperatingSystem, SetupAction, VarContext,
        vars::Expander,
    },
};
use std::{fs, path::Path};

/// Applies the definition's setup steps for `install` on `os`. A step that
/// fails is reported and the rest still run.
pub(super) fn apply(
    definition: &EmulatorDefinition,
    install: &EmulatorInstall,
    os: OperatingSystem,
    context: &VarContext,
) -> Vec<String> {
    let expander = Expander {
        vars: &definition.vars,
        os,
        kind: install.kind(),
        context,
    };
    let mut warnings = Vec::new();
    for step in definition.setup.iter().filter(|step| step.applies(os)) {
        let done = match step.action() {
            Some(SetupAction::Ini {
                path,
                section,
                key,
                value,
            }) => expander
                .expand(path)
                .and_then(|path| Ok((path, expander.expand(value)?)))
                .map_err(Error::Invalid)
                .and_then(|(path, value)| set_ini(Path::new(&path), section, key, &value)),
            Some(SetupAction::File { path, text }) => expander
                .expand(path)
                .map_err(Error::Invalid)
                .and_then(|path| write_once(Path::new(&path), text)),
            None => continue,
        };
        if let Err(error) = done {
            tracing::warn!(emulator = definition.id, %error, "first-run setup step failed");
            warnings.push(crate::tr!(
                "Couldn't prepare {emulator}'s settings: {error}",
                emulator = definition.name,
                error = error
            ));
        }
    }
    warnings
}

fn write_once(path: &Path, text: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(fs::write(path, text)?)
}

fn set_ini(path: &Path, section: &str, key: &str, value: &str) -> Result<()> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let next = with_key(&text, section, key, value);
    if next != text {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, next)?;
    }
    Ok(())
}

/// `text` with `key = value` in `[section]`: the key's line replaced, or
/// added after the section's last entry, or a new section at the end.
fn with_key(text: &str, section: &str, key: &str, value: &str) -> String {
    let entry = format!("{key} = {value}");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    fn header(line: &str) -> Option<&str> {
        let line = line.trim();
        (line.starts_with('[') && line.ends_with(']')).then(|| line[1..line.len() - 1].trim())
    }
    let start = lines.iter().position(|line| header(line) == Some(section));
    match start {
        Some(start) => {
            let end = lines[start + 1..]
                .iter()
                .position(|line| header(line).is_some())
                .map_or(lines.len(), |offset| start + 1 + offset);
            let existing = lines[start + 1..end].iter().position(|line| {
                line.split_once('=')
                    .is_some_and(|(name, _)| name.trim() == key)
            });
            match existing {
                Some(offset) => lines[start + 1 + offset] = entry,
                None => {
                    // After the last non-blank line of the section.
                    let last = lines[start..end]
                        .iter()
                        .rposition(|line| !line.trim().is_empty())
                        .map_or(start, |offset| start + offset);
                    lines.insert(last + 1, entry);
                }
            }
        }
        None => {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(entry);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn ini_keys_are_replaced_added_or_given_a_section() {
        let text = "[UI]\nTheme = Dark\nSetupWizardIncomplete = true\n\n[Folders]\nSaves = s\n";
        assert_eq!(
            with_key(text, "UI", "SetupWizardIncomplete", "false"),
            "[UI]\nTheme = Dark\nSetupWizardIncomplete = false\n\n[Folders]\nSaves = s\n"
        );
        assert_eq!(
            with_key(text, "Folders", "Bios", "/b"),
            "[UI]\nTheme = Dark\nSetupWizardIncomplete = true\n\n[Folders]\nSaves = s\nBios = /b\n"
        );
        assert_eq!(
            with_key(text, "UI", "Language", "en"),
            "[UI]\nTheme = Dark\nSetupWizardIncomplete = true\nLanguage = en\n\n[Folders]\nSaves = s\n"
        );
        assert_eq!(
            with_key("", "Main", "SetupWizardIncomplete", "false"),
            "[Main]\nSetupWizardIncomplete = false\n"
        );
        assert_eq!(
            with_key("[Main]\nA = 1", "BIOS", "SearchDirectory", "/b"),
            "[Main]\nA = 1\n\n[BIOS]\nSearchDirectory = /b\n"
        );
    }

    #[test]
    fn files_are_written_only_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("a/portable.txt");
        write_once(&path, "first").unwrap();
        write_once(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "first");
    }
}
