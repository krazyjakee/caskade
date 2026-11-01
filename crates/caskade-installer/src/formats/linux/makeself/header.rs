//! Conservative static parsing of Makeself's literal header metadata. This is
//! deliberately not a shell evaluator: substitutions and arithmetic never run.
pub const MAX_HEADER_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Makeself,
    MojoSetup,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub offset: u64,
    pub length: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub family: Family,
    pub spans: Vec<Span>,
    pub label: Option<String>,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HeaderError {
    #[error("not a recognised Makeself shell header")]
    Unrecognised,
    #[error("shell header exceeds the static parsing limit")]
    HeaderLimit,
    #[error("no literal offset or skip line count in shell header")]
    MissingOffset,
    #[error("invalid or conflicting shell header offsets")]
    InvalidOffset,
    #[error("invalid or conflicting shell payload lengths")]
    InvalidLength,
    #[error("encrypted shell payload is unsupported")]
    Encrypted,
}
pub fn recognised(bytes: &[u8]) -> Option<Family> {
    let bytes = &bytes[..bytes.len().min(MAX_HEADER_BYTES)];
    let first = bytes.split(|b| *b == b'\n').next()?;
    if !first.starts_with(b"#!") || !first.windows(2).any(|w| w == b"sh") {
        return None;
    }
    let mut text = String::new();
    let mut skip = None;
    let mut offset = None;
    let mut consumed = 0;
    for (index, raw) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if raw.contains(&0) {
            break;
        }
        let Ok(line) = std::str::from_utf8(raw) else {
            break;
        };
        text.push_str(line);
        consumed += raw.len();
        if let Some((key, value)) = line.trim().split_once('=') {
            match key.trim() {
                "skip" => skip = number(value),
                "offset" | "archive_offset" | "zip_offset" => offset = number(value),
                _ => (),
            }
        }
        if skip == Some(index as u64 + 1) || offset.is_some_and(|offset| consumed as u64 >= offset)
        {
            break;
        }
    }
    let text = text.to_ascii_lowercase();
    if text.contains("mojosetup") || text.contains("gog.com installer") {
        Some(Family::MojoSetup)
    } else if text.contains("makeself") || (text.contains("ms_dd") && text.contains("filesizes=")) {
        Some(Family::Makeself)
    } else {
        None
    }
}
fn literal(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = if value.starts_with(['\'', '"']) {
        let quote = value.as_bytes()[0] as char;
        value.strip_prefix(quote)?.strip_suffix(quote)?
    } else {
        value
    };
    if value.contains(['$', '`', '\\', ';', '\0']) {
        None
    } else {
        Some(value)
    }
}
fn number(value: &str) -> Option<u64> {
    let value = literal(value)?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}
fn assign<T: PartialEq>(
    slot: &mut Option<T>,
    value: T,
    error: HeaderError,
) -> Result<(), HeaderError> {
    if slot.as_ref().is_some_and(|previous| previous != &value) {
        return Err(error);
    }
    *slot = Some(value);
    Ok(())
}
pub fn parse_header(bytes: &[u8], file_size: u64) -> Result<Header, HeaderError> {
    let family = recognised(bytes).ok_or(HeaderError::Unrecognised)?;
    let bytes = &bytes[..bytes.len().min(MAX_HEADER_BYTES)];
    let mut offset = None;
    let mut skip = None;
    let mut sizes = None;
    let mut total = None;
    let mut label = None;
    let mut consumed = 0_usize;
    let mut line_count = 0_u64;
    let mut line_offset = None;
    for raw in bytes.split_inclusive(|b| *b == b'\n') {
        if raw.contains(&0) {
            break;
        }
        let Ok(line) = std::str::from_utf8(raw) else {
            break;
        };
        consumed += raw.len();
        line_count += 1;
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "offset" | "archive_offset" | "zip_offset" => {
                    if let Some(value) = number(value) {
                        assign(&mut offset, value, HeaderError::InvalidOffset)?;
                    }
                }
                "skip" => {
                    if let Some(value) = number(value) {
                        assign(&mut skip, value, HeaderError::InvalidOffset)?;
                    }
                }
                "filesizes" => {
                    if let Some(value) = literal(value) {
                        let values: Vec<_> =
                            value.split_whitespace().map(str::parse::<u64>).collect();
                        if values.is_empty()
                            || values.len() > 64
                            || values.iter().any(Result::is_err)
                        {
                            return Err(HeaderError::InvalidLength);
                        }
                        assign(
                            &mut sizes,
                            values
                                .into_iter()
                                .collect::<Result<Vec<_>, _>>()
                                .map_err(|_| HeaderError::InvalidLength)?,
                            HeaderError::InvalidLength,
                        )?;
                    }
                }
                "totalsize" => {
                    if let Some(value) = number(value) {
                        assign(&mut total, value, HeaderError::InvalidLength)?;
                    }
                }
                "label" => {
                    label = literal(value)
                        .filter(|s| s.len() <= 512 && !s.chars().any(char::is_control))
                        .map(str::to_owned);
                }
                "decrypt_cmd" if literal(value).is_some_and(|value| !value.is_empty()) => {
                    return Err(HeaderError::Encrypted);
                }
                _ => (),
            }
        }
        if skip == Some(line_count) {
            line_offset = Some(consumed as u64);
        }
        let boundary = offset.or(line_offset);
        if boundary.is_some_and(|value| consumed as u64 >= value) {
            break;
        }
    }
    let offset = match (offset, skip, line_offset) {
        (Some(offset), Some(_), Some(lines)) if offset != lines => {
            return Err(HeaderError::InvalidOffset);
        }
        (Some(offset), _, _) => offset,
        (None, Some(_), Some(lines)) => lines,
        (None, Some(_), None) => return Err(HeaderError::HeaderLimit),
        _ => return Err(HeaderError::MissingOffset),
    };
    if offset == 0
        || offset < consumed as u64
        || offset >= file_size
        || offset > MAX_HEADER_BYTES as u64
    {
        return Err(HeaderError::InvalidOffset);
    }
    let lengths = if family == Family::MojoSetup {
        // MojoSetup's Makeself bootstrap can be followed by another ZIP. The
        // ZIP's central directory is authoritative for that appended payload.
        vec![file_size - offset]
    } else {
        sizes.unwrap_or_else(|| vec![file_size - offset])
    };
    let mut cursor = offset;
    let mut spans = Vec::new();
    for length in lengths {
        if length == 0 {
            return Err(HeaderError::InvalidLength);
        }
        spans.push(Span {
            offset: cursor,
            length,
        });
        cursor = cursor
            .checked_add(length)
            .ok_or(HeaderError::InvalidLength)?;
        if cursor > file_size {
            return Err(HeaderError::InvalidLength);
        }
    }
    if family == Family::Makeself
        && (cursor != file_size || total.is_some_and(|total| total != cursor - offset))
    {
        return Err(HeaderError::InvalidLength);
    }
    Ok(Header {
        family,
        spans,
        label,
    })
}
