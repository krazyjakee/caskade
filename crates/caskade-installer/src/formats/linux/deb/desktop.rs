use super::{GeneratedReference, PackageError, Result};
use crate::{
    formats::archive::ExtractionControl,
    safety::{SafeDir, sanitize_path},
};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn mapped(dir: &SafeDir, absolute: &str, root: &Path, directory: bool) -> Result<String> {
    let relative = sanitize_path(
        absolute
            .strip_prefix('/')
            .ok_or_else(|| PackageError::Invalid("desktop path is not absolute".into()))?,
    )?;
    let present = dir.extracted_files().any(|path| {
        if directory {
            path.starts_with(&relative)
        } else {
            path == relative
        }
    });
    if !present {
        return Err(PackageError::RequiresDelegate(format!(
            "desktop path {absolute} is outside packaged files"
        )));
    }
    let path = root
        .join(relative)
        .to_str()
        .ok_or_else(|| PackageError::Invalid("game path encoding".into()))?
        .to_owned();
    if path.contains(['\n', '\r', '\0']) {
        return Err(PackageError::Invalid(
            "game path contains desktop control characters".into(),
        ));
    }
    Ok(path)
}
pub(super) fn first_word(value: &str) -> Result<(String, &str)> {
    let value = value.trim_start();
    if let Some(quoted) = value.strip_prefix('"') {
        let mut word = String::new();
        let mut escaped = false;
        for (index, ch) in quoted.char_indices() {
            if escaped {
                word.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Ok((word, &quoted[index + 1..]));
            } else {
                word.push(ch);
            }
        }
        return Err(PackageError::Invalid(
            "unterminated desktop Exec quote".into(),
        ));
    }
    let end = value.find(char::is_whitespace).unwrap_or(value.len());
    Ok((value[..end].into(), &value[end..]))
}
fn exec_quote(path: &str) -> String {
    format!(
        "\"{}\"",
        path.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%")
    )
}
/// Rewrites are confined and written as Caskade-generated reference records.
/// The supplied root must be the final collision-resolved game path, not stage.
pub fn rewrite_desktop_files(
    dir: &mut SafeDir,
    root: &Path,
    control: &mut dyn ExtractionControl,
) -> Result<Vec<GeneratedReference>> {
    let desktops: Vec<PathBuf> = dir
        .extracted_files()
        .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
        .map(Path::to_path_buf)
        .collect();
    let mut records = Vec::new();
    let mut metadata_bytes = 0_usize;
    for path in desktops {
        control.checkpoint(0)?;
        let name = path
            .to_str()
            .ok_or_else(|| PackageError::Invalid("desktop path encoding".into()))?;
        let mut bytes = Vec::new();
        dir.open_read(name)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err(PackageError::MetadataLimit);
        }
        metadata_bytes = metadata_bytes
            .checked_add(bytes.len())
            .ok_or(PackageError::MetadataLimit)?;
        if metadata_bytes > 16 * 1024 * 1024 {
            return Err(PackageError::MetadataLimit);
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| PackageError::Invalid("desktop file encoding".into()))?;
        let mut output = String::new();
        for line in text.lines() {
            let replacement = if let Some(value) = line.strip_prefix("Exec=") {
                let (word, suffix) = first_word(value)?;
                if word.starts_with('/') {
                    Some((
                        "Exec",
                        value,
                        format!(
                            "{}{}",
                            exec_quote(&mapped(dir, &word, root, false)?),
                            suffix
                        ),
                    ))
                } else {
                    None
                }
            } else if let Some(value) = line.strip_prefix("Icon=") {
                if value.starts_with('/') {
                    Some(("Icon", value, mapped(dir, value, root, false)?))
                } else {
                    None
                }
            } else if let Some(value) = line.strip_prefix("Path=") {
                if value.starts_with('/') {
                    Some(("Path", value, mapped(dir, value, root, true)?))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((key, original, replacement)) = replacement {
                metadata_bytes = metadata_bytes
                    .checked_add(original.len() + replacement.len())
                    .ok_or(PackageError::MetadataLimit)?;
                if metadata_bytes > 16 * 1024 * 1024 {
                    return Err(PackageError::MetadataLimit);
                }
                output.push_str(key);
                output.push('=');
                output.push_str(&replacement);
                records.push(GeneratedReference {
                    path: path.clone(),
                    key: key.into(),
                    original: original.into(),
                    replacement,
                    origin: "caskade-generated".into(),
                });
            } else {
                output.push_str(line);
            }
            output.push('\n');
        }
        if output.as_bytes() != bytes {
            control.checkpoint(output.len() as u64)?;
            dir.rewrite_generated_file(name, output.as_bytes())?;
        }
    }
    if !records.is_empty() {
        let bytes = serde_json::to_vec_pretty(&records)
            .map_err(|e| PackageError::Invalid(e.to_string()))?;
        dir.write_own_file(".caskade/package-references.json", &bytes)?;
    }
    Ok(records)
}

pub(super) fn check_absolute_payload_references(
    dir: &SafeDir,
    control: &mut dyn ExtractionControl,
) -> Result<()> {
    let paths: Vec<_> = dir
        .extracted_files()
        .filter(|p| !p.starts_with(".caskade") && !p.extension().is_some_and(|e| e == "desktop"))
        .collect();
    let needles: Vec<Vec<u8>> = paths
        .iter()
        .filter_map(|path| {
            path.to_str()
                .map(|s| format!("/{}", s.replace('\\', "/")).into_bytes())
        })
        .collect();
    for path in paths {
        control.checkpoint(0)?;
        let mut file = dir.open_read(
            path.to_str()
                .ok_or_else(|| PackageError::Invalid("payload path encoding".into()))?,
        )?;
        let mut prefix = [0; 4];
        let length = file.read(&mut prefix)?;
        if !prefix[..length].starts_with(b"#!") && !prefix[..length].starts_with(b"\x7fELF") {
            continue;
        }
        let mut bytes = prefix[..length].to_vec();
        file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(PackageError::RequiresDelegate(
                "executable exceeds static relocation scan budget".into(),
            ));
        }
        if needles
            .iter()
            .any(|needle| bytes.windows(needle.len()).any(|w| w == needle))
        {
            return Err(PackageError::RequiresDelegate(
                "payload contains absolute references to packaged files".into(),
            ));
        }
    }
    Ok(())
}

/// The arguments after a desktop entry's Exec command, without the file and
/// URL field codes; `None` when it uses one that needs the desktop's context.
pub(super) fn exec_arguments(mut suffix: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    while !suffix.trim().is_empty() {
        let (word, rest) = first_word(suffix).ok()?;
        if word.contains('\0') {
            return None;
        }
        if rest.len() >= suffix.len() || args.len() >= 256 {
            return None;
        }
        suffix = rest;
        // File/URL inputs are supplied only when launching with documents;
        // installed games have none. Other substitutions need OS context.
        if matches!(word.as_str(), "%f" | "%F" | "%u" | "%U") {
            continue;
        }
        let literal = word.replace("%%", "\0");
        if literal.contains(['%', '\\']) {
            return None;
        }
        args.push(literal.replace('\0', "%"));
    }
    Some(args)
}

/// Recover exact, game-relative launch metadata from the rewrites produced by
/// this handler. Absolute generated desktop paths otherwise cannot be resolved
/// by discovery's deliberately confined relative-path parser.
pub fn discovery_hints(records: &[GeneratedReference]) -> crate::discover::DiscoveryHints {
    let mut hints = crate::discover::DiscoveryHints::default();
    for record in records
        .iter()
        .filter(|r| r.origin == "caskade-generated" && r.key == "Exec")
    {
        let Ok((word, suffix)) = first_word(&record.original) else {
            continue;
        };
        let Some(relative) = word.strip_prefix('/') else {
            continue;
        };
        let Ok(path) = sanitize_path(relative) else {
            continue;
        };
        let Some(args) = exec_arguments(suffix) else {
            continue;
        };
        let working_dir = records
            .iter()
            .find(|r| r.origin == "caskade-generated" && r.path == record.path && r.key == "Path")
            .and_then(|r| r.original.strip_prefix('/'))
            .and_then(|s| sanitize_path(s).ok());
        hints.launch_targets.push(crate::discover::LaunchHint {
            path,
            args,
            working_dir,
            reason: "Package Desktop Entry Exec (Caskade-generated reference)".into(),
        });
    }
    hints
}
