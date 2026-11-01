use super::{PkgError, Result, digest::Checksum, invalid};
use crate::safety::sanitize_path;
use quick_xml::events::{BytesStart, Event};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone)]
pub struct XarEntry {
    pub path: PathBuf,
    pub directory: bool,
    pub offset: u64,
    pub length: u64,
    pub size: u64,
    pub encoding: String,
    pub(super) archived: Option<Checksum>,
    pub(super) extracted: Option<Checksum>,
}
#[derive(Debug, Clone)]
pub struct Toc {
    pub entries: Vec<XarEntry>,
    pub(super) checksum: Option<(String, u64, u64)>,
}
#[derive(Default)]
struct RawFile {
    name: Option<String>,
    kind: Option<String>,
    data: RawData,
    children: Vec<RawFile>,
    depth: usize,
}
#[derive(Default)]
struct RawData {
    offset: Option<u64>,
    length: Option<u64>,
    size: Option<u64>,
    encoding: Option<String>,
    archived_style: Option<String>,
    archived: Option<String>,
    extracted_style: Option<String>,
    extracted: Option<String>,
}
fn set<T>(slot: &mut Option<T>, value: T) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(invalid("duplicate XAR metadata field"));
    }
    Ok(())
}
fn attrs(element: &BytesStart<'_>) -> Result<Vec<(String, String)>> {
    let mut values = Vec::new();
    for attribute in element.attributes().with_checks(true) {
        if values.len() >= 64 {
            return Err(PkgError::Limit);
        }
        let attribute = attribute.map_err(|_| invalid("XML attributes"))?;
        let key = attribute.key.as_ref().to_owned();
        let text = attribute.value.as_ref();
        let value = quick_xml::escape::unescape(text)
            .map_err(|_| invalid("XML entity"))?
            .into_owned();
        if key.len() > 128 || value.len() > 4096 {
            return Err(PkgError::Limit);
        }
        values.push((key, value));
    }
    Ok(values)
}
struct Parser {
    elements: Vec<String>,
    texts: Vec<String>,
    files: Vec<RawFile>,
    top: Vec<RawFile>,
    checksum_style: Option<String>,
    checksum_offset: Option<u64>,
    checksum_size: Option<u64>,
    count: usize,
    toc: bool,
    closed: bool,
}
impl Parser {
    fn start(&mut self, element: &BytesStart<'_>) -> Result<()> {
        let name = element.name().as_ref().to_owned();
        if name.len() > 128 || self.elements.len() >= 64 {
            return Err(PkgError::Limit);
        }
        if self.elements.is_empty() && (name != "xar" || self.closed) {
            return Err(invalid("XAR XML root"));
        }
        let attributes = attrs(element)?;
        if name == "toc" {
            if self.elements.as_slice() != ["xar"] || self.toc {
                return Err(invalid("ambiguous XAR TOC"));
            }
            self.toc = true;
        }
        if name == "file" {
            if !self.toc
                || !matches!(
                    self.elements.last().map(String::as_str),
                    Some("toc" | "file")
                )
            {
                return Err(invalid("unexpected XAR file"));
            }
            self.count += 1;
            if self.count > 100_000 {
                return Err(PkgError::Limit);
            }
            self.files.push(RawFile {
                depth: self.elements.len() + 1,
                ..RawFile::default()
            });
        }
        self.elements.push(name);
        self.texts.push(String::new());
        if let Some(file) = self.files.last_mut() {
            let path = self.elements[file.depth..].join("/");
            let style = attributes
                .iter()
                .find(|(key, _)| key == "style")
                .map(|(_, value)| value.clone());
            if let Some(style) = style {
                match path.as_str() {
                    "data/encoding" => set(&mut file.data.encoding, style)?,
                    "data/archived-checksum" => set(&mut file.data.archived_style, style)?,
                    "data/extracted-checksum" => set(&mut file.data.extracted_style, style)?,
                    _ => (),
                }
            }
        } else if self.elements.as_slice() == ["xar", "toc", "checksum"] {
            let style = attributes
                .iter()
                .find(|(key, _)| key == "style")
                .ok_or_else(|| invalid("TOC checksum style"))?
                .1
                .clone();
            set(&mut self.checksum_style, style)?;
        }
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<()> {
        if let Some(current) = self.texts.last_mut() {
            if current.len() + text.len() > 4096 {
                return Err(PkgError::Limit);
            }
            current.push_str(text);
        } else if !text.trim().is_empty() {
            return Err(invalid("text outside XAR root"));
        }
        Ok(())
    }
    fn value(&mut self, text: &str) -> Result<()> {
        let value = text.trim();
        if value.is_empty() {
            return Ok(());
        }
        if value.len() > 4096 {
            return Err(PkgError::Limit);
        }
        if let Some(file) = self.files.last_mut() {
            let path = self.elements[file.depth..].join("/");
            match path.as_str() {
                "name" => set(&mut file.name, text.into())?,
                "type" => set(&mut file.kind, value.into())?,
                "data/offset" => set(
                    &mut file.data.offset,
                    value.parse().map_err(|_| invalid("XAR offset"))?,
                )?,
                "data/length" => set(
                    &mut file.data.length,
                    value.parse().map_err(|_| invalid("XAR length"))?,
                )?,
                "data/size" => set(
                    &mut file.data.size,
                    value.parse().map_err(|_| invalid("XAR size"))?,
                )?,
                "data/archived-checksum" => set(&mut file.data.archived, value.into())?,
                "data/extracted-checksum" => set(&mut file.data.extracted, value.into())?,
                _ => (),
            }
        } else {
            match self
                .elements
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["xar", "toc", "checksum", "offset"] => set(
                    &mut self.checksum_offset,
                    value.parse().map_err(|_| invalid("TOC checksum offset"))?,
                )?,
                ["xar", "toc", "checksum", "size"] => set(
                    &mut self.checksum_size,
                    value.parse().map_err(|_| invalid("TOC checksum size"))?,
                )?,
                _ => (),
            }
        }
        Ok(())
    }
    fn end(&mut self) -> Result<()> {
        let text = self
            .texts
            .pop()
            .ok_or_else(|| invalid("unbalanced XAR XML"))?;
        self.value(&text)?;
        let name = self
            .elements
            .pop()
            .ok_or_else(|| invalid("unbalanced XAR XML"))?;
        if name == "file" {
            let file = self
                .files
                .pop()
                .ok_or_else(|| invalid("unbalanced XAR file"))?;
            if let Some(parent) = self.files.last_mut() {
                parent.children.push(file);
            } else {
                self.top.push(file);
            }
        }
        if self.elements.is_empty() {
            self.closed = true;
        }
        Ok(())
    }
}
fn checksum(style: Option<String>, text: Option<String>) -> Result<Option<Checksum>> {
    match (style, text) {
        (Some(style), Some(text)) => Ok(Some(Checksum::parse(&style, &text)?)),
        (None, None) => Ok(None),
        _ => Err(invalid("incomplete member checksum")),
    }
}
fn flatten(
    files: Vec<RawFile>,
    parent: &Path,
    entries: &mut Vec<XarEntry>,
    paths: &mut BTreeSet<PathBuf>,
    metadata: &mut usize,
    heap: u64,
) -> Result<()> {
    for file in files {
        let name = file
            .name
            .ok_or_else(|| invalid("missing XAR member name"))?;
        let name = sanitize_path(&name)?;
        if name.components().count() != 1 {
            return Err(invalid("XAR name is not a path component"));
        }
        let path = parent.join(name);
        *metadata = metadata
            .checked_add(path.as_os_str().len())
            .ok_or(PkgError::Limit)?;
        if *metadata > 16 * 1024 * 1024 {
            return Err(PkgError::Limit);
        }
        if !paths.insert(path.clone()) {
            return Err(invalid("duplicate XAR path"));
        }
        let directory = match file.kind.as_deref() {
            Some("directory") => true,
            Some("file") => false,
            _ => return Err(PkgError::Unsupported("XAR link or special member".into())),
        };
        let data = file.data;
        if directory {
            if data.offset.is_some() || data.length.is_some() || data.size.is_some() {
                return Err(invalid("data in XAR directory"));
            }
            entries.push(XarEntry {
                path: path.clone(),
                directory: true,
                offset: 0,
                length: 0,
                size: 0,
                encoding: String::new(),
                archived: None,
                extracted: None,
            });
        } else {
            if !file.children.is_empty() {
                return Err(invalid("XAR file contains children"));
            }
            let offset = data.offset.ok_or_else(|| invalid("missing heap offset"))?;
            let length = data.length.ok_or_else(|| invalid("missing heap length"))?;
            let size = data.size.ok_or_else(|| invalid("missing member size"))?;
            if offset.checked_add(length).is_none_or(|end| end > heap) {
                return Err(invalid("XAR member outside heap"));
            }
            entries.push(XarEntry {
                path: path.clone(),
                directory: false,
                offset,
                length,
                size,
                encoding: data
                    .encoding
                    .unwrap_or_else(|| "application/octet-stream".into()),
                archived: checksum(data.archived_style, data.archived)?,
                extracted: checksum(data.extracted_style, data.extracted)?,
            });
        }
        flatten(file.children, &path, entries, paths, metadata, heap)?;
    }
    Ok(())
}
/// Parse bounded UTF-8 XAR TOC XML. Public for the dedicated robustness target.
pub fn parse_toc(bytes: &[u8], heap_size: u64) -> Result<Toc> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(PkgError::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("TOC encoding must be UTF-8"))?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut parser = Parser {
        elements: vec![],
        texts: vec![],
        files: vec![],
        top: vec![],
        checksum_style: None,
        checksum_offset: None,
        checksum_size: None,
        count: 0,
        toc: false,
        closed: false,
    };
    let mut events = 0;
    loop {
        events += 1;
        if events > 1_000_000 {
            return Err(PkgError::Limit);
        }
        match reader.read_event().map_err(|_| invalid("TOC XML"))? {
            Event::Start(element) => parser.start(&element)?,
            Event::Empty(element) => {
                parser.start(&element)?;
                parser.end()?;
            }
            Event::End(_) => parser.end()?,
            Event::Text(text) => parser.text(text.as_ref())?,
            Event::CData(text) => parser.text(text.as_ref())?,
            Event::GeneralRef(reference) => {
                let raw = format!("&{};", reference.as_ref());
                let value = quick_xml::escape::unescape(&raw).map_err(|_| invalid("XML entity"))?;
                parser.text(&value)?;
            }
            Event::DocType(_) => return Err(invalid("TOC DTD is unsupported")),
            Event::Eof => break,
            _ => (),
        }
    }
    if !parser.closed || !parser.toc || !parser.elements.is_empty() || !parser.files.is_empty() {
        return Err(invalid("incomplete TOC"));
    }
    let checksum = match (
        parser.checksum_style,
        parser.checksum_offset,
        parser.checksum_size,
    ) {
        (Some(style), Some(offset), Some(size)) => {
            if size > 64 || offset.checked_add(size).is_none_or(|end| end > heap_size) {
                return Err(invalid("TOC checksum span"));
            }
            Some((style, offset, size))
        }
        (None, None, None) => None,
        _ => return Err(invalid("incomplete TOC checksum")),
    };
    let mut entries = Vec::new();
    flatten(
        parser.top,
        Path::new(""),
        &mut entries,
        &mut BTreeSet::new(),
        &mut 0,
        heap_size,
    )?;
    let mut spans: Vec<_> = entries
        .iter()
        .filter(|entry| !entry.directory && entry.length != 0)
        .map(|entry| (entry.offset, entry.length))
        .collect();
    if let Some((_, offset, length)) = &checksum
        && *length != 0
    {
        spans.push((*offset, *length));
    }
    spans.sort_unstable();
    for pair in spans.windows(2) {
        if pair[0].0 + pair[0].1 > pair[1].0 {
            return Err(invalid("overlapping XAR heap members"));
        }
    }
    Ok(Toc { entries, checksum })
}
