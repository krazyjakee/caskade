//! Static Makeself extraction. Header text is data, never shell code.
mod adapter;
mod header;
use crate::{
    ArtifactNode, Format,
    formats::archive::{self, ArchiveError, ArchiveOptions, ExtractionControl, PayloadReader},
    safety::SafeDir,
};
pub use adapter::{MakeselfExtractor, MakeselfHandler, MakeselfProbe};
pub use header::{Family, Header, HeaderError, MAX_HEADER_BYTES, Span, parse_header};
use std::io::{Read, Seek, SeekFrom};

pub(crate) fn read_header(reader: &mut (impl Read + Seek)) -> archive::Result<Header> {
    let size = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = vec![0; size.min(MAX_HEADER_BYTES as u64) as usize];
    reader.read_exact(&mut bytes)?;
    parse_header(&bytes, size).map_err(|e| ArchiveError::Unsupported(e.to_string()))
}
pub(crate) fn payload_format(
    reader: &mut (impl Read + Seek),
    span: Span,
) -> archive::Result<Format> {
    let mut view = PayloadReader::new(reader, span.offset, span.length)?;
    let mut bytes = [0; 512];
    let mut length = 0;
    while length < bytes.len() {
        let count = view.read(&mut bytes[length..])?;
        if count == 0 {
            break;
        }
        length += count;
    }
    let bytes = &bytes[..length];
    let format = if bytes.starts_with(b"\x1f\x8b") {
        Format::Gzip
    } else if bytes.starts_with(b"\xfd7zXZ\0") {
        Format::Xz
    } else if bytes.starts_with(b"BZh") {
        Format::Bzip2
    } else if bytes.starts_with(b"\x28\xb5\x2f\xfd") {
        Format::Zstd
    } else if bytes.len() >= 262 && &bytes[257..262] == b"ustar" {
        Format::Tar
    } else {
        return Err(ArchiveError::Unsupported(
            "Makeself payload is not a supported tar stream".into(),
        ));
    };
    Ok(format)
}
/// Extract all embedded tar segments through one shared SafeDir budget. GOG
/// MojoSetup inputs instead select and strip only their appended game-data ZIP.
pub fn extract(
    reader: &mut (impl Read + Seek),
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> archive::Result<Vec<ArtifactNode>> {
    control.checkpoint(0)?;
    let header = read_header(reader)?;
    if header.family == Family::MojoSetup {
        return super::mojosetup::extract(reader, dir, options, control);
    }
    let mut nodes = Vec::new();
    for span in header.spans {
        control.checkpoint(0)?;
        let format = payload_format(reader, span)?;
        let view = PayloadReader::new(&mut *reader, span.offset, span.length)?;
        // extract re-probes all completed files. Keep only its latest complete
        // snapshot when multiple appended tar segments share the same budget.
        nodes = archive::extract(view, format, dir, options, control)?;
    }
    Ok(nodes)
}
