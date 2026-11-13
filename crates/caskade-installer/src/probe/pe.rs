//! Bounded static PE inspection. Resources and overlay samples are read by
//! offset; neither SizeOfImage nor a large SFX overlay is allocated or executed.
use super::{Detection, FormatProbe, ProbeError, ProbeSource};
use crate::{ArtifactNode, Format, PayloadPlatform, Role};
use pelite::{
    PeFile,
    image::IMAGE_DATA_DIRECTORY,
    resources::{Name, Resources},
};
use quick_xml::{Reader, events::Event};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Type)]
pub struct PeProbeLimits {
    pub max_header_bytes: u64,
    pub max_resource_bytes: u64,
    pub max_signature_bytes: u64,
}
impl Default for PeProbeLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 128 * 1024,
            max_resource_bytes: 2 * 1024 * 1024,
            max_signature_bytes: 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct PeMetadata {
    pub machine: u16,
    pub architecture: String,
    pub subsystem: u16,
    pub sections: Vec<String>,
    pub resource_types: Vec<String>,
    pub requested_execution_level: Option<String>,
    /// Deterministic language-tagged keys: 040904b0/ProductName, for example.
    pub version_info: BTreeMap<String, String>,
    pub overlay_offset: u64,
    pub overlay_size: u64,
    pub certificate_bytes: u64,
    pub installer_format: Option<Format>,
    pub sfx_format: Option<String>,
    pub signature_scan_truncated: bool,
    pub warnings: Vec<String>,
}
#[derive(Debug, Default)]
pub struct PeProbe {
    limits: PeProbeLimits,
}
impl PeProbe {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_limits(limits: PeProbeLimits) -> Self {
        Self { limits }
    }
    fn inspect(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> Result<Option<Detection>, ProbeError> {
        if source.size() < 64 {
            return Ok(None);
        }
        let dos = source.read_at(0, 64)?;
        if !dos.starts_with(b"MZ") {
            return Ok(None);
        }
        let offset = u32_at(&dos, 60) as u64;
        if offset
            .checked_add(24)
            .is_none_or(|end| end > source.size() || end > self.limits.max_header_bytes)
        {
            return Ok(None);
        }
        let coff = source.read_at(offset, 24)?;
        if !coff.starts_with(b"PE\0\0") {
            return Ok(None);
        }
        let optional_size = u16_at(&coff, 20) as u64;
        let sections = u16_at(&coff, 6) as u64;
        if !(96..=4096).contains(&optional_size) || sections > 96 {
            return Ok(None);
        }
        let end = offset + 24 + optional_size + sections * 40;
        if end > self.limits.max_header_bytes || end > source.size() {
            return Ok(None);
        }
        let optional = source.read_at(offset + 24, optional_size as usize)?;
        let header_size = u32_at(&optional, 60) as u64;
        let end = end.max(header_size);
        if end > self.limits.max_header_bytes || end > source.size() {
            return Ok(None);
        }
        // Pelite validates both PE32 and PE32+ headers without reading sections.
        let headers = source.read_at(0, end as usize)?;
        let Some(pe) = parse_pe(&headers) else {
            return Ok(None);
        };
        let machine = pe.file_header().Machine;
        let architecture = match machine {
            0x14c => "x86",
            0x8664 => "x86_64",
            0x1c0 | 0x1c4 => "arm",
            0xaa64 => "aarch64",
            _ => "unknown",
        }
        .into();
        let mut meta = PeMetadata {
            machine,
            architecture,
            subsystem: u16_at(&optional, 68),
            sections: vec![],
            resource_types: vec![],
            requested_execution_level: None,
            version_info: BTreeMap::new(),
            overlay_offset: end,
            overlay_size: 0,
            certificate_bytes: 0,
            installer_format: None,
            sfx_format: None,
            signature_scan_truncated: false,
            warnings: vec![],
        };
        for section in pe.section_headers().iter() {
            let length = section.Name.iter().position(|b| *b == 0).unwrap_or(8);
            meta.sections
                .push(String::from_utf8_lossy(&section.Name[..length]).into_owned());
            let raw_end = u64::from(section.PointerToRawData) + u64::from(section.SizeOfRawData);
            if raw_end <= source.size() {
                meta.overlay_offset = meta.overlay_offset.max(raw_end);
            } else {
                meta.warnings.push("section extends beyond source".into());
            }
        }
        // Authenticode is file-offset based, unlike other PE data directories.
        // Skip a certificate table when it immediately follows the image; a
        // certificate alone must never turn an application into an installer.
        if let Some(cert) = pe.data_directory().get(4) {
            let cert_start = u64::from(cert.VirtualAddress);
            let cert_end = cert_start + u64::from(cert.Size);
            if cert.Size != 0 && cert_end <= source.size() {
                meta.certificate_bytes = u64::from(cert.Size);
                if cert_start <= meta.overlay_offset.saturating_add(7)
                    && cert_end >= meta.overlay_offset
                {
                    meta.overlay_offset = cert_end;
                }
            }
        }
        let mut resource_bytes = vec![];
        if let Some(resource) = pe
            .data_directory()
            .get(2)
            .filter(|r| r.VirtualAddress != 0 && r.Size != 0)
        {
            if u64::from(resource.Size) > self.limits.max_resource_bytes {
                meta.warnings
                    .push("resource directory exceeds probe limit".into());
            } else if let Some(section) = pe.section_headers().iter().find(|s| {
                resource.VirtualAddress >= s.VirtualAddress
                    && u64::from(resource.VirtualAddress)
                        < u64::from(s.VirtualAddress) + u64::from(s.SizeOfRawData)
            }) {
                let delta = u64::from(resource.VirtualAddress - section.VirtualAddress);
                let file_offset = u64::from(section.PointerToRawData) + delta;
                let length = u64::from(resource.Size);
                if length <= u64::from(section.SizeOfRawData).saturating_sub(delta)
                    && file_offset + length <= source.size()
                {
                    resource_bytes = source.read_at(file_offset, length as usize)?;
                    read_resources(&resource_bytes, resource, &mut meta);
                } else {
                    meta.warnings
                        .push("resource directory is out of bounds".into());
                }
            } else {
                meta.warnings.push("resource RVA has no raw section".into());
            }
        }
        meta.overlay_size = source.size().saturating_sub(meta.overlay_offset);
        let sample_size = meta.overlay_size.min(self.limits.max_signature_bytes);
        let overlay = source.read_at(meta.overlay_offset, sample_size as usize)?;
        let tail_size = meta.overlay_size.saturating_sub(sample_size).min(64 * 1024);
        let tail = source.read_at(source.size() - tail_size, tail_size as usize)?;
        meta.signature_scan_truncated = meta.overlay_size > sample_size + tail_size;
        let resource_text: Vec<u8> = resource_bytes.iter().copied().filter(|b| *b != 0).collect();
        let samples = [
            resource_bytes.as_slice(),
            resource_text.as_slice(),
            overlay.as_slice(),
            tail.as_slice(),
        ];
        meta.installer_format = if meta
            .sections
            .iter()
            .any(|s| s.eq_ignore_ascii_case(".wixburn"))
        {
            Some(Format::Burn)
        } else {
            signature_family(&samples)
        };
        meta.sfx_format = if has(&samples, b"7z\xbc\xaf\x27\x1c") {
            Some("seven_zip".into())
        } else if has(&samples, b"Rar!\x1a\x07") {
            Some("rar".into())
        } else if has(&samples, b"PK\x03\x04") || has(&samples, b"PK\x05\x06") {
            Some("zip".into())
        } else {
            None
        };
        let role = role(name, &meta);
        let format = meta.installer_format.unwrap_or(Format::Pe);
        let mut detection = Detection::new(
            format,
            PayloadPlatform::Windows,
            meta.installer_format.is_some() || meta.sfx_format.is_some(),
        );
        detection.role = Some(role);
        detection
            .facts
            .insert("pe.arch".into(), meta.architecture.clone().into());
        detection
            .facts
            .insert("pe.subsystem".into(), meta.subsystem.into());
        detection
            .facts
            .insert("pe.overlay_offset".into(), meta.overlay_offset.into());
        if let Some(sfx) = &meta.sfx_format {
            detection
                .facts
                .insert("pe.sfx_format".into(), sfx.clone().into());
        }
        if let Some(level) = &meta.requested_execution_level {
            detection
                .facts
                .insert("pe.requested_execution_level".into(), level.clone().into());
        }
        detection.facts.insert("pe.metadata".into(), json!(meta));
        Ok(Some(detection))
    }
}
impl FormatProbe for PeProbe {
    fn id(&self) -> &str {
        "pe"
    }
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> Result<Option<Detection>, ProbeError> {
        self.inspect(source, name)
    }
    fn list(
        &self,
        source: &mut ProbeSource<'_>,
        node: &ArtifactNode,
    ) -> Result<Option<Vec<ArtifactNode>>, ProbeError> {
        // ZIP SFXs can use the existing safe lazy lister. Other embedded engines
        // need their own format handler or the pinned 7zz adapter.
        if node.format == Format::Pe
            && node.facts.get("pe.sfx_format").and_then(|v| v.as_str()) == Some("zip")
        {
            let mut archive = node.clone();
            archive.format = Format::Zip;
            return crate::formats::archive::ArchiveProbe.list(source, &archive);
        }
        Ok(None)
    }
}
/// pelite 0.10 slices the section table at `SizeOfOptionalHeader` without
/// checking alignment (undefined behaviour), so such headers are declined
/// before parsing. Every PE parse in the crate goes through here.
pub(crate) fn parse_pe(bytes: &[u8]) -> Option<PeFile<'_>> {
    let offset = u32_at(bytes, 60) as usize;
    let optional_size = u16_at(bytes, offset.checked_add(20)?);
    if !optional_size.is_multiple_of(4) {
        return None;
    }
    PeFile::from_bytes(bytes).ok()
}
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    bytes
        .get(offset..offset + 2)
        .and_then(|b| b.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    bytes
        .get(offset..offset + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}
fn has(samples: &[&[u8]], needle: &[u8]) -> bool {
    samples
        .iter()
        .any(|sample| sample.windows(needle.len()).any(|bytes| bytes == needle))
}
fn signature_family(samples: &[&[u8]]) -> Option<Format> {
    [
        (Format::Inno, b"Inno Setup Setup Data (".as_slice()),
        (Format::Nsis, b"\xef\xbe\xad\xdeNullsoftInst"),
        (Format::InstallShield, b"InstallShield"),
        (Format::Wise, b"Wise Installation"),
        (Format::SetupFactory, b"Setup Factory"),
    ]
    .into_iter()
    .find(|(_, needle)| has(samples, needle))
    .map(|(format, _)| format)
}
fn role(name: &Path, meta: &PeMetadata) -> Role {
    let name = name
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let descriptions = meta
        .version_info
        .iter()
        .filter(|(k, _)| k.ends_with("/FileDescription") || k.ends_with("/ProductName"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if name.starts_with("unins")
        || name.contains("uninstall")
        || descriptions.contains("uninstaller")
    {
        Role::Uninstaller
    } else if [
        "vcredist",
        "vc_redist",
        "dxsetup",
        "directx",
        "dotnetfx",
        "oalinst",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
    {
        Role::Redistributable
    } else if meta.installer_format.is_some() || meta.sfx_format.is_some() {
        Role::Installer
    } else if name.contains("launcher") || descriptions.contains("launcher") {
        Role::Launcher
    } else {
        Role::Application
    }
}
fn read_resources(bytes: &[u8], directory: &IMAGE_DATA_DIRECTORY, meta: &mut PeMetadata) {
    let resources = Resources::new(bytes, directory);
    if let Ok(root) = resources.root() {
        for entry in root.entries().take(128) {
            if let Ok(name) = entry.name() {
                meta.resource_types.push(format!("{name:?}"));
            }
        }
    } else {
        meta.warnings.push("malformed resource directory".into());
        return;
    }
    if let Ok(version) = resources.version_info() {
        for (language, strings) in version.file_info().strings {
            // Builders such as Inno pad fixed-width values with spaces/NULs.
            for (key, value) in strings {
                let value = value.trim_matches(|c: char| c.is_whitespace() || c == '\0');
                if !value.is_empty() {
                    meta.version_info.insert(
                        format!("{:04x}{:04x}/{key}", language.lang_id, language.charset_id),
                        value.to_owned(),
                    );
                }
            }
        }
    }
    if let Some(manifest) = resources
        .root()
        .ok()
        .and_then(|r| r.get_dir(Name::MANIFEST).ok())
        .and_then(|r| r.first_dir().ok())
        .and_then(|r| r.first_data().ok())
        .and_then(|r| r.bytes().ok())
    {
        match execution_level(manifest) {
            Ok(level) => meta.requested_execution_level = level,
            Err(()) => meta.warnings.push("malformed application manifest".into()),
        }
    }
}
fn execution_level(bytes: &[u8]) -> Result<Option<String>, ()> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE.decode(&bytes[2..]).0.into_owned()
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        encoding_rs::UTF_16BE.decode(&bytes[2..]).0.into_owned()
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| ())?
            .trim_start_matches('\u{feff}')
            .into()
    };
    let mut reader = Reader::from_str(&text);
    let mut result = None;
    let mut depth = 0_u32;
    let mut root_seen = false;
    for _ in 0..10_000 {
        let event = reader.read_event().map_err(|_| ())?;
        match &event {
            Event::Start(_) => {
                depth += 1;
                root_seen = true;
                if depth > 64 {
                    return Err(());
                }
            }
            Event::Empty(_) => {
                root_seen = true;
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(())?;
            }
            _ => {}
        }
        match event {
            Event::Start(tag) | Event::Empty(tag)
                if tag.local_name().as_ref() == "requestedExecutionLevel" =>
            {
                for attribute in tag.attributes().take(32) {
                    let attribute = attribute.map_err(|_| ())?;
                    if attribute.key.as_ref() == "level" {
                        let value = attribute
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|_| ())?;
                        if !["asInvoker", "highestAvailable", "requireAdministrator"]
                            .contains(&value.as_ref())
                            || result.is_some()
                        {
                            return Err(());
                        }
                        result = Some(value.into_owned());
                    }
                }
            }
            Event::DocType(_) => return Err(()),
            Event::Eof => {
                return if depth == 0 && root_seen {
                    Ok(result)
                } else {
                    Err(())
                };
            }
            _ => {}
        }
    }
    Err(())
}
