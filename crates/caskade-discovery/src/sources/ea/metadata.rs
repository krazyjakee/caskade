//! Original bounded readers for EA installer XML and Origin's form-encoded
//! LocalContent manifests. Format references: BoilR origin_platform.rs
//! (Apache-2.0) and EA installerdata.xml contentIDs. No code was ported.
use crate::InstallState;
use quick_xml::events::Event;
use serde::Serialize;
use std::{collections::BTreeMap, io};

const MAX_BYTES: usize = 4 * 1024 * 1024;
fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn text(bytes: &[u8]) -> io::Result<String> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("EA metadata exceeds 4 MiB"));
    }
    let decoded = if bytes.starts_with(&[0xff, 0xfe]) {
        let (value, errors) = encoding_rs::UTF_16LE.decode_without_bom_handling(&bytes[2..]);
        if errors {
            return Err(invalid("invalid UTF-16LE metadata"));
        }
        value.into_owned()
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        let (value, errors) = encoding_rs::UTF_16BE.decode_without_bom_handling(&bytes[2..]);
        if errors {
            return Err(invalid("invalid UTF-16BE metadata"));
        }
        value.into_owned()
    } else {
        std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map_err(invalid)?
            .to_owned()
    };
    if decoded.contains('\0') {
        return Err(invalid("NUL in EA metadata"));
    }
    Ok(decoded)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct InstallerMetadata {
    pub content_ids: Vec<String>,
    pub title: Option<String>,
    pub executable: Option<String>,
}
struct Element {
    name: String,
    text: String,
    english: bool,
}
/// No DTD/entity loading, filesystem access or subprocesses. Includes node,
/// depth and decoded-string limits before growing parser-owned collections.
pub fn parse_installer_xml(bytes: &[u8]) -> io::Result<InstallerMetadata> {
    let text = text(bytes)?;
    let mut reader = quick_xml::Reader::from_str(&text);
    let mut stack: Vec<Element> = Vec::new();
    let mut result = InstallerMetadata::default();
    let mut title_english = false;
    let mut nodes = 0;
    loop {
        nodes += 1;
        if nodes > 50_000 {
            return Err(invalid("EA XML node limit"));
        }
        let event = reader.read_event().map_err(invalid)?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                if stack.len() >= 32 {
                    return Err(invalid("EA XML depth limit"));
                }
                let name = element.local_name().as_ref().to_ascii_lowercase();
                let mut english = stack.last().is_some_and(|e| e.english);
                let runtime = stack.iter().any(|e| e.name == "runtime");
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(invalid)?;
                    let key = attribute.key.local_name().as_ref().to_ascii_lowercase();
                    let value = quick_xml::escape::unescape(&attribute.value).map_err(invalid)?;
                    if value.len() > 65_536 {
                        return Err(invalid("EA XML attribute limit"));
                    }
                    if key == "locale" || key == "lang" {
                        english = value.replace('-', "_").eq_ignore_ascii_case("en_US")
                            || value.eq_ignore_ascii_case("en");
                    }
                    if runtime
                        && name == "execute"
                        && ["file", "path", "filepath", "executable"].contains(&key.as_str())
                        && result.executable.is_none()
                    {
                        result.executable = Some(value.trim().into());
                    }
                }
                stack.push(Element {
                    name,
                    text: String::new(),
                    english,
                });
                if empty {
                    finish(&mut stack, &mut result, &mut title_english)?;
                }
            }
            Event::End(_) => finish(&mut stack, &mut result, &mut title_english)?,
            Event::Text(value) => append(&mut stack, value.as_ref())?,
            Event::CData(value) => append(&mut stack, value.as_ref())?,
            Event::GeneralRef(value) => {
                let encoded = format!("&{};", value.as_ref());
                let decoded = quick_xml::escape::unescape(&encoded).map_err(invalid)?;
                append(&mut stack, &decoded)?;
            }
            Event::DocType(_) => return Err(invalid("EA XML DTD is forbidden")),
            Event::Eof => {
                if !stack.is_empty() {
                    return Err(invalid("truncated EA XML"));
                }
                break;
            }
            _ => {}
        }
    }
    Ok(result)
}
fn append(stack: &mut [Element], value: &str) -> io::Result<()> {
    if let Some(element) = stack.last_mut() {
        if element.text.len() + value.len() > 65_536 {
            return Err(invalid("EA XML string limit"));
        }
        element.text.push_str(value);
    }
    Ok(())
}
fn finish(
    stack: &mut Vec<Element>,
    result: &mut InstallerMetadata,
    title_english: &mut bool,
) -> io::Result<()> {
    let element = stack.pop().ok_or_else(|| invalid("unbalanced EA XML"))?;
    let value = element.text.trim();
    if value.is_empty() {
        return Ok(());
    }
    match element.name.as_str() {
        "contentid" if stack.iter().any(|e| e.name == "contentids") => {
            if value.len() > 1024 || value.chars().any(char::is_control) {
                return Err(invalid("invalid EA content id"));
            }
            if !result.content_ids.iter().any(|id| id == value) {
                if result.content_ids.len() >= 128 {
                    return Err(invalid("EA content id limit"));
                }
                result.content_ids.push(value.into());
            }
        }
        "title" | "gametitle" | "displayname" => {
            if value.len() <= 4096 && (result.title.is_none() || element.english && !*title_english)
            {
                result.title = Some(value.into());
                *title_english = element.english;
            }
        }
        "filepath" | "executable" | "exe" | "launchexecutable"
            if stack.iter().any(|e| e.name == "runtime") && result.executable.is_none() =>
        {
            result.executable = Some(value.into())
        }
        _ => {}
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OriginManifest {
    pub offer: String,
    pub install_dir: String,
    pub title: Option<String>,
    pub state: InstallState,
    pub size: Option<u64>,
    pub addon: bool,
}
fn form_piece(value: &str) -> io::Result<String> {
    for (index, byte) in value.as_bytes().iter().enumerate() {
        if *byte == b'%'
            && !value
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|p| p.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(invalid("invalid manifest percent escape"));
        }
    }
    let value = value.replace('+', " ");
    let value = percent_encoding::percent_decode_str(&value)
        .decode_utf8()
        .map_err(invalid)?
        .into_owned();
    if value.chars().any(char::is_control) {
        return Err(invalid("control character in manifest field"));
    }
    Ok(value)
}
pub fn parse_origin_manifest(bytes: &[u8]) -> io::Result<OriginManifest> {
    let text = text(bytes)?;
    let mut fields = BTreeMap::<String, String>::new();
    for (count, pair) in text.trim().trim_start_matches('?').split('&').enumerate() {
        if count >= 8192 {
            return Err(invalid("Origin manifest field limit"));
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = form_piece(key)?.to_ascii_lowercase();
        let value = form_piece(value)?;
        if !value.is_empty() {
            if fields.get(&key).is_some_and(|old| old != &value) {
                return Err(invalid("conflicting Origin manifest field"));
            }
            fields.insert(key, value);
        }
    }
    let offer = fields
        .get("id")
        .or_else(|| fields.get("offerid"))
        .filter(|s| s.len() <= 1024)
        .cloned()
        .ok_or_else(|| invalid("Origin manifest has no offer id"))?;
    let install_dir = fields
        .get("dipinstallpath")
        .or_else(|| fields.get("installpath"))
        .cloned()
        .ok_or_else(|| invalid("Origin manifest has no install path"))?;
    let state = match fields.get("currentstate").map(String::as_str) {
        Some("kReadyToStart" | "kCompleted" | "kInstalled") => InstallState::Installed,
        Some("kDownloading" | "kUpdating" | "kResuming") => InstallState::Updating,
        Some("kPaused" | "kInitializing" | "kInstalling") => InstallState::Partial,
        _ => return Err(invalid("unsupported Origin install state")),
    };
    Ok(OriginManifest {
        offer,
        install_dir,
        title: fields
            .get("displayname")
            .or_else(|| fields.get("title"))
            .cloned(),
        state,
        size: fields
            .get("totalbytes")
            .and_then(|v| v.parse().ok())
            .filter(|n| *n > 0),
        addon: fields.get("isdlc").is_some_and(|v| v == "1")
            || fields
                .get("contenttype")
                .is_some_and(|v| v.eq_ignore_ascii_case("addon")),
    })
}
