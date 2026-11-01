use super::{DmgError, Result, invalid};
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
pub const MAX_PLIST_BYTES: u64 = 4 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmgMetadata {
    pub data_offset: u64,
    pub data_length: u64,
    pub resource_offset: u64,
    pub resource_length: u64,
    pub xml_offset: u64,
    pub xml_length: u64,
    pub expanded_bytes: u64,
    pub sla: bool,
}
fn be32(raw: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(raw[at..at + 4].try_into().unwrap())
}
fn be64(raw: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(raw[at..at + 8].try_into().unwrap())
}
/// Validate only the UDIF trailer and metadata spans; the OS verifies and mounts
/// filesystem/compression blocks. No image code is executed by this reader.
pub fn parse_trailer(raw: &[u8], file_size: u64) -> Result<DmgMetadata> {
    if raw.len() != 512
        || file_size < 512
        || &raw[..4] != b"koly"
        || be32(raw, 4) != 4
        || be32(raw, 8) != 512
    {
        return Err(invalid("UDIF trailer magic/version/length"));
    }
    let segment = be32(raw, 56);
    let count = be32(raw, 60);
    if count > 1 || segment > 1 {
        return Err(DmgError::Unsupported("segmented image".into()));
    }
    let result = DmgMetadata {
        data_offset: be64(raw, 24),
        data_length: be64(raw, 32),
        resource_offset: be64(raw, 40),
        resource_length: be64(raw, 48),
        xml_offset: be64(raw, 216),
        xml_length: be64(raw, 224),
        expanded_bytes: be64(raw, 492).checked_mul(512).ok_or(DmgError::Limit)?,
        sla: false,
    };
    let end = file_size - 512;
    let mut spans = Vec::new();
    for (offset, length) in [
        (result.data_offset, result.data_length),
        (result.resource_offset, result.resource_length),
        (result.xml_offset, result.xml_length),
    ] {
        if offset.checked_add(length).is_none_or(|n| n > end) {
            return Err(invalid("UDIF span outside image"));
        }
        if length != 0 {
            spans.push((offset, length));
        }
    }
    spans.sort_unstable();
    for pair in spans.windows(2) {
        if pair[0].0 + pair[0].1 > pair[1].0 {
            return Err(invalid("overlapping UDIF spans"));
        }
    }
    if result.xml_length > MAX_PLIST_BYTES {
        return Err(DmgError::Limit);
    }
    if result.xml_length == 0 && result.resource_length == 0 {
        return Err(invalid("image has no resource metadata"));
    }
    Ok(result)
}
/// Bounded XML-only resource inspection for the dedicated robustness target.
/// Data/base64 block maps are never decoded or allocated as filesystem buffers.
pub fn parse_resource_plist(bytes: &[u8]) -> Result<bool> {
    if bytes.len() as u64 > MAX_PLIST_BYTES {
        return Err(DmgError::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("resource plist encoding"))?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut elements = Vec::new();
    let mut text_value = String::new();
    let mut closed = false;
    let mut sla = false;
    let mut events = 0;
    loop {
        events += 1;
        if events > 500_000 {
            return Err(DmgError::Limit);
        }
        let event = reader
            .read_event()
            .map_err(|_| invalid("resource plist XML"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                if elements.len() >= 64 {
                    return Err(DmgError::Limit);
                }
                let name = element.name().as_ref().to_owned();
                if name.len() > 128 {
                    return Err(DmgError::Limit);
                }
                if elements.is_empty() && (name != "plist" || closed) {
                    return Err(invalid("resource plist root"));
                }
                for (count, attribute) in element.attributes().with_checks(true).enumerate() {
                    let attribute = attribute.map_err(|_| invalid("resource plist attribute"))?;
                    if count >= 64
                        || attribute.key.as_ref().len() > 128
                        || attribute.value.len() > 4096
                    {
                        return Err(DmgError::Limit);
                    }
                }
                elements.push(name);
                text_value.clear();
                if empty {
                    elements.pop();
                    if elements.is_empty() {
                        closed = true;
                    }
                }
            }
            Event::End(_) => {
                let name = elements
                    .pop()
                    .ok_or_else(|| invalid("resource plist balance"))?;
                if name == "key" && matches!(text_value.trim(), "LPic" | "STR#" | "SLA") {
                    sla = true;
                }
                text_value.clear();
                if elements.is_empty() {
                    closed = true;
                }
            }
            Event::Text(value) => {
                if elements.last().is_some_and(|n| n == "key") {
                    if text_value.len() + value.as_ref().len() > 256 {
                        return Err(DmgError::Limit);
                    }
                    text_value.push_str(value.as_ref());
                } else if elements.is_empty() && !value.as_ref().trim().is_empty() {
                    return Err(invalid("text outside resource plist"));
                }
            }
            Event::CData(value) => {
                if elements.last().is_some_and(|n| n == "key") {
                    if text_value.len() + value.as_ref().len() > 256 {
                        return Err(DmgError::Limit);
                    }
                    text_value.push_str(value.as_ref());
                } else if elements.is_empty() && !value.as_ref().trim().is_empty() {
                    return Err(invalid("text outside resource plist"));
                }
            }
            Event::GeneralRef(value) => {
                let raw = format!("&{};", value.as_ref());
                let decoded = quick_xml::escape::unescape(&raw)
                    .map_err(|_| invalid("resource plist entity"))?;
                if elements.last().is_some_and(|n| n == "key") {
                    if text_value.len() + decoded.len() > 256 {
                        return Err(DmgError::Limit);
                    }
                    text_value.push_str(&decoded);
                }
            }
            Event::DocType(value) => {
                if value.as_ref().contains('[') {
                    return Err(invalid("resource plist entity declaration"));
                }
            }
            Event::Eof => break,
            _ => (),
        }
    }
    if !closed || !elements.is_empty() {
        return Err(invalid("incomplete resource plist"));
    }
    Ok(sla)
}
pub fn inspect(reader: &mut (impl Read + Seek)) -> Result<DmgMetadata> {
    let size = reader.seek(SeekFrom::End(0))?;
    if size < 512 {
        return Err(invalid("short image"));
    }
    reader.seek(SeekFrom::End(-512))?;
    let mut raw = [0; 512];
    reader.read_exact(&mut raw)?;
    let mut metadata = parse_trailer(&raw, size)?;
    if metadata.xml_length != 0 {
        reader.seek(SeekFrom::Start(metadata.xml_offset))?;
        let mut xml = vec![0; metadata.xml_length as usize];
        reader.read_exact(&mut xml)?;
        metadata.sla = parse_resource_plist(&xml)?;
    }
    // Old resource-fork SLAs are passed to hdiutil unchanged; no native fork
    // parsing or pure-Rust filesystem reader is required for mounting.
    metadata.sla |= metadata.resource_length != 0;
    Ok(metadata)
}
