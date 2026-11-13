//! Static Makeself extraction. Header text is data, never shell code.
mod adapter;
mod header;
use crate::{
    Format,
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
    if let Some(format) = crate::probe::compression(bytes) {
        Ok(format)
    } else if bytes.get(257..262) == Some(b"ustar") {
        Ok(Format::Tar)
    } else {
        Err(ArchiveError::Unsupported(
            "Makeself payload is not a supported tar stream".into(),
        ))
    }
}
/// Extract all embedded tar segments through one shared SafeDir budget. GOG
/// MojoSetup inputs instead select and strip only their appended game-data ZIP.
pub fn extract(
    reader: &mut (impl Read + Seek),
    dir: &mut SafeDir,
    options: &ArchiveOptions,
    control: &mut dyn ExtractionControl,
) -> archive::Result<()> {
    control.checkpoint(0)?;
    let header = read_header(reader)?;
    if header.family == Family::MojoSetup {
        return super::mojosetup::extract(reader, dir, options, control);
    }
    for span in header.spans {
        control.checkpoint(0)?;
        let format = payload_format(reader, span)?;
        let view = PayloadReader::new(&mut *reader, span.offset, span.length)?;
        archive::extract(view, format, dir, options, control)?;
    }
    Ok(())
}
