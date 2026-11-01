//! Explicit, opt-in replay of statically understood prefix effects. Pure
//! extraction remains independent of a configured process runtime.
use super::{InnoError, InnoMetadata, Result, app_path};
use crate::{RegistryEntry, RegistryHive, RegistryValue, RuntimeSpec, Step};
use std::{collections::BTreeMap, path::Path};
fn expand(value: &str) -> Result<String> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(stripped) = tail.strip_prefix("{{") {
            result.push('{');
            rest = stripped;
            continue;
        }
        let end = tail
            .find('}')
            .ok_or_else(|| InnoError::Unsupported("unterminated Inno constant".into()))?;
        if !tail[1..end].eq_ignore_ascii_case("app") {
            return Err(InnoError::Unsupported(
                "prefix replay only resolves {app}".into(),
            ));
        }
        let after = &tail[end + 1..];
        result.push_str(crate::runtime::proton::GAME_DIR);
        rest = after;
    }
    result.push_str(rest);
    if result.contains(['\0', '\r', '\n']) {
        return Err(InnoError::Unsupported(
            "multiline prefix replay value".into(),
        ));
    }
    Ok(result)
}
impl InnoMetadata {
    /// Reproduce supported new [INI] files and typed registry strings in an
    /// explicitly configured Wine/Proton prefix. Unsupported constants, native
    /// host imports and overwriting embedded INI files fail before any writes.
    /// This helper never executes Pascal code, [Run], or arbitrary commands.
    pub fn prefix_replay_steps(&self, runtime: RuntimeSpec) -> Result<Vec<Step>> {
        match &runtime {
            RuntimeSpec::Native => {
                return Err(InnoError::Unsupported(
                    "registry replay requires a prefix runtime".into(),
                ));
            }
            RuntimeSpec::WinePrefix { id } | RuntimeSpec::ProtonPrefix { id }
                if id.is_empty() || id.contains(['\0', '/', '\\']) || id == ".." =>
            {
                return Err(InnoError::Unsupported("invalid prefix ID".into()));
            }
            _ => {}
        }
        if self.password_required {
            return Err(InnoError::PasswordRequired);
        }
        let mut files: BTreeMap<std::path::PathBuf, BTreeMap<String, BTreeMap<String, String>>> =
            BTreeMap::new();
        for ini in &self.ini {
            let path = app_path(&ini.filename)?
                .filter(|path| path != Path::new("."))
                .ok_or_else(|| {
                    InnoError::Unsupported("INI replay must target a file below {app}".into())
                })?;
            if self
                .files
                .iter()
                .any(|file| app_path(&file.destination).ok().flatten().as_ref() == Some(&path))
            {
                return Err(InnoError::Unsupported(
                    "INI replay cannot replace an embedded file without a merge step".into(),
                ));
            }
            if ini.section.contains(['[', ']', '\0', '\r', '\n'])
                || ini.key.is_empty()
                || ini.key.contains(['=', '[', ']', '\0', '\r', '\n'])
            {
                return Err(InnoError::Unsupported(
                    "unsupported INI section or key".into(),
                ));
            }
            files
                .entry(path)
                .or_default()
                .entry(ini.section.clone())
                .or_default()
                .insert(ini.key.clone(), expand(&ini.value)?);
        }
        let mut steps = vec![];
        for (path, sections) in files {
            let mut contents = String::new();
            for (section, values) in sections {
                if !section.is_empty() {
                    contents.push('[');
                    contents.push_str(&section);
                    contents.push_str("]\r\n");
                }
                for (key, value) in values {
                    contents.push_str(&key);
                    contents.push('=');
                    contents.push_str(&value);
                    contents.push_str("\r\n");
                }
            }
            steps.push(Step::WriteFile {
                path,
                contents: contents.into_bytes(),
            });
        }
        let mut entries = vec![];
        for entry in &self.registry {
            let hive = match entry.hive.as_str() {
                "HKeyCurrentUser" | "HKEY_CURRENT_USER" | "HKCU" => RegistryHive::CurrentUser,
                "HKeyLocalMachine" | "HKEY_LOCAL_MACHINE" | "HKLM" => RegistryHive::LocalMachine,
                _ => {
                    return Err(InnoError::Unsupported(
                        "prefix replay only supports HKCU/HKLM".into(),
                    ));
                }
            };
            let key = expand(&entry.key)?;
            let name = expand(&entry.name)?;
            let value = expand(&entry.value)?;
            if key.starts_with(['\\', '/'])
                || key
                    .split(['\\', '/'])
                    .any(|part| part == ".." || part.is_empty())
            {
                return Err(InnoError::Unsupported("unsafe registry replay key".into()));
            }
            let value = match entry.value_type.to_ascii_lowercase().as_str() {
                "string" | "sz" | "reg_sz" => RegistryValue::String(value),
                "expandstring" | "expandsz" | "reg_expand_sz" => RegistryValue::ExpandString(value),
                _ => {
                    return Err(InnoError::Unsupported(
                        "registry replay value type is not supported".into(),
                    ));
                }
            };
            entries.push(RegistryEntry {
                hive,
                key,
                name,
                value,
            });
        }
        if !entries.is_empty() {
            steps.push(Step::RegistryImport { runtime, entries });
        }
        Ok(steps)
    }
}
