use super::{AppError, BundleMetadata, Result};
use crate::safety::sanitize_path;
use std::{io::Cursor, path::PathBuf};
pub const MAX_PLIST_BYTES: usize = 1024 * 1024;

/// Parse only bounded plist documents. Binary object-table counts and XML depth
/// are checked before plist's general-purpose decoder sees untrusted metadata.
pub fn parse_info(bytes: &[u8]) -> Result<BundleMetadata> {
    if bytes.len() > MAX_PLIST_BYTES {
        return Err(AppError::MetadataLimit);
    }
    if bytes.starts_with(b"bplist00") {
        super::binary::validate(bytes)?;
    } else {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let mut depth = 0_u32;
        let mut events = 0;
        loop {
            events += 1;
            if events > 100_000 {
                return Err(AppError::MetadataLimit);
            }
            match reader
                .read_event()
                .map_err(|e| AppError::Invalid(e.to_string()))?
            {
                quick_xml::events::Event::Start(_) => {
                    depth += 1;
                    if depth > 64 {
                        return Err(AppError::MetadataLimit);
                    }
                }
                quick_xml::events::Event::End(_) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| AppError::Invalid("unbalanced plist".into()))?;
                }
                quick_xml::events::Event::Eof => break,
                _ => (),
            }
        }
    }
    let value = plist::Value::from_reader(Cursor::new(bytes))
        .map_err(|e| AppError::Invalid(e.to_string()))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| AppError::Invalid("Info.plist is not a dictionary".into()))?;
    let string = |key| dict.get(key).and_then(plist::Value::as_string);
    let executable = string("CFBundleExecutable")
        .ok_or_else(|| AppError::Invalid("missing CFBundleExecutable".into()))?;
    let executable = sanitize_path(executable)?;
    if executable.components().count() != 1 {
        return Err(AppError::Invalid(
            "CFBundleExecutable must be a leaf name".into(),
        ));
    }
    let name = string("CFBundleDisplayName")
        .or_else(|| string("CFBundleName"))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("Game")
        .to_owned();
    let identifier = string("CFBundleIdentifier").map(str::to_owned);
    let icon = string("CFBundleIconFile")
        .map(|icon| {
            let mut path = sanitize_path(icon)?;
            if path.extension().is_none() {
                path.set_extension("icns");
            }
            Ok::<_, AppError>(PathBuf::from("Contents/Resources").join(path))
        })
        .transpose()?;
    Ok(BundleMetadata {
        name,
        identifier,
        executable: PathBuf::from("Contents/MacOS").join(executable),
        icon,
    })
}
