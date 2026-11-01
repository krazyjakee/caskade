use super::{ArchiveOptions, Entry, EntryKind};
use crate::{
    ArtifactId, ArtifactNode, Format, PayloadPlatform, Role,
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::io::{Seek, SeekFrom};

#[derive(Default)]
pub struct ArchiveProbe;
impl ArchiveProbe {
    pub fn new() -> Self {
        Self
    }
}
impl FormatProbe for ArchiveProbe {
    fn id(&self) -> &str {
        "archive"
    }
    fn detect(&self, source: &mut ProbeSource<'_>) -> Result<Option<Detection>, ProbeError> {
        // ZIP central records also identify prepended/SFX ZIPs. Do not parse
        // their metadata or decompress any payload during initial probing.
        if source.size() < 22 {
            return Ok(None);
        }
        let header = source.read_at(0, 6)?;
        if header.starts_with(b"PK\x03\x04")
            || header.starts_with(b"PK\x05\x06")
            || header.starts_with(b"PK\x07\x08")
        {
            return Ok(Some(Detection::new(
                Format::Zip,
                PayloadPlatform::Unknown,
                true,
            )));
        }
        // Avoid scanning a trailer for other known archives (or every extracted
        // ELF/PE file). Only ZIP candidates require this bounded fallback scan.
        if header.starts_with(b"7z\xbc\xaf\x27\x1c")
            || header.starts_with(b"\x1f\x8b")
            || header.starts_with(b"\xfd7zXZ")
            || header.starts_with(b"BZh")
            || header.starts_with(b"\x28\xb5\x2f\xfd")
        {
            return Ok(None);
        }
        let length = source.size().min(65_557);
        let tail = source.read_at(source.size() - length, length as usize)?;
        if (0..tail.len().saturating_sub(21)).rev().any(|i| {
            tail.get(i..i + 4) == Some(b"PK\x05\x06")
                && i + 22 + u16::from_le_bytes([tail[i + 20], tail[i + 21]]) as usize == tail.len()
        }) {
            return Ok(Some(Detection::new(
                Format::Zip,
                PayloadPlatform::Unknown,
                true,
            )));
        }
        Ok(None)
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> Result<Option<Vec<ArtifactNode>>, ProbeError> {
        if !super::supported(node.format) {
            return Ok(None);
        }
        source.seek(SeekFrom::Start(0))?;
        let entries = super::list(source, node.format, &ArchiveOptions::default())
            .map_err(|error| ProbeError::Io(std::io::Error::other(error)))?;
        Ok(Some(
            entries
                .into_iter()
                .enumerate()
                .map(|(index, entry)| child(node, entry, index))
                .collect(),
        ))
    }
}
fn child(parent: &ArtifactNode, entry: Entry, index: usize) -> ArtifactNode {
    let mut facts = std::collections::BTreeMap::new();
    facts.insert(
        "archive.entry_kind".into(),
        serde_json::to_value(entry.kind).unwrap(),
    );
    facts.insert("archive.mode".into(), entry.mode.into());
    if let Some(target) = entry.target {
        facts.insert("archive.link_target".into(), target.into());
    }
    ArtifactNode {
        id: ArtifactId(format!("{}:entry:{index}", parent.id.0)),
        path: entry.name.into(),
        format: if entry.kind == EntryKind::Directory {
            Format::Directory
        } else {
            Format::Unknown
        },
        platform: PayloadPlatform::Unknown,
        role: if entry.kind == EntryKind::Directory {
            Role::Unknown
        } else {
            Role::Data
        },
        size: Some(entry.size),
        facts,
        children: vec![],
        children_complete: entry.kind != EntryKind::Directory,
    }
}
