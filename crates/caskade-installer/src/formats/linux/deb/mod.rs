//! Bounded Debian ar/control parsing and game-data extraction. Package scripts
//! remain data and never run during managed extraction.
pub(super) mod adapter;
pub(super) mod common;
mod desktop;
pub mod installed;
use crate::{
    Format,
    formats::archive::{self, ArchiveOptions, ExtractionControl, PayloadReader},
    safety::{SafeDir, sanitize_path},
};
pub use adapter::{DebHandler, PackageExtractor, PackageProbe};
pub use common::{
    DependencyReport, GeneratedReference, PackageDependency, PackageError, PackageInfo,
    check_dependencies,
};
pub use desktop::{discovery_hints, rewrite_desktop_files};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, SeekFrom},
};

pub type Result<T> = std::result::Result<T, PackageError>;
pub const MAX_METADATA: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub offset: u64,
    pub length: u64,
}
#[derive(Debug, Clone)]
pub struct Deb {
    pub info: PackageInfo,
    pub data: Span,
    pub format: Format,
}
fn compression(name: &str) -> Result<Format> {
    Ok(if name.ends_with(".tar") {
        Format::Tar
    } else if name.ends_with(".tar.gz") {
        Format::Gzip
    } else if name.ends_with(".tar.xz") {
        Format::Xz
    } else if name.ends_with(".tar.zst") {
        Format::Zstd
    } else if name.ends_with(".tar.bz2") {
        Format::Bzip2
    } else {
        return Err(PackageError::Unsupported(format!("package member {name}")));
    })
}
pub fn parse_control(bytes: &[u8]) -> Result<PackageInfo> {
    if bytes.len() > MAX_METADATA {
        return Err(PackageError::MetadataLimit);
    }
    if bytes.contains(&0) {
        return Err(PackageError::Invalid("NUL in Debian control".into()));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| PackageError::Invalid("non-UTF-8 Debian control".into()))?;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut previous = None;
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            let key = previous
                .as_ref()
                .ok_or_else(|| PackageError::Invalid("orphan control continuation".into()))?;
            fields
                .get_mut(key)
                .unwrap()
                .push_str(&format!(" {}", line.trim()));
        } else if line.is_empty() {
            continue;
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| PackageError::Invalid("invalid control field".into()))?;
            let key = key.to_ascii_lowercase();
            if key.is_empty()
                || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || fields.insert(key.clone(), value.trim().into()).is_some()
            {
                return Err(PackageError::Invalid(
                    "duplicate or invalid control key".into(),
                ));
            }
            previous = Some(key);
        }
    }
    let name = fields
        .get("package")
        .ok_or_else(|| PackageError::Invalid("missing Package".into()))?
        .clone();
    common::package_name(&name)?;
    let version = fields
        .get("version")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| PackageError::Invalid("missing Version".into()))?
        .clone();
    let mut dependencies = Vec::new();
    for key in ["pre-depends", "depends"] {
        if let Some(value) = fields.get(key) {
            dependencies.extend(common::debian_dependencies(value)?);
        }
    }
    Ok(PackageInfo {
        name,
        version,
        dependencies,
        maintainer_scripts: Vec::new(),
        absolute_paths: fields
            .get("x-caskade-absolute-paths")
            .is_some_and(|s| matches!(s.as_str(), "yes" | "true")),
        format: Format::Deb,
    })
}

/// Validate all fixed ar headers before a library sees a claimed member size.
/// Debian's standard short member names avoid GNU/BSD name-table allocations.
pub fn ar_members(reader: &mut (impl Read + Seek)) -> Result<Vec<(String, Span)>> {
    let size = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != b"!<arch>\n" {
        return Err(PackageError::Invalid("not an ar archive".into()));
    }
    let mut cursor = 8_u64;
    let mut members = Vec::new();
    let mut names = BTreeSet::new();
    while cursor < size {
        if members.len() >= 64 {
            return Err(PackageError::MetadataLimit);
        }
        reader.seek(SeekFrom::Start(cursor))?;
        let mut header = [0; 60];
        reader.read_exact(&mut header)?;
        if &header[58..] != b"`\n" {
            return Err(PackageError::Invalid("invalid ar member header".into()));
        }
        let name = std::str::from_utf8(&header[..16])
            .map_err(|_| PackageError::Invalid("invalid ar member name".into()))?
            .trim()
            .trim_end_matches('/')
            .to_owned();
        if name.is_empty() || name.contains(['/', '\\', '#', '\0']) || !names.insert(name.clone()) {
            return Err(PackageError::Invalid("ambiguous ar member name".into()));
        }
        let length = std::str::from_utf8(&header[48..58])
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .ok_or_else(|| PackageError::Invalid("invalid ar member length".into()))?;
        let start = cursor.checked_add(60).ok_or(PackageError::MetadataLimit)?;
        let end = start
            .checked_add(length)
            .ok_or(PackageError::MetadataLimit)?;
        cursor = end
            .checked_add(length % 2)
            .ok_or(PackageError::MetadataLimit)?;
        if cursor > size {
            return Err(PackageError::Invalid("truncated ar member".into()));
        }
        members.push((
            name,
            Span {
                offset: start,
                length,
            },
        ));
    }
    Ok(members)
}

pub fn inspect(reader: &mut (impl Read + Seek)) -> Result<Deb> {
    let members = ar_members(reader)?;
    if members
        .first()
        .is_none_or(|(name, _)| name != "debian-binary")
    {
        return Err(PackageError::Invalid(
            "debian-binary must be the first member".into(),
        ));
    }
    let binary = members
        .iter()
        .find(|(n, _)| n == "debian-binary")
        .ok_or_else(|| PackageError::Invalid("missing debian-binary".into()))?;
    let mut version = [0; 4];
    reader.seek(SeekFrom::Start(binary.1.offset))?;
    if binary.1.length != 4 {
        return Err(PackageError::Invalid(
            "invalid Debian format version".into(),
        ));
    }
    reader.read_exact(&mut version)?;
    if &version != b"2.0\n" {
        return Err(PackageError::Unsupported(
            "Debian package format version".into(),
        ));
    }
    let controls: Vec<_> = members
        .iter()
        .filter(|(n, _)| n.starts_with("control.tar"))
        .collect();
    let data: Vec<_> = members
        .iter()
        .filter(|(n, _)| n.starts_with("data.tar"))
        .collect();
    if controls.len() != 1 || data.len() != 1 {
        return Err(PackageError::Invalid(
            "missing or ambiguous control/data archives".into(),
        ));
    }
    let (control_name, control_span) = controls[0];
    if control_span.offset >= data[0].1.offset {
        return Err(PackageError::Invalid(
            "control archive must precede data archive".into(),
        ));
    }
    if control_span.length > MAX_METADATA as u64 {
        return Err(PackageError::MetadataLimit);
    }
    // Read the selected ar member through the actual ar reader after preflight.
    reader.seek(SeekFrom::Start(0))?;
    let mut archive = ar::Archive::new(&mut *reader);
    let mut compressed = Vec::new();
    while let Some(entry) = archive.next_entry() {
        let mut entry = entry?;
        if entry
            .header()
            .identifier()
            .strip_suffix(b"/")
            .unwrap_or(entry.header().identifier())
            == control_name.as_bytes()
        {
            entry.read_to_end(&mut compressed)?;
            break;
        }
    }
    if compressed.len() as u64 != control_span.length {
        return Err(PackageError::Invalid("ar control member mismatch".into()));
    }
    let options = ArchiveOptions {
        max_unpacked_bytes: MAX_METADATA as u64,
        max_metadata_bytes: MAX_METADATA,
        ..ArchiveOptions::default()
    };
    let decoder = archive::decoded(
        Cursor::new(compressed),
        compression(control_name)?,
        &options,
    )?;
    let mut tar = tar::Archive::new(common::Budget::new(decoder, MAX_METADATA as u64));
    let mut info = None;
    let mut scripts = Vec::new();
    let mut count = 0;
    for entry in tar.entries()? {
        let mut entry = entry?;
        count += 1;
        if count > 128 {
            return Err(PackageError::MetadataLimit);
        }
        let path = entry.path()?;
        if path == std::path::Path::new(".") {
            continue;
        }
        let path = sanitize_path(
            path.to_str()
                .ok_or_else(|| PackageError::Invalid("control path encoding".into()))?,
        )?;
        if !entry.header().entry_type().is_file() {
            return Err(PackageError::Invalid(
                "non-file Debian control entry".into(),
            ));
        }
        if path == std::path::Path::new("control") {
            if info.is_some() {
                return Err(PackageError::Invalid("duplicate control record".into()));
            }
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_METADATA as u64 + 1)
                .read_to_end(&mut bytes)?;
            info = Some(parse_control(&bytes)?);
        } else if path.components().count() == 1
            && [
                "preinst", "postinst", "prerm", "postrm", "config", "triggers",
            ]
            .iter()
            .any(|s| path == std::path::Path::new(s))
        {
            scripts.push(path.to_string_lossy().into_owned());
        }
    }
    std::io::copy(&mut tar.into_inner(), &mut std::io::sink())?;
    let mut info =
        info.ok_or_else(|| PackageError::Invalid("control archive has no control record".into()))?;
    info.maintainer_scripts = scripts;
    Ok(Deb {
        info,
        data: data[0].1,
        format: compression(&data[0].0)?,
    })
}

pub fn extract(
    reader: &mut (impl Read + Seek),
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> Result<(PackageInfo, Vec<crate::ArtifactNode>)> {
    control.checkpoint(0)?;
    let package = inspect(reader)?;
    if package.info.absolute_paths {
        return Err(PackageError::RequiresDelegate(
            "package requires absolute system paths".into(),
        ));
    }
    let view = PayloadReader::new(reader, package.data.offset, package.data.length)?;
    let nodes = archive::extract(view, package.format, dir, options, control)?;
    Ok((package.info, nodes))
}
