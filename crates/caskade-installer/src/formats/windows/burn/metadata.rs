use super::{BurnError, Result, invalid};
use crate::formats::archive::PayloadReader;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom},
};
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Span {
    pub offset: u64,
    pub length: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    pub source: String,
    pub path: String,
    pub size: u64,
    pub sha1: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnMetadata {
    pub ux: Span,
    pub attached: Span,
    pub payloads: Vec<Payload>,
}
fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        data.get(offset..offset + 4)
            .ok_or_else(|| invalid("truncated Burn header"))?
            .try_into()
            .unwrap(),
    ))
}
fn span(offset: u64, length: u64, size: u64) -> Result<Span> {
    if length == 0 || offset.checked_add(length).is_none_or(|end| end > size) {
        return Err(invalid("Burn container outside input"));
    }
    Ok(Span { offset, length })
}
pub fn inspect(reader: &mut (impl Read + Seek)) -> Result<BurnMetadata> {
    let size = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut dos = [0_u8; 64];
    reader.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err(invalid("not PE"));
    }
    let offset = u64::from(u32_at(&dos, 60)?);
    if offset
        .checked_add(24)
        .is_none_or(|end| end > size || end > 128 * 1024)
    {
        return Err(BurnError::Limit);
    }
    reader.seek(SeekFrom::Start(offset))?;
    let mut coff = [0_u8; 24];
    reader.read_exact(&mut coff)?;
    if &coff[..4] != b"PE\0\0" {
        return Err(invalid("PE signature"));
    }
    let sections = u16::from_le_bytes(coff[6..8].try_into().unwrap()) as u64;
    let optional = u16::from_le_bytes(coff[20..22].try_into().unwrap()) as u64;
    let length = offset + 24 + optional + sections * 40;
    if sections > 96 || length > size || length > 128 * 1024 {
        return Err(BurnError::Limit);
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut header = vec![0; length as usize];
    reader.read_exact(&mut header)?;
    if optional < 64 {
        return Err(invalid("PE optional header"));
    }
    let declared = u64::from(u32_at(&header, offset as usize + 24 + 60)?);
    if declared > size || declared > 128 * 1024 {
        return Err(BurnError::Limit);
    }
    if declared > length {
        header.resize(declared as usize, 0);
        reader.read_exact(&mut header[length as usize..])?;
    }
    let pe = crate::probe::parse_pe(&header).ok_or_else(|| invalid("PE headers"))?;
    let mut sections = pe
        .section_headers()
        .iter()
        .filter(|section| section.Name == *b".wixburn");
    let section = sections
        .next()
        .ok_or_else(|| invalid("no .wixburn section"))?;
    if sections.next().is_some() || section.SizeOfRawData < 56 {
        return Err(invalid("ambiguous Burn section"));
    }
    let pe_end = pe.section_headers().iter().try_fold(0u64, |end, section| {
        let next = u64::from(section.PointerToRawData) + u64::from(section.SizeOfRawData);
        if next > size {
            Err(invalid("PE section outside file"))
        } else {
            Ok(end.max(next))
        }
    })?;
    let offset = u64::from(section.PointerToRawData);
    span(offset, 56, size)?;
    reader.seek(SeekFrom::Start(offset))?;
    let mut burn = [0; 56];
    reader.read_exact(&mut burn)?;
    if u32_at(&burn, 0)? != 0x00f14300
        || u32_at(&burn, 4)? != 2
        || u32_at(&burn, 40)? != 1
        || u32_at(&burn, 44)? != 2
    {
        return Err(invalid("requires Burn v2 with one attached CAB"));
    }
    let stub = u64::from(u32_at(&burn, 24)?);
    let ux_size = u64::from(u32_at(&burn, 48)?);
    if stub < pe_end {
        return Err(invalid("Burn stub overlaps PE sections"));
    }
    let ux = span(stub, ux_size, size)?;
    let signature = u64::from(u32_at(&burn, 32)?);
    let signature_size = u64::from(u32_at(&burn, 36)?);
    let engine = if signature > 0 {
        let end = signature
            .checked_add(signature_size)
            .ok_or(BurnError::Limit)?;
        if signature < stub + ux_size {
            return Err(invalid("Burn signature overlaps UX"));
        }
        end
    } else {
        stub + ux_size
    };
    let attached = span(engine, u64::from(u32_at(&burn, 52)?), size)?;
    let mut cab = cab::Cabinet::new(
        PayloadReader::new(&mut *reader, ux.offset, ux.length)
            .map_err(|e| invalid(&e.to_string()))?,
    )?;
    let entry = cab
        .get_file_entry("0")
        .ok_or_else(|| invalid("Burn manifest missing"))?;
    if entry.uncompressed_size() > 2 * 1024 * 1024 {
        return Err(BurnError::Limit);
    }
    let mut xml = Vec::new();
    cab.read_file("0")?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut xml)?;
    if xml.len() > 2 * 1024 * 1024 {
        return Err(BurnError::Limit);
    }
    let payloads = parse_manifest(&xml)?;
    let total = payloads.iter().try_fold(0_u64, |total, payload| {
        total.checked_add(payload.size).ok_or(BurnError::Limit)
    })?;
    // Conservative preflight before materializing attached members. Extraction
    // applies the caller's stricter SafeDir budget as well.
    if total > crate::safety::Limits::default().max_total_bytes {
        return Err(BurnError::Limit);
    }
    Ok(BurnMetadata {
        ux,
        attached,
        payloads,
    })
}
/// Bounded XML metadata entry point for robustness tests. Never fetches entities.
pub fn parse_manifest(bytes: &[u8]) -> Result<Vec<Payload>> {
    use quick_xml::events::Event;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(BurnError::Limit);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid("Burn XML encoding"))?
        .trim_start_matches('\u{feff}');
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack = Vec::new();
    let mut payloads = Vec::new();
    let mut paths = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut count = 0;
    let mut root = false;
    loop {
        count += 1;
        if count > 100_000 {
            return Err(BurnError::Limit);
        }
        let event = reader.read_event().map_err(|_| invalid("Burn XML"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.name().as_ref().to_owned();
                if stack.is_empty() {
                    if root || name != "BurnManifest" {
                        return Err(invalid("Burn manifest root"));
                    }
                    root = true;
                }
                if name == "Payload" && stack.as_slice() == ["BurnManifest"] {
                    let mut values = BTreeMap::new();
                    for attribute in element.attributes().with_checks(true) {
                        let attribute = attribute.map_err(|_| invalid("Burn attribute"))?;
                        if values.len() >= 32 || attribute.value.len() > 4096 {
                            return Err(BurnError::Limit);
                        }
                        values.insert(
                            attribute.key.as_ref().to_owned(),
                            quick_xml::escape::unescape(attribute.value.as_ref())
                                .map_err(|_| invalid("Burn XML entity"))?
                                .into_owned(),
                        );
                    }
                    let mut required = |key: &str| {
                        values
                            .remove(key)
                            .ok_or_else(|| invalid("Burn payload attribute"))
                    };
                    if required("Packaging")? != "embedded" {
                        return Err(invalid("external Burn payload needs native installer"));
                    }
                    let source = required("SourcePath")?;
                    let path = required("FilePath")?;
                    let size = required("FileSize")?
                        .parse()
                        .map_err(|_| invalid("Burn payload size"))?;
                    let sha1 = required("Hash")?;
                    let source_safe = crate::safety::sanitize_path(&source)?;
                    let path_safe = crate::safety::sanitize_path(&path)?;
                    if source_safe.components().count() != 1
                        || path_safe.to_string_lossy().replace('\\', "/") != path.replace('\\', "/")
                        || !sources.insert(source.clone())
                        || !paths.insert(path.to_lowercase())
                        || sha1.len() != 40
                        || !sha1.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err(invalid("Burn payload identity"));
                    }
                    payloads.push(Payload {
                        source,
                        path,
                        size,
                        sha1,
                    });
                    if payloads.len() > 1024 {
                        return Err(BurnError::Limit);
                    }
                }
                if !empty {
                    stack.push(name);
                    if stack.len() > 64 {
                        return Err(BurnError::Limit);
                    }
                }
            }
            Event::End(_) => {
                stack.pop().ok_or_else(|| invalid("Burn XML depth"))?;
            }
            Event::DocType(_) | Event::GeneralRef(_) => {
                return Err(invalid("Burn XML entities forbidden"));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root || !stack.is_empty() || payloads.is_empty() {
        return Err(invalid("Burn payload manifest empty"));
    }
    Ok(payloads)
}
